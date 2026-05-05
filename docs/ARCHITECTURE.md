# PicManager 架构设计

## 整体架构

PicManager 由两个独立工具组成：**picmanager**（Rust，核心库）负责照片库管理，**photobridge**（Swift CLI）负责从 macOS 照片库导出照片并送入 picmanager。两者均为单一二进制，通过文件系统（暂存目录）和子进程调用（`picmanager import`）交互。

```
┌─────────────────────────────────────────────────────────────┐
│                    外部数据源                                  │
│   ┌─────────────────────────────────────────────────────┐   │
│   │  photobridge（Swift CLI）                            │   │
│   │  export / sync / status / fix-timestamps / setup    │   │
│   │  ↓ PHPhotoLibrary / iCloud Photos                   │   │
│   │  ↓ 导出到暂存目录，设置 mtime = asset.creationDate   │   │
│   │  ↓ 调用 picmanager import（子进程）                   │   │
│   └─────────────────────────────────────────────────────┘   │
│                         │（暂存目录）                          │
└─────────────────────────┼───────────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────────┐
│                   picmanager 用户界面层                        │
│   ┌──────────────┐      ┌──────────────────┐                │
│   │   CLI (clap) │      │  Web UI (Axum)   │                │
│   └──────┬───────┘      └────────┬─────────┘                │
└──────────┼──────────────────────┼─────────────────────────--┘
           │                      │
┌──────────▼──────────────────────▼───────────────────────────┐
│                   核心库 (lib)                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐            │
│  │ importer │ │ metadata │ │      dedup        │            │
│  └──────────┘ └──────────┘ └──────────────────┘            │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐            │
│  │  album   │ │   face   │ │     storage       │            │
│  └──────────┘ └──────────┘ └──────────────────┘            │
│  ┌──────────┐                                               │
│  │  animal  │                                               │
│  └──────────┘                                               │
└──────────────────────────────────────────────────────────---┘
           │
┌──────────▼──────────────────────────────────────────────────┐
│                   存储层                                       │
│   ┌──────────────┐      ┌──────────────────┐               │
│   │  SQLite DB   │      │   照片文件系统     │               │
│   └──────────────┘      └──────────────────┘               │
└─────────────────────────────────────────────────────────────┘
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
│   │   ├── mod.rs           # import_dir()、import_dir_with_progress()、import_dir_batch()，串联导入流水线
│   │   ├── log.rs           # MigrationLog（NDJSON 导入日志，断点续传）
│   │   ├── placer.rs        # 文件移动/复制，冲突重命名，按日期目录放置
│   │   ├── scanner.rs       # 递归扫描目录，magic bytes 过滤格式
│   │   └── state.rs         # SHA-256 计算，导入状态决策（按 sha256 去重）
│   ├── metadata/
│   │   ├── mod.rs           # re-export + mtime_to_naive_datetime()
│   │   ├── exif.rs          # EXIF 读取（时间多字段回退链、GPS、相机、exif_orientation）
│   │   ├── filename.rs      # 从文件名推断拍摄日期（Unix 时间戳 / 紧凑 / 分隔符）
│   │   ├── format.rs        # 格式识别（magic bytes）
│   │   └── types.rs         # ImageFormat 枚举、PhotoMeta 结构体
│   ├── dedup/
│   │   ├── mod.rs           # list_groups()、resolve()
│   │   ├── hash.rs          # Layer-1 Gradient pHash + Layer-2 DCT pHash；is_degenerate()
│   │   └── candidate.rs     # scan()增量扫描、scan_full()多索引分桶全扫；Union-Find 聚类
│   ├── album/
│   │   ├── mod.rs
│   │   ├── organize.rs      # 按月份、按相机自动分组
│   │   ├── location.rs      # 按 GPS 地点自动分组（Nominatim + 邻近缓存）
│   │   └── merge.rs         # 手动合并相册
│   ├── face/
│   │   ├── mod.rs           # analyze_one()、apply_exif_orientation()、apply_transform()
│   │   ├── detector.rs      # detect()，ultraface-slim-320，OnceLock<Mutex<Session>>
│   │   ├── embedder.rs      # Embedder::load/extract，ArcFace，512D L2 归一化
│   │   ├── job.rs           # run_job()、execute_job()、reanalyze_one_photo()，批量重分析
│   │   └── cluster.rs       # 两阶段 DBSCAN；run_clustering()；run_incremental_clustering()
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
│           ├── photos.rs    # GET /api/photos、GET /api/photos/:id/thumb、GET /api/photos/:id/file、GET/PATCH /api/photos/:id、POST /api/photos/batch-update、GET /api/photos/gps-points
│           ├── dedup.rs     # GET /api/dedup、POST /api/dedup/:id/resolve
│           ├── albums.rs    # GET /api/albums（含 latest_photo_at）、GET /api/albums/:id/photos、POST /api/albums/merge
│           ├── collections.rs # GET/POST /api/collections、PATCH/DELETE /api/collections/:id、GET/POST/DELETE /api/collections/:id/photos
│           ├── faces.rs     # POST /api/faces/analyze（支持 missing_only）、GET /api/faces/jobs/:id、GET /api/photos/:id/faces
│           ├── people.rs    # 人物相关所有端点；compute_refined_centroid() 质心算法
│           ├── geo.rs       # GET /api/geo/hierarchy、GET /api/geo/photos、POST /api/geo/regeocode、GET /api/geo/regeocode/status
│           └── animals.rs   # GET /api/animals/species、GET /api/animals/:species/photos、GET /api/photos/:id/animals
├── frontend/                # 静态 HTML + CSS + JS（编译时嵌入二进制）
├── migrations/              # SQLx 数据库迁移文件（0001–0014）
├── tests/                   # 集成测试与测试 fixture
├── docs/                    # 架构设计与开发计划
└── photobridge/             # Swift 包：macOS Photos 导出桥
    └── Sources/
        ├── PhotoBridge/     # 可执行目标（CLI 入口 + 命令）
        │   ├── PhotoBridgeCommand.swift  # @main，注册子命令
        │   └── Commands/
        │       ├── ExportCommand.swift       # export：全量导出
        │       ├── SyncCommand.swift         # sync：增量导出（PHPersistentChangeToken）
        │       ├── StatusCommand.swift       # status：显示同步状态
        │       ├── FixTimestampsCommand.swift # fix-timestamps：修复文件 mtime
        │       ├── FixOrientationsCommand.swift # fix-orientations：批量修复 HEIC EXIF 方向
        │       └── SetupCommand.swift        # setup：初始化配置
        └── PhotoBridgeLib/  # 库目标（业务逻辑，可独立测试）
            ├── LibraryEnumerator.swift       # 全量枚举 PHAsset
            ├── IncrementalEnumerator.swift   # 增量枚举（PHPersistentChangeToken）
            ├── AssetExporter.swift           # 写出文件，UTI → 扩展名映射
            ├── AssetTimestamp.swift          # applyTimestamp()：设置 mtime/ctime
            ├── DiskSpaceCheck.swift          # 导出前磁盘空间检查
            ├── PicManagerRunner.swift        # 子进程调用 picmanager import + NDJSON 日志解析
            └── SyncState.swift              # 持久化同步状态（JSON，含 PHPersistentChangeToken）
```

## 核心模块说明

### photobridge — iCloud 照片导出桥

Swift CLI，需要 macOS 13+，依赖 PhotoKit 框架（macOS TCC 权限由启动终端 App 持有）。

**功能：**
- `export`：全量枚举 Photos 库所有图片资产，批量下载（含 iCloud），写出到暂存目录，设置文件 mtime/ctime = `PHAsset.creationDate`，自动修正 EXIF 方向，快照当前 `PHPersistentChangeToken`
- `sync`：从上次保存的 `PHPersistentChangeToken` 读取变更，仅导出新增资产，更新 token；同样自动修正 EXIF 方向
- `status`：显示上次同步时间、导出计数、暂存目录占用
- `fix-timestamps`：对已导出但 mtime 不正确的文件重新设置时间戳
- `fix-orientations`：批量扫描 HEIC 文件，通过 PhotoKit 查询 Photos 的显示方向，对不一致的文件用 exiftool 修复 EXIF 方向；可选同时更新 PicManager DB 和清除缩略图缓存
- `setup`：引导用户完成初始配置

导出完成后可选自动调用 `picmanager import --copy --log <path>` 子进程，解析 NDJSON 日志汇报导入结果。mtime 被 picmanager 用作日期推断的第二优先级（当 EXIF 缺失时），因此正确设置 mtime 对库组织至关重要。

HEIC 方向修正原理：`writeAssetResourceOrientationFixed` 调用 `requestImageDataAndOrientation(for:options:)` 查询 Photos 的"显示方向"（含用户在 Photos.app 内手动旋转的修正），与文件 EXIF tag 比对，不一致时用 exiftool 无损修改标签。exiftool 未安装时静默跳过。

### importer — 导入模块

负责从指定目录扫描照片文件，将其整理到库目录并写入数据库。

**流程：**
1. 递归扫描目录，按文件扩展名 + magic bytes 过滤支持格式
2. 对每张照片计算 SHA-256，查询数据库判断是否已导入（相同 sha256 即跳过）
3. 新照片：四级日期推断（见下），**移动**文件到 `{library}/{yyyy-mm-dd}/`（传 `--copy` 则复制），提取元数据，写入数据库，状态标记为 `imported`
4. 数据库记录的 `path` 为文件在库内的最终路径

**拍摄日期四级推断链：**

| 优先级 | 来源 | 说明 |
|--------|------|------|
| 1 | EXIF 字段（多字段回退） | DateTimeOriginal → DateTimeDigitized → GPS DateStamp+TimeStamp → DateTime |
| 2 | 文件 mtime（`mtime_to_naive_datetime()`） | photobridge 导出时设置为 `PHAsset.creationDate`；适用于无 EXIF 的截图等 |
| 3 | 文件名模式 | Unix 时间戳（10/13位）/ YYYYMMDD_HHMMSS / YYYY-MM-DD |
| 4 | 无法推断 | 放入 `{library}/unknown/` |

**进度跟踪（`ImportProgress`）：**
- `total / processed / imported / skipped / errors`：基础导入计数
- `faces_found`：本次新检测到的人脸总数
- `gps_found`：有 GPS 的照片数
- `geo_total / geo_done`：地理编码进度

**大批量分批导入（`import_dir_batch`）：**
- `batch_size`：每次最多处理 N 张
- `log_path`：NDJSON 迁移日志（`importer/log.rs` MigrationLog），记录每文件状态，已完成文件下次跳过（断点续传）
- `dry_run`：只扫描不实际导入，返回文件数统计

**导入状态（`import_status`）：**

| 状态 | 含义 |
|------|------|
| `imported` | 已成功导入库 |
| `deleted` | 去重确认后被软删除 |

### metadata — 元数据提取

从各格式文件中提取结构化元数据。

- **时间**：EXIF 四字段回退链（DateTimeOriginal → DateTimeDigitized → GPS DateStamp+TimeStamp → DateTime）；EXIF 全部缺失时退回到文件 mtime，再退回文件名推断（`filename.rs`）
- **方向**：读取 EXIF Orientation tag（1–8），存入 `exif_orientation`；`image::open()` 不自动应用此字段，需手动调用 `face::apply_exif_orientation()`
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

**两层架构**，兼顾召回率（Layer 1 宽松筛候选）和精度（Layer 2 DCT 验证）：

**Layer 1 — Gradient pHash**（`image_hasher::HashAlg::Gradient`，64-bit）
- 连拍对（`taken_at` 差值 ≤ 60 s）：Hamming 距离 ≤ 10
- 远距离对（差值 > 60 s 或时间未知）：Hamming 距离 ≤ 3
- 退化 hash 过滤：set bits < 10 或 > 54 均跳过（双侧对称）

**Layer 2 — DCT pHash**（自实现，`compute_dcthash()` 在 `dedup/hash.rs`）
- 图像缩至 32×32 灰度 → Row-wise + Column-wise DCT-II → 取左上 8×8 低频系数 → 与均值比较生成 64-bit hash
- Hamming 距离 ≤ 8 才认为相似
- 图像读取失败时返回 `None`，跳过 Layer 2（不过滤），防止误判
- 按 photo ID 缓存（`HashMap<i64, Option<u64>>`），避免重复计算

**Union-Find 聚类**
- 两层都通过的候选对收集完毕后，用带路径压缩的 Union-Find 合并为连通分量
- 连拍对与远距离对**独立**使用 Union-Find：连拍照片所在的簇不会被远距离结构相似对污染
- 每个连通分量写入一个 DB 组（`dedup_groups` / `dedup_members`）

去重结果仅写入数据库标记为候选，**不自动删除**，需用户在 Web 界面确认后才执行软删除。

**扫描模式：**

| 模式 | 函数 | 适用场景 | 复杂度 |
|------|------|---------|--------|
| 增量扫描 | `scan()` | 每次导入后运行（默认） | O(new × old + new²) |
| 全量重扫 | `scan_full()` | `dedup --full`，首次或修复数据 | O(n × 548 × avg_bucket) |

`scan()` 仅比较 `dedup_scanned_at IS NULL` 的新照片；`scan_full()` 使用多索引哈希（4 × 16-bit 分段，邻桶查找），鸽巢原理保证距离 ≤ 10 的任意对至少在一个分段命中，无漏报；扫描前清除所有旧 `pending` 组，从零重建。

### album — 相册模块

**自动分组维度：**
- 时间（`kind='time'`）：按 `taken_at` 年月，形如 `2024-06`；`latest_photo_at` 字段记录最新照片时间，供前端排序
- 地点（`kind='location'`）：调用 OSM Nominatim 反地理编码将 GPS 坐标解析为城市名；结果缓存到 `geocache` 表，限速 1 req/s；精确 key 未命中时先在 ±0.01°（约 1 km）范围内查邻近缓存（proximity lookup），命中则写入精确 key 并返回；无 GPS 的照片跳过；导入时通过 `group_by_location_scoped` 仅处理本次新导入的照片
- 相机（`kind='camera'`）：按 EXIF Make+Model；无相机信息的照片跳过
- 精选集（`kind='curated'`）：用户手动创建，照片可自由加入/移除，复用 `albums` 表

一张照片可同时属于多个相册（时间相册 + 地点相册 + 相机相册 + 精选集），通过 `photo_albums` 多对多表关联。

手动合并：将 source 相册的所有照片并入 target，source 相册记录随之删除。

### face — 人脸模块

本地离线人脸检测与特征提取，模型文件存放于 `{config_dir}/picmanager/models/`。

**检测（`detector.rs`）**：
- 模型：ultraface-slim-320（约 1 MB），输入 `[1,3,240,320]` BGR float32
- 输出：`scores [1,4420,2]` + `boxes [1,4420,4]`（归一化坐标）
- 后处理：置信度 ≥ 0.5 过滤 + IoU NMS（阈值 0.45）
- `OnceLock<Option<Mutex<Session>>>` 懒加载；模型不存在时安静返回 `[]`，不 panic

**方向处理（`mod.rs`）**：
- `apply_exif_orientation(img, orientation)`：将 EXIF Orientation 1–8 转换为旋转/翻转，得到显示空间图像
- `apply_transform(img, rotation, flip_h, flip_v)`：叠加用户手动调整
- 两层变换后的图像才是"显示空间"；人脸 bbox、缩略图、裁图均在显示空间操作

**特征提取（`embedder.rs`）**：
- 模型：ArcFace MobileNetV1（约 10 MB），输入 112×112 RGB，输出 512D float32
- 预处理：按检测框裁剪（扩边 20%）→ resize 112×112 → 归一化 `[-1,1]`
- 后处理：L2 归一化（保证余弦相似度 = 点积）
- embedding 以小端序 `Vec<f32>` 存入 `faces.embedding` BLOB（2048 字节/512 维）
- 模型不存在时返回 `AppError::ModelNotFound`，导入流程中 embedding 留 NULL

**批量作业（`job.rs`）**：
- `run_job(pool, scope)` 在 `tokio::spawn` 中异步执行，立即返回 `job_id`
- `reanalyze_one_photo(pool, photo_id)`：先清除 cover_face_id 引用，再删除旧 faces 记录，再检测+嵌入；供照片旋转/翻转后后台触发
- 每张照片先 DELETE 旧 faces 行再重新检测+嵌入，保证不累积

**人物聚类（`cluster.rs`）**：

两阶段算法，避免 DBSCAN 链式合并（chaining）问题：

**Phase 1 — DBSCAN 核心（高置信度人脸）**
- 仅对 `confidence ≥ MIN_CONFIDENCE (0.70)` 的人脸做 DBSCAN
- `EPS = 0.35`，`min_samples = 2`（含点自身，即至少 1 个其他邻居）
- 距离度量：`1.0 - dot(a, b)`（余弦距离，L2 归一化嵌入）
- 噪点（未归入核心点）各自单独成一个人物记录

**Phase 2 — 低置信度人脸后归入**
- 对每张 `confidence < 0.70` 的人脸，计算到所有已有人物的最小余弦距离
- 距离 < EPS 则归入最近人物，否则各自单独建人物记录

**增量聚类（`run_incremental_clustering`）**：
- 非破坏性：只处理尚未分配人物的新人脸（`LEFT JOIN person_faces` 过滤）
- 导入完成后自动触发，将新人脸归入已有人物或创建新记录
- 同样使用两阶段算法，不重建已有人物记录

**精炼质心算法（`compute_refined_centroid` in `people.rs`）**：

用于 merge-suggestions、outlier-faces、centroid-faces 三个端点，两步计算：
1. 置信度预过滤：优先取 ≥ 0.85 的人脸（≥ 10 张则用），降级取 ≥ 0.70（≥ 10 张则用），否则全部
2. 几何精炼：候选数 > 50 时，先算粗质心，取距粗质心最近的 40% 再算精炼质心

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

**核心表（0001–0014 迁移）：**

```sql
photos          -- 照片主记录（path、sha256、phash、format、taken_at、gps_lat/lon、camera、
                --   import_status、dedup_scanned_at、timezone_offset、exif_orientation、
                --   rotation、flip_h、flip_v、width、height）
albums          -- 相册（name、kind：time / camera / location / curated、latest_photo_at）
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

迁移清单：

| 文件 | 内容 |
|------|------|
| 0001 | photos、albums、photo_albums、dedup_groups/members、import_sessions |
| 0002 | geocache 表 |
| 0003 | faces、face_jobs 表 |
| 0004 | photo_stats 计数器 |
| 0005 | photos.dedup_scanned_at（增量 dedup 标记） |
| 0006 | photos.timezone_offset |
| 0007 | people、person_faces 表 |
| 0008 | geocache 新增 country/state/county 列 |
| 0009 | animals 表 |
| 0010 | people.status 列 |
| 0011 | photos.rotation/flip_h/flip_v 列 |
| 0012 | photos.exif_orientation 列 |
| 0013 | 修复 people.parent_id 自引用孤儿（`UPDATE people SET parent_id = NULL WHERE id = parent_id`） |
| 0014 | photos.width/height 列 |

数据库文件和照片库存放在用户指定的数据目录（默认 `~/Pictures/PicManager/`）。

### web — Web 服务

基于 **Axum** 提供 REST API，前端为静态文件（HTML+JS+CSS，编译进二进制）。

**主要 API：**

```
POST   /api/import                              # 触发后台导入任务（tokio::spawn）
GET    /api/import/status                       # 轮询导入进度
GET    /api/photos                              # 照片列表（分页）
GET    /api/photos/gps-points                   # 所有有 GPS 的照片坐标列表
POST   /api/photos/batch-update                 # 批量修改时间/时区/旋转/翻转
GET    /api/photos/:id                          # 单张照片详情
PATCH  /api/photos/:id                          # 修改照片时间/时区/旋转/翻转
GET    /api/photos/:id/thumb                    # 300px JPEG 缩略图（含方向变换）
GET    /api/photos/:id/file                     # 原始文件字节（Content-Type 依 format 推断）
GET    /api/photos/:id/faces                    # 该照片的所有人脸区域（含 person_id/person_name）
GET    /api/photos/:id/animals                  # 该照片的所有动物检测结果
GET    /api/dedup                               # 待确认重复组列表（含 filename/width/height/taken_at/camera）
POST   /api/dedup/:group_id/resolve             # 确认保留（软删除其余项）
GET    /api/albums                              # 相册列表（含 photo_count、latest_photo_at）
GET    /api/albums/:id/photos                   # 相册内照片列表（分页）
POST   /api/albums/merge                        # 合并相册
GET    /api/collections                         # 精选集列表
POST   /api/collections                         # 创建精选集
PATCH  /api/collections/:id                     # 重命名精选集
DELETE /api/collections/:id                     # 删除精选集
GET    /api/collections/:id/photos              # 精选集内照片列表
POST   /api/collections/:id/photos              # 批量加入精选集
DELETE /api/collections/:id/photos              # 批量移出精选集
POST   /api/faces/analyze                       # 触发人脸重分析任务（全库、指定 photo_ids 或 missing_only）
GET    /api/faces/jobs/:id                      # 轮询重分析任务进度
GET    /api/faces/:id/thumb                     # 人脸裁剪缩略图（磁盘缓存，含方向变换）
GET    /api/geo/hierarchy                       # 地理层级（country→state→city 含照片数）
GET    /api/geo/photos                          # 地理过滤照片列表
POST   /api/geo/regeocode                       # 为有 GPS 但缺 geocache 的照片触发后台反地理编码
GET    /api/geo/regeocode/status                # 查询反地理编码后台任务是否在运行
GET    /api/people                              # 人物列表（支持 ?status= 和 ?name_exact= 过滤）
POST   /api/people                              # 创建人物
GET    /api/people/tree                         # 人物树（嵌套 JSON，含 cover_face_id）
POST   /api/people/cluster                      # 触发全量 DBSCAN 重聚类
POST   /api/people/cluster/incremental          # 触发增量聚类（仅处理未分配人脸）
POST   /api/people/merge                        # 合并两个人物
POST   /api/people/batch-update                 # 批量修改人物状态
GET    /api/people/:id                          # 人物照片列表（WITH RECURSIVE CTE 含子树）
PATCH  /api/people/:id                          # 修改人物姓名或状态
DELETE /api/people/:id                          # 删除人物
POST   /api/people/:id/reparent                 # 变更人物父节点
POST   /api/people/:id/transfer                 # 将指定照片的人脸移转到该人物
POST   /api/people/:id/lift                     # 将该人物从子节点提升为独立顶级节点
GET    /api/people/:id/merge-suggestions        # 返回质心余弦距离最近的其他人物（最多 10 个）
GET    /api/people/:id/outlier-faces            # 返回距质心最远的人脸（离群检测）
POST   /api/people/:id/eject-face               # 将指定人脸移出该人物（单独成一个新人物）
GET    /api/people/:id/centroid-faces           # 返回质心计算用的人脸及距离分布统计
GET    /api/animals/species                     # 动物种类列表（含照片数）
GET    /api/animals/:species/photos             # 指定种类的照片列表
```

长耗时任务（导入、人脸分析、反地理编码）通过 **tokio::spawn** 后台执行，进度通过客户端轮询对应 status API 获取。导入和地理编码各用 `Arc<Mutex<>>` / `Arc<AtomicBool>` 防止并发重入。

## 数据流

### 导入流程

```
用户指定目录（或 photobridge 暂存目录）
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
metadata：提取 EXIF（时间四字段回退链、GPS、相机、exif_orientation）
    │
    ▼
日期推断（四级）：EXIF taken_at → 文件 mtime → 文件名模式 → None
    │
    ▼
placer：将文件移动（或 --copy 时复制）到 {library}/{yyyy-mm-dd}/ 或 unknown/
    │
    ▼
dedup/hash：计算 Gradient pHash，写入 photos.phash
    │
    ▼
storage：写入 photos 表（path = 库内新路径），import_status = 'imported'
         存储 width/height、exif_orientation
  photo_stats.active_count += 1
    │
    ▼
face::analyze_one：读取 photos.exif_orientation + rotation/flip_h/flip_v
  → apply_exif_orientation + apply_transform 到显示空间
  → detect() → 写入 faces 表（bbox 坐标为显示空间）→ 若模型可用则提取 embedding
    │
    ▼
animal::detect_and_save：YOLOv8-nano 检测 → 写入 animals 表（模型不存在时跳过）
    │
    ▼
album：按月份自动分组 → 按相机自动分组 → 按 GPS 地点自动分组（仅本次新导入）
    │
    ▼
face::cluster::run_incremental_clustering：将新人脸归入已有人物或创建新人物记录
```

### 去重确认流程

```
picmanager dedup（CLI）或 GET /api/dedup（Web）
    │
    ▼
dedup::scan()：
  仅比较 dedup_scanned_at IS NULL 的新照片（增量）
  → Layer 1：Gradient pHash 汉明距离（连拍 ≤ 10，远距离 ≤ 3）
  → Layer 2：DCT pHash 汉明距离 ≤ 8（Layer 1 通过才计算）
  → Union-Find 聚类（burst / far 独立）→ 写入 dedup_groups / dedup_members
  → 打上 dedup_scanned_at 时间戳
  无新照片时立即返回 0（O(1) 无操作）

  picmanager dedup --full：
    scan_full() 清除所有 pending 组，重置所有时间戳
    使用多索引分桶（4 × 16-bit 段，邻桶查找）全量重扫
    │
    ▼
用户在 Web 界面查看重复候选（展示文件名、尺寸、拍摄日期、相机），选择保留哪张
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
| HTTP 框架 | Axum 0.8 | 异步、与 tokio 生态契合 |
| 数据库 | SQLite + sqlx 0.8 | 单机、零部署、编译期查询检查 |
| 异步运行时 | tokio | Rust 生态标准选择 |
| CLI | clap 4 | 成熟、声明式 |
| EXIF | kamadak-exif 0.5 | 纯 Rust，无 C 依赖 |
| 图像处理 | image 0.25 | 支持多格式，纯 Rust（不自动应用 EXIF Orientation） |
| HEIC | kamadak-exif + 系统 libheif | 依赖 brew install libheif |
| 感知哈希 | image_hasher 3 | Gradient pHash（Layer 1），纯 Rust |
| DCT pHash | 自实现（`dedup/hash.rs`） | Layer 2 精确验证，无外部依赖 |
| 反地理编码 | reqwest + OSM Nominatim | GPS → 城市名，结果缓存到 geocache 表 |
| ONNX 推理 | ort 2.0.0-rc.12 | 人脸检测 + 特征提取 + 动物检测；download-binaries + coreml 特性，macOS 自动使用 ANE |
| 数值计算 | ndarray 0.17 | ort RC.12 的 ndarray 依赖版本，需严格匹配 |
| 日志 | tracing | 结构化日志，适配 tokio |
| 静态文件 | rust-embed 8 | 前端编译进二进制，零运行时依赖 |
| Photos 导出 | Swift + PhotoKit | macOS 原生，访问 iCloud 照片库 |
| Swift CLI | Swift ArgumentParser | photobridge 子命令框架 |

## 部署模型

### picmanager（主工具）

单一 Rust 二进制，包含 Web 服务器和 CLI 两种模式：

```bash
picmanager serve                              # 启动 Web 服务（默认 127.0.0.1:8080）
picmanager import <dir>                       # 命令行导入（默认 move 文件）
picmanager import --copy <dir>                # 命令行导入（复制，保留源文件）
picmanager import --batch-size 200 <dir>      # 分批导入（大批量迁移场景）
picmanager import --dry-run <dir>             # 仅统计，不实际导入
picmanager import --log import.log <dir>      # NDJSON 日志（断点续传）
picmanager dedup                              # 命令行增量去重扫描（仅报告组数，Web 界面确认）
picmanager dedup --full                       # 全量重扫（多索引分桶）
picmanager faces analyze                      # 全库人脸重分析
picmanager faces analyze --photo-ids 1,2,3   # 指定照片人脸重分析
picmanager faces analyze --rotated-only       # 仅重分析已旋转过的照片
picmanager models fetch                       # 下载 ONNX 模型文件到 config_dir/models/
picmanager models bundle                      # 复制模型到 ./models/，cargo build 可内置于二进制
picmanager config                             # 显示当前生效配置
```

照片库数据目录由配置文件指定，默认 `~/Pictures/PicManager/`。前端静态文件编译进二进制，无需额外部署。

### photobridge（iCloud 导出桥，macOS 专用）

独立 Swift CLI，需要 macOS 13+。通过 TCC 请求照片库权限（权限归属启动终端 App）。

```bash
photobridge export --output ~/staging/          # 全量导出到暂存目录（含方向修正）
photobridge sync --output ~/staging/            # 增量导出（自上次同步后的新照片）
photobridge export --dry-run                    # 统计资产数，不实际导出
photobridge status                              # 显示同步状态和统计
photobridge fix-timestamps --output ~/staging/  # 修复已导出文件的 mtime
photobridge fix-orientations --dir ~/staging/   # 批量修复 staging 中 HEIC EXIF 方向
photobridge setup                               # 初始化配置引导
```

典型工作流：
1. `photobridge export --output ~/staging/` 将 Photos 库全量导出到暂存目录，设置 mtime = `PHAsset.creationDate`
2. `picmanager import --copy ~/staging/` 将暂存目录导入 picmanager 库
3. 后续增量：`photobridge sync --output ~/staging/` → `picmanager import --copy ~/staging/`
