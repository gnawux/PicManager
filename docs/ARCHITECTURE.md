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
│   ├── main.rs              # CLI 入口（import [--copy] / dedup / serve / config）
│   ├── lib.rs               # 库根，re-export 各模块
│   ├── config.rs            # 全局配置（库路径、端口、缩略图尺寸等）
│   ├── error.rs             # 统一错误类型 AppError
│   ├── importer/
│   │   ├── mod.rs           # import_dir()，串联导入流水线
│   │   ├── placer.rs        # 文件移动/复制，冲突重命名，按日期目录放置
│   │   ├── scanner.rs       # 递归扫描目录，magic bytes 过滤格式
│   │   └── state.rs         # SHA-256 计算，导入状态决策（按 sha256 去重）
│   ├── metadata/
│   │   ├── mod.rs
│   │   ├── exif.rs          # EXIF 读取（时间多字段回退链、GPS、相机）
│   │   ├── filename.rs      # 从文件名推断拍摄日期（Unix 时间戳 / 紧凑 / 分隔符）
│   │   ├── format.rs        # 格式识别（magic bytes）
│   │   └── types.rs         # ImageFormat 枚举、PhotoMeta 结构体
│   ├── dedup/
│   │   ├── mod.rs           # list_groups()、resolve()
│   │   ├── hash.rs          # 感知哈希（dHash）计算与汉明距离
│   │   └── candidate.rs     # 重复候选扫描，写入 dedup_groups 表
│   ├── album/
│   │   ├── mod.rs
│   │   ├── organize.rs      # 按月份、按相机自动分组
│   │   ├── location.rs      # 按 GPS 地点自动分组（Nominatim 反地理编码）
│   │   └── merge.rs         # 手动合并相册
│   ├── storage/
│   │   ├── mod.rs
│   │   └── db.rs            # SQLite 连接池，运行迁移
│   └── web/
│       ├── mod.rs           # AppState、router()、serve()
│       ├── embed.rs         # rust-embed 静态文件服务（frontend/ 编译进二进制）
│       └── handlers/
│           ├── import.rs    # POST /api/import、GET /api/import/status
│           ├── photos.rs    # GET /api/photos、GET /api/photos/:id/thumb
│           ├── dedup.rs     # GET /api/dedup、POST /api/dedup/:id/resolve
│           └── albums.rs    # GET /api/albums、GET /api/albums/:id/photos、POST /api/albums/merge
├── frontend/                # 静态 HTML + CSS + JS（编译时嵌入二进制）
├── migrations/              # SQLx 数据库迁移文件
├── tests/                   # 集成测试与测试 fixture
└── docs/                    # 架构设计与开发计划
```

## 核心模块说明

### importer — 导入模块

负责从指定目录扫描照片文件，将其整理到库目录并写入数据库。

**流程：**
1. 递归扫描目录，按文件扩展名 + magic bytes 过滤支持格式
2. 对每张照片计算 SHA-256，查询数据库判断是否已导入（相同 sha256 即跳过）
3. 新照片：三级日期推断（见下），**移动**文件到 `{library}/{yyyy-mm-dd}/`（传 `--copy` 则复制），提取元数据，写入数据库，状态标记为 `imported`
4. 数据库记录的 `path` 为文件在库内的最终路径

**拍摄日期三级推断链：**

| 优先级 | 来源 | 说明 |
|--------|------|------|
| 1 | EXIF 字段（多字段回退） | DateTimeOriginal → DateTimeDigitized → GPS DateStamp+TimeStamp → DateTime |
| 2 | 文件名模式 | Unix 时间戳（10/13位）/ YYYYMMDD_HHMMSS / YYYY-MM-DD |
| 3 | 无法推断 | 放入 `{library}/unknown/` |

**导入状态（`import_status`）：**

| 状态 | 含义 |
|------|------|
| `imported` | 已成功导入库 |
| `deleted` | 去重确认后被软删除 |

### metadata — 元数据提取

从各格式文件中提取结构化元数据。

- **时间**：EXIF 四字段回退链（DateTimeOriginal → DateTimeDigitized → GPS DateStamp+TimeStamp → DateTime）；EXIF 全部缺失时退回到文件名推断（`filename.rs`）
- **地点**：读取 GPS IFD，转换为十进制坐标；逆地理编码由 `album/location.rs` 负责
- **相机**：读取 Make + Model 字段，规范化品牌名称
- **格式探测**：magic bytes 优先，扩展名辅助

**支持格式：**

| 格式 | 库 |
|------|-----|
| JPG/JPEG | `kamadak-exif` |
| ARW（索尼 RAW）| `kamadak-exif`（ARW 内嵌标准 EXIF） |
| HEIC / Live Photo | `kamadak-exif`（依赖系统 `libheif`）|
| WebP / PNG / GIF | `image` crate |

### dedup — 去重模块

两阶段去重，平衡召回率和精度：

1. **精确去重**：SHA-256 内容哈希，直接判定完全相同文件
2. **感知去重**：pHash（感知哈希），汉明距离 ≤ 10 判定为视觉相似（覆盖缩放、轻度裁剪等情况）

去重结果仅写入数据库标记为候选，**不自动删除**，需用户在 Web 界面或 CLI 逐一确认后才执行删除。

### album — 相册模块

**自动分组维度：**
- 时间（`kind='time'`）：按 `taken_at` 年月，形如 `2024-06`
- 地点（`kind='location'`）：调用 OSM Nominatim 反地理编码将 GPS 坐标解析为城市名；结果缓存到 `geocache` 表，限速 1 req/s；无 GPS 的照片跳过
- 相机（`kind='camera'`）：按 EXIF Make+Model；无相机信息的照片跳过

一张照片可同时属于多个相册（时间相册 + 地点相册 + 相机相册），通过 `photo_albums` 多对多表关联。

手动合并：将 source 相册的所有照片并入 target，source 相册记录随之删除。

### storage — 数据库

使用 SQLite，通过 `sqlx` 访问（编译时查询检查）。

**核心表：**

```sql
photos          -- 照片主记录（路径、sha256、phash、元数据、导入状态）
albums          -- 相册（name、kind：time / camera / location）
photo_albums    -- 照片-相册 多对多关联
dedup_groups    -- 重复候选组（status：pending / resolved）
dedup_members   -- 重复组成员（group_id、photo_id、keep 标记）
import_sessions -- 导入会话记录
geocache        -- GPS 坐标 → 城市名缓存（lat_key、lon_key、city）
```

数据库文件和照片库存放在用户指定的数据目录（默认 `~/Pictures/PicManager/`）。

### web — Web 服务

基于 **Axum** 提供 REST API，前端为静态文件（初期用简单 HTML+JS，后续可升级）。

**主要 API：**

```
POST   /api/import                   # 触发后台导入任务（tokio::spawn）
GET    /api/import/status            # 轮询导入进度
GET    /api/photos                   # 照片列表（分页）
GET    /api/photos/:id/thumb         # 300px JPEG 缩略图
GET    /api/dedup                    # 待确认重复组列表
POST   /api/dedup/:group_id/resolve  # 确认保留（软删除其余项）
GET    /api/albums                   # 相册列表（含 photo_count）
GET    /api/albums/:id/photos        # 相册内照片列表（分页）
POST   /api/albums/merge             # 合并相册
```

长耗时任务（导入）通过 **tokio::spawn** 后台执行，进度通过客户端轮询 `/api/import/status` 获取。

## 数据流

### 导入流程

```
用户指定目录
    │
    ▼
scanner：递归遍历，magic bytes 过滤支持格式
    │
    ▼
state：计算 SHA-256
    │
    ├─── sha256 已存在于 DB → 跳过（幂等）
    │
    ▼
metadata：提取 EXIF（时间四字段回退链、GPS、相机）
    │
    ▼
日期推断（三级）：EXIF taken_at → 文件名模式 → None
    │
    ▼
placer：将文件移动（或 --copy 时复制）到 {library}/{yyyy-mm-dd}/ 或 unknown/
    │
    ▼
dedup/hash：计算感知哈希（pHash），写入 photos.phash
    │
    ▼
storage：写入 photos 表（path = 库内新路径），import_status = 'imported'
    │
    ▼
album：按月份自动分组 → 按相机自动分组 → 按 GPS 地点自动分组
```

### 去重确认流程

```
picmanager dedup（CLI）或 GET /api/dedup（Web）
    │
    ▼
dedup::scan()：O(n²) 比较 phash 汉明距离，写入 dedup_groups / dedup_members
    │
    ▼
用户查看重复候选，选择保留哪张
    │
    ▼
dedup::resolve()：将未保留项的 import_status 置为 'deleted'（软删除，不操作文件）
    │
    ▼
dedup_groups.status 置为 'resolved'，不再出现在候选列表中
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
| HEIC | kamadak-exif + 系统 libheif | 依赖 brew install libheif |
| 感知哈希 | image_hasher | dHash（Gradient），纯 Rust |
| 反地理编码 | reqwest + OSM Nominatim | GPS → 城市名，结果缓存到 geocache 表 |
| 日志 | tracing | 结构化日志，适配 tokio |

## 部署模型

单一二进制，包含 Web 服务器和 CLI 两种模式：

```bash
picmanager serve              # 启动 Web 服务（默认 127.0.0.1:8080）
picmanager import <dir>       # 命令行导入（默认 move 文件）
picmanager import --copy <dir># 命令行导入（复制，保留源文件）
picmanager dedup              # 命令行触发去重扫描
picmanager config             # 显示当前生效配置
```

照片库数据目录由配置文件指定，默认 `~/Pictures/PicManager/`。前端静态文件编译进二进制，无需额外部署。
