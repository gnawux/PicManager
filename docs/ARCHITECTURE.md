# PicManager 架构设计

## 整体架构

PicManager 采用单进程、多模块的架构，同时提供 Web 界面和命令行界面。核心逻辑以库（lib crate）的形式组织，CLI 和 Web 服务器分别作为两个薄壳入口调用库功能。

```
┌─────────────────────────────────────────────────┐
│                   用户界面层                      │
│   ┌──────────────┐      ┌──────────────────┐    │
│   │   CLI (clap) │      │  Web UI (Axum)   │    │
│   └──────┬───────┘      └────────┬─────────┘    │
└──────────┼──────────────────────┼───────────────┘
           │                      │
┌──────────▼──────────────────────▼───────────────┐
│                   核心库 (lib)                    │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │ importer │ │ metadata │ │      dedup        │ │
│  └──────────┘ └──────────┘ └──────────────────┘ │
│  ┌──────────┐ ┌──────────────────────────────┐  │
│  │  album   │ │         storage (sqlx)        │  │
│  └──────────┘ └──────────────────────────────┘  │
└─────────────────────────────────────────────────┘
           │
┌──────────▼──────────────────────────────────────┐
│                   存储层                          │
│   ┌──────────────┐      ┌──────────────────┐    │
│   │  SQLite DB   │      │   照片文件系统     │    │
│   └──────────────┘      └──────────────────┘    │
└─────────────────────────────────────────────────┘
```

## 目录结构

```
picmanager/
├── src/
│   ├── main.rs              # CLI 入口
│   ├── lib.rs               # 库根，re-export 各模块
│   ├── config.rs            # 全局配置（数据目录、库路径等）
│   ├── error.rs             # 统一错误类型
│   ├── importer/
│   │   ├── mod.rs
│   │   ├── scanner.rs       # 扫描目录，过滤支持格式
│   │   └── state.rs         # 导入状态跟踪（已导入/未导入）
│   ├── metadata/
│   │   ├── mod.rs
│   │   ├── exif.rs          # EXIF 读取（时间、GPS、相机）
│   │   ├── format.rs        # 格式识别（magic bytes）
│   │   └── types.rs         # 元数据结构体
│   ├── dedup/
│   │   ├── mod.rs
│   │   ├── hash.rs          # 感知哈希（pHash）+ 精确哈希
│   │   └── candidate.rs     # 重复候选，等待人工确认
│   ├── album/
│   │   ├── mod.rs
│   │   ├── organize.rs      # 按时间/地点/相机自动分组
│   │   └── merge.rs         # 手动合并相册
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── db.rs            # SQLite 连接池
│   │   └── migrations/      # sqlx migrate 文件
│   └── web/
│       ├── mod.rs
│       ├── server.rs        # Axum 服务器启动
│       ├── routes.rs        # 路由注册
│       └── handlers/
│           ├── import.rs
│           ├── photos.rs
│           ├── dedup.rs
│           └── albums.rs
├── frontend/                # Web 前端（待定：静态 HTML 或 JS 框架）
├── migrations/              # SQLx 数据库迁移文件
├── docs/
└── tests/                   # 集成测试
```

## 核心模块说明

### importer — 导入模块

负责从指定目录扫描照片文件，写入数据库并跟踪导入状态。

**流程：**
1. 递归扫描目录，按文件扩展名 + magic bytes 过滤支持格式
2. 对每张照片计算内容哈希，查询数据库判断是否已导入
3. 新照片：复制到库目录，提取元数据，写入数据库，状态标记为 `imported`
4. 源文件不做修改或删除，状态信息供用户手工决策

**导入状态（`import_status`）：**

| 状态 | 含义 |
|------|------|
| `pending` | 发现但尚未处理 |
| `imported` | 已成功导入库 |
| `duplicate` | 与库中已有照片重复，跳过 |
| `error` | 处理失败 |

### metadata — 元数据提取

从各格式文件中提取结构化元数据。

- **时间**：优先读 EXIF DateTimeOriginal，fallback 到文件修改时间
- **地点**：读取 GPS IFD，转换为十进制坐标；可选逆地理编码为地名
- **相机**：读取 Make + Model 字段，规范化品牌名称
- **格式探测**：magic bytes 优先，扩展名辅助

**支持格式：**

| 格式 | 库 |
|------|-----|
| JPG/JPEG | `kamadak-exif` |
| ARW（索尼 RAW）| `rawler` 或 `libraw` bindings |
| HEIC / Live Photo | `libheif-rs`（依赖系统 libheif）|
| WebP / PNG / GIF | `image` crate |

### dedup — 去重模块

两阶段去重，平衡召回率和精度：

1. **精确去重**：SHA-256 内容哈希，直接判定完全相同文件
2. **感知去重**：pHash（感知哈希），汉明距离 ≤ 10 判定为视觉相似（覆盖缩放、轻度裁剪等情况）

去重结果仅写入数据库标记为候选，**不自动删除**，需用户在 Web 界面或 CLI 逐一确认后才执行删除。

### album — 相册模块

**自动分组维度：**
- 时间：按年/月（可配置粒度）
- 地点：按 GPS 聚类（k-means 或网格分区），或按逆地理编码的城市/地区
- 相机：按 Make+Model

相册之间可多对多关联，一张照片可属于多个相册（时间相册 + 地点相册）。

手动合并：将两个相册的照片合并到新相册，原相册记录保留供审计。

### storage — 数据库

使用 SQLite，通过 `sqlx` 访问（编译时查询检查）。

**核心表：**

```sql
photos          -- 照片主记录（路径、哈希、元数据、导入状态）
albums          -- 相册
photo_albums    -- 照片-相册 多对多关联
dedup_groups    -- 重复候选组
import_sessions -- 导入会话记录
```

数据库文件和照片库存放在用户指定的数据目录（默认 `~/Pictures/PicManager/`）。

### web — Web 服务

基于 **Axum** 提供 REST API，前端为静态文件（初期用简单 HTML+JS，后续可升级）。

**主要 API：**

```
POST   /api/import              # 触发导入任务
GET    /api/import/status       # 查询导入进度
GET    /api/photos              # 照片列表（分页、过滤）
GET    /api/photos/:id          # 单张照片详情
GET    /api/photos/:id/thumb    # 缩略图
GET    /api/dedup               # 重复候选列表
POST   /api/dedup/:group/resolve # 确认删除/保留
GET    /api/albums              # 相册列表
POST   /api/albums/merge        # 合并相册
```

长耗时任务（导入、去重扫描）通过 **tokio 后台任务** 异步执行，进度通过轮询或 SSE 推送。

## 数据流

### 导入流程

```
用户指定目录
    │
    ▼
scanner：递归遍历文件
    │
    ▼
format：格式识别过滤
    │
    ▼
hash：计算 SHA-256
    │
    ├─── 已存在 → 标记 duplicate，跳过
    │
    ▼
metadata：提取 EXIF
    │
    ▼
storage：写入 photos 表，状态 imported
    │
    ▼
album：按元数据归入自动相册
    │
    ▼
dedup：计算 pHash，检查感知重复候选
```

### 去重确认流程

```
用户查看重复候选列表
    │
    ▼
选择保留哪张 / 全部保留 / 删除哪张
    │
    ▼
写入确认结果到 dedup_groups
    │
    ▼
执行文件删除（仅库内副本，不影响源目录）
    │
    ▼
更新 photos 表状态
```

## 关键技术选型

| 用途 | 选型 | 理由 |
|------|------|------|
| HTTP 框架 | Axum | 异步、与 tokio 生态契合 |
| 数据库 | SQLite + sqlx | 单机、零部署、编译期查询检查 |
| 异步运行时 | tokio | Rust 生态标准选择 |
| CLI | clap | 成熟、声明式 |
| EXIF | kamadak-exif | 纯 Rust，无 C 依赖 |
| 图像处理 | image crate | 支持多格式，纯 Rust |
| HEIC | libheif-rs | 通过系统 libheif 支持苹果格式 |
| 感知哈希 | img_hash | 纯 Rust pHash 实现 |
| 日志 | tracing | 结构化日志，适配 tokio |

## 部署模型

单一二进制，包含 Web 服务器和 CLI 两种模式：

```bash
picmanager serve              # 启动 Web 服务（默认 127.0.0.1:8080）
picmanager import <dir>       # 命令行导入
picmanager dedup              # 命令行触发去重扫描
```

照片库数据目录通过 `--library` 参数或配置文件指定，默认 `~/Pictures/PicManager/`。
