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
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │  album   │ │   face   │ │     storage       │ │
│  └──────────┘ └──────────┘ └──────────────────┘ │
│  ┌──────────┐                                    │
│  │  animal  │                                    │
│  └──────────┘                                    │
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
│   ├── main.rs              # CLI 入口（import / dedup / faces / models / serve / config）
│   ├── lib.rs               # 库根，re-export 各模块
│   ├── config.rs            # 全局配置（库路径、端口、缩略图尺寸等）
│   ├── error.rs             # 统一错误类型 AppError（含 ModelNotFound）
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
│   ├── face/
│   │   ├── mod.rs           # analyze_one()，re-export detector/embedder/job
│   │   ├── detector.rs      # detect()，ultraface-slim-320，OnceLock<Mutex<Session>>
│   │   ├── embedder.rs      # Embedder::load/extract，ArcFace，512D L2 归一化
│   │   ├── job.rs           # run_job()，execute_job()，批量重分析
│   │   └── cluster.rs       # DBSCAN 聚类（cosine 距离），run_clustering(pool)
│   ├── animal/
│   │   ├── mod.rs           # detect_and_save()，导入时调用，模型不存在时静默跳过
│   │   └── detector.rs      # detect()，YOLOv8-nano，OnceLock<Mutex<Session>>
│   ├── storage/
│   │   ├── mod.rs
│   │   └── db.rs            # SQLite 连接池，运行迁移
│   └── web/
│       ├── mod.rs           # AppState（含 geo_running: Arc<AtomicBool>）、router()、serve()
│       ├── embed.rs         # rust-embed 静态文件服务（frontend/ 编译进二进制）
│       └── handlers/
│           ├── import.rs    # POST /api/import、GET /api/import/status
│           ├── photos.rs    # GET /api/photos、GET /api/photos/:id/thumb、GET/PATCH /api/photos/:id、POST /api/photos/batch-update、GET /api/photos/gps-points
│           ├── dedup.rs     # GET /api/dedup、POST /api/dedup/:id/resolve
│           ├── albums.rs    # GET /api/albums、GET /api/albums/:id/photos、POST /api/albums/merge
│           ├── faces.rs     # POST /api/faces/analyze（支持 missing_only）、GET /api/faces/jobs/:id、GET /api/photos/:id/faces
│           ├── people.rs    # GET /api/people（含 status 过滤）、GET /api/people/tree、POST /api/people/cluster/merge、PATCH /api/people/:id、POST /api/people/batch-update、GET /api/people/:id、POST /api/people/:id/reparent、GET /api/faces/:id/thumb
│           ├── geo.rs       # GET /api/geo/hierarchy、POST /api/geo/regeocode、GET /api/geo/regeocode/status
│           └── animals.rs   # GET /api/animals/species、GET /api/animals/:species/photos、GET /api/photos/:id/animals
├── frontend/                # 静态 HTML + CSS + JS（编译时嵌入二进制）
├── migrations/              # SQLx 数据库迁移文件（0001–0010）
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

**扫描模式：**

| 模式 | 函数 | 适用场景 | 复杂度 |
|------|------|---------|--------|
| 增量扫描 | `scan()` | 每次导入后运行（默认） | O(new × old + new²) |
| 全量重扫 | `scan_full()` | `dedup --full`，首次或修复数据 | O(n × 548 × avg_bucket) |

`scan()` 仅比较 `dedup_scanned_at IS NULL` 的新照片，扫描后打上时间戳；若无新照片则为 O(1) 无操作。

`scan_full()` 使用多索引哈希（4 × 16-bit 分段，Hamming 距离 ≤ 2 邻桶查找），鸽巢原理保证距离 ≤ 10 的任意对至少在一个分段命中，无漏报。

### album — 相册模块

**自动分组维度：**
- 时间（`kind='time'`）：按 `taken_at` 年月，形如 `2024-06`
- 地点（`kind='location'`）：调用 OSM Nominatim 反地理编码将 GPS 坐标解析为城市名；结果缓存到 `geocache` 表，限速 1 req/s；精确 key 未命中时先在 ±0.01°（约 1 km）范围内查邻近缓存（proximity lookup），命中则写入精确 key 并返回，避免 API 调用（实测覆盖约 89% 的坐标）；无 GPS 的照片跳过；`count_missing_geo(pool)` 返回有 GPS 但尚无缓存的照片数，供 CLI `fill-missing` 使用
- 相机（`kind='camera'`）：按 EXIF Make+Model；无相机信息的照片跳过

一张照片可同时属于多个相册（时间相册 + 地点相册 + 相机相册），通过 `photo_albums` 多对多表关联。

手动合并：将 source 相册的所有照片并入 target，source 相册记录随之删除。

### face — 人脸模块

本地离线人脸检测与特征提取，模型文件存放于 `{config_dir}/picmanager/models/`。

**检测（`detector.rs`）**：
- 模型：ultraface-slim-320（约 1 MB），输入 `[1,3,240,320]` BGR float32
- 输出：`scores [1,4420,2]` + `boxes [1,4420,4]`（归一化坐标）
- 后处理：置信度 ≥ 0.5 过滤 + IoU NMS（阈值 0.45）
- `OnceLock<Option<Mutex<Session>>>` 懒加载；模型不存在时安静返回 `[]`，不 panic

**特征提取（`embedder.rs`）**：
- 模型：ArcFace MobileNetV1（约 10 MB），输入 112×112 RGB，输出 512D float32
- 预处理：按检测框裁剪（扩边 20%）→ resize 112×112 → 归一化 `[-1,1]`
- 后处理：L2 归一化（保证余弦相似度 = 点积）
- embedding 以小端序 `Vec<f32>` 存入 `faces.embedding` BLOB（2048 字节/512 维）
- 模型不存在时返回 `AppError::ModelNotFound`，导入流程中 embedding 留 NULL

**批量作业（`job.rs`）**：
- `run_job(pool, scope)` 在 `tokio::spawn` 中异步执行，立即返回 `job_id`
- `scope_for_missing(pool)` 返回所有尚无 faces 记录的已导入照片 ID，供 CLI `fill-missing` 使用
- 每张照片先 DELETE 旧 faces 行再重新检测+嵌入，保证不累积

**人物聚类（`cluster.rs`）**：
- DBSCAN 算法，距离度量为 `1.0 - dot(a, b)`（L2 归一化嵌入，等价于余弦距离）
- 默认 `eps = 0.4`，`min_samples = 2`（含点自身，即至少 1 个其他邻居）
- 噪点各自单独成组，便于用户后续手动合并
- `run_clustering(pool)`：清空 `people` / `person_faces`，重新聚类所有有 embedding 的人脸，写入新人物记录

### animal — 动物检测模块

本地离线动物目标检测，模型文件存放于 `{config_dir}/picmanager/models/yolov8n.onnx`（约 6 MB）。

**检测（`detector.rs`）**：
- 模型：YOLOv8-nano ONNX
- 输入：`[1, 3, 640, 640]` float32 归一化 `[0,1]` RGB
- 输出：`[1, 84, 8400]`，feature-first 布局；前 4 行为 cx/cy/w/h，后 80 行为 COCO 类别分数
- 只关注动物类（COCO class 14–23）：bird / cat / dog / horse / sheep / cow / elephant / bear / zebra / giraffe
- 后处理：置信度 ≥ 0.4 过滤 + IoU NMS（阈值 0.45）
- `OnceLock<Option<Mutex<Session>>>` 懒加载，与 face detector 同一模式；模型不存在时安静返回 `[]`

**导入集成（`mod.rs`）**：
- `detect_and_save(pool, photo_id, img)` 在 `face::analyze_one` 完成后调用
- 检测结果写入 `animals` 表（species、confidence、bbox）
- 模型不存在时跳过并 `tracing::warn!`，不中断导入

### storage — 数据库

使用 SQLite，通过 `sqlx` 访问（编译时查询检查）。

**核心表：**

```sql
photos          -- 照片主记录（路径、sha256、phash、元数据、导入状态、dedup_scanned_at、timezone_offset）
albums          -- 相册（name、kind：time / camera / location）
photo_albums    -- 照片-相册 多对多关联
dedup_groups    -- 重复候选组（status：pending / resolved）
dedup_members   -- 重复组成员（group_id、photo_id、keep 标记）
import_sessions -- 导入会话记录
geocache        -- GPS 坐标 → 城市名缓存（lat_key、lon_key、city、country、state、county）
faces           -- 人脸区域（photo_id、bbox、confidence、embedding BLOB、embed_model）
face_jobs       -- 人脸批量分析任务（status、scope、total/processed 进度）
photo_stats     -- 单行计数器（active_count），替代全表 COUNT(*) 查询
people          -- 人物记录（name、status active/ignored/not_a_person、parent_id 支持树状层级、cover_face_id）
person_faces    -- 人物-人脸多对多关联
animals         -- 动物检测结果（photo_id、species、confidence、bbox、detected_at）
```

数据库文件和照片库存放在用户指定的数据目录（默认 `~/Pictures/PicManager/`）。

### web — Web 服务

基于 **Axum** 提供 REST API，前端为静态文件（初期用简单 HTML+JS，后续可升级）。

**主要 API：**

```
POST   /api/import                        # 触发后台导入任务（tokio::spawn）
GET    /api/import/status                 # 轮询导入进度
GET    /api/photos                        # 照片列表（分页）
GET    /api/photos/gps-points             # 所有有 GPS 的照片坐标列表
POST   /api/photos/batch-update           # 批量修改时间/时区
GET    /api/photos/:id                    # 单张照片详情
PATCH  /api/photos/:id                    # 修改照片时间/时区
GET    /api/photos/:id/thumb              # 300px JPEG 缩略图
GET    /api/photos/:id/faces              # 该照片的所有人脸区域
GET    /api/photos/:id/animals            # 该照片的所有动物检测结果
GET    /api/dedup                         # 待确认重复组列表
POST   /api/dedup/:group_id/resolve       # 确认保留（软删除其余项）
GET    /api/albums                        # 相册列表（含 photo_count）
GET    /api/albums/:id/photos             # 相册内照片列表（分页）
POST   /api/albums/merge                  # 合并相册
POST   /api/faces/analyze                 # 触发人脸重分析任务（全库、指定 photo_ids 或 missing_only）
GET    /api/faces/jobs/:id                # 轮询重分析任务进度
GET    /api/faces/:id/thumb               # 人脸裁剪缩略图（磁盘缓存）
GET    /api/geo/hierarchy                 # 地理层级（country→state→city 含照片数）
POST   /api/geo/regeocode                 # 为有 GPS 但缺 geocache 的照片触发后台反地理编码
GET    /api/geo/regeocode/status          # 查询反地理编码后台任务是否在运行
GET    /api/people                        # 人物列表（支持 ?status=active|ignored|not_a_person|all 和 ?name_exact=xxx）
GET    /api/people/tree                   # 人物树（嵌套 JSON）
POST   /api/people/cluster                # 触发 DBSCAN 重聚类
POST   /api/people/merge                  # 合并两个人物
PATCH  /api/people/:id                    # 修改人物姓名或状态
POST   /api/people/batch-update           # 批量修改人物状态
GET    /api/people/:id                    # 人物照片列表
POST   /api/people/:id/reparent           # 变更人物父节点
GET    /api/animals/species               # 动物种类列表（含照片数）
GET    /api/animals/:species/photos       # 指定种类的照片列表
```

长耗时任务（导入、人脸分析、反地理编码）通过 **tokio::spawn** 后台执行，进度通过客户端轮询对应 status API 获取。导入和地理编码各用 `Arc<Mutex<>>` / `Arc<AtomicBool>` 防止并发重入。

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
  photo_stats.active_count += 1
    │
    ▼
face::analyze_one：detect() → 写入 faces 表 → 若模型可用则提取 embedding
    │
    ▼
animal::detect_and_save：YOLOv8-nano 检测 → 写入 animals 表（模型不存在时跳过）
    │
    ▼
album：按月份自动分组 → 按相机自动分组 → 按 GPS 地点自动分组
```

### 去重确认流程

```
picmanager dedup（CLI）或 GET /api/dedup（Web）
    │
    ▼
dedup::scan()：
  仅比较 dedup_scanned_at IS NULL 的新照片（增量）
  → 对每对计算 phash 汉明距离，写入 dedup_groups / dedup_members
  → 打上 dedup_scanned_at 时间戳
  无新照片时立即返回 0（O(1) 无操作）

  picmanager dedup --full：
    scan_full() 重置所有时间戳，使用多索引分桶全量重扫
    │
    ▼
用户查看重复候选，选择保留哪张
    │
    ▼
dedup::resolve()：
  将未保留项的 import_status 置为 'deleted'（软删除，不操作文件）
  photo_stats.active_count -= 软删除数量
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
| ONNX 推理 | ort 2.0.0-rc.12 (load-dynamic) | 人脸检测 + 特征提取，动态加载 libonnxruntime |
| 数值计算 | ndarray 0.17 | ort RC.12 的 ndarray 依赖版本，需严格匹配 |
| 日志 | tracing | 结构化日志，适配 tokio |

## 部署模型

单一二进制，包含 Web 服务器和 CLI 两种模式：

```bash
picmanager serve                            # 启动 Web 服务（默认 127.0.0.1:8080）
picmanager import <dir>                     # 命令行导入（默认 move 文件）
picmanager import --copy <dir>              # 命令行导入（复制，保留源文件）
picmanager dedup                            # 命令行增量去重扫描
picmanager dedup --full                     # 全量重扫（多索引分桶）
picmanager faces analyze                    # 全库人脸重分析
picmanager faces analyze --photo-ids 1,2,3  # 指定照片人脸重分析
picmanager fill-missing                     # 为缺人脸或地理元数据的照片批量补全
picmanager fill-missing --faces             # 仅补充未分析人脸的照片
picmanager fill-missing --geo               # 仅补充缺地理编码的照片
picmanager models fetch                     # 下载 ONNX 模型文件到 config_dir/models/
picmanager config                           # 显示当前生效配置
```

照片库数据目录由配置文件指定，默认 `~/Pictures/PicManager/`。前端静态文件编译进二进制，无需额外部署。
