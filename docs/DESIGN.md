# PicManager 详细设计文档

本文档描述已实现代码的实际设计，供开发和调试参考。与代码有出入时以代码为准。

---

## 目录

1. [目录结构](#目录结构)
2. [数据库 Schema](#数据库-schema)
3. [配置系统](#配置系统)
4. [错误处理](#错误处理)
5. [模块：importer](#模块-importer)
6. [模块：metadata](#模块-metadata)
7. [模块：dedup](#模块-dedup)
8. [模块：album](#模块-album)
9. [模块：face](#模块-face)
10. [模块：storage](#模块-storage)
11. [模块：web](#模块-web)
12. [REST API 参考](#rest-api-参考)
13. [前端](#前端)
14. [测试策略](#测试策略)
15. [关键约束与边界情况](#关键约束与边界情况)

---

## 目录结构

```
src/
  main.rs                  CLI 入口，clap 解析，调用库函数
  lib.rs                   pub mod 声明，re-export 各模块
  config.rs                Config 结构体，TOML 配置文件加载
  error.rs                 AppError 枚举，Result<T> 类型别名
  importer/
    mod.rs                 import_dir() 主入口，串联流水线
    placer.rs              文件移动/复制到库目录，日期分目录，冲突重命名
    scanner.rs             递归目录扫描，magic bytes 格式过滤
    state.rs               SHA-256 计算，ImportDecision 枚举（按 sha256 去重）
  metadata/
    mod.rs                 re-export extract_from_file, infer_date
    exif.rs                EXIF 解析（时间四字段回退链/GPS/相机）
    filename.rs            文件名日期推断（Unix 时间戳/紧凑/分隔符模式）
    format.rs              magic bytes 格式检测
    types.rs               ImageFormat 枚举，PhotoMeta 结构体
  dedup/
    mod.rs                 scan(), scan_full(), list_groups(), resolve()
    hash.rs                compute_phash()（Layer 1 Gradient pHash），compute_dcthash()（Layer 2 DCT pHash），hamming_distance()，is_degenerate()
    candidate.rs           scan()增量扫描，scan_full()多索引分桶，写入 dedup_groups
  album/
    mod.rs                 re-export 三个分组函数和 merge
    organize.rs            group_by_month(), group_by_camera()
    location.rs            group_by_location()，Nominatim 反地理编码
    merge.rs               merge(source_id, target_id)
  face/
    mod.rs                 analyze_one(pool, photo_id, img)，re-export detect/FaceRegion/Embedder
    detector.rs            detect()，FaceRegion，preprocess()，iou()，nms()
    embedder.rs            Embedder::load/extract，l2_normalize，encode/decode_embedding
    job.rs                 run_job(pool, scope)，execute_job() pub(crate)
    cluster.rs             cluster_faces()，run_clustering(pool)（两阶段，confidence≥0.70 核心聚类），run_incremental_clustering(pool)（非破坏性增量聚类）
  animal/
    mod.rs                 detect_and_save(pool, photo_id, img)，导入时调用，模型不存在时静默跳过
    detector.rs            detect()，AnimalDetection，YOLOv8-nano，OnceLock<Mutex<Session>>
  activities/
    mod.rs                 pub re-export；import_dir_activities()、import_one()、ImportSummary
    parser.rs              parse_fit()、parse_gpx()；ActivityData 结构体、TrackPoint 结构体
    importer.rs            scan_dir()、import_one()；SHA-256 去重；批量插入轨迹点（chunk 500）
    rdp.rs                 simplify()；Ramer-Douglas-Peucker 轨迹压缩算法
  storage/
    mod.rs                 re-export connect
    db.rs                  connect()，SQLitePool，运行迁移
  web/
    mod.rs                 AppState, router(), serve()
    handlers/
      import.rs            ImportStatus, start_import, get_import_status
      photos.rs            list_photos, get_thumb, get_photo_file, get_photo, patch_photo, batch_update_photos, get_gps_points
      dedup.rs             list_dedup_groups, resolve_group
      albums.rs            list_albums, list_album_photos, merge_albums
      collections.rs       list_collections, create_collection, rename_collection, delete_collection, add_photos, remove_photos, list_collection_photos
      faces.rs             start_analyze (支持 missing_only), get_job_status, list_photo_faces
      people.rs            list_people, get_person_photos, get_people_tree, cluster_people, merge_people, patch_person, batch_update_people, reparent_person, get_face_thumb
      geo.rs               get_geo_hierarchy, start_regeocode, get_regeocode_status
      animals.rs           list_species, list_species_photos, list_photo_animals
      activities.rs        list_activities, get_activity, get_activity_track, get_activity_photos, trim_activity, merge_activities
frontend/
  index.html               主页面（侧边栏 + 照片网格 + 弹窗骨架）
  style.css                布局与主题（Catppuccin 深色侧边栏）
  app.js                   原生 JS，调用 REST API
migrations/
  0001_initial.sql         基础表（photos, albums, photo_albums, dedup_*, import_sessions）
  0002_geocache.sql        geocache 表（GPS → 城市名缓存）
  0003_faces.sql           faces 表、face_jobs 表
  0004_photo_stats.sql     photo_stats 计数器表（active_count）
  0005_dedup_incremental.sql photos.dedup_scanned_at 列
  0006_timezone_offset.sql photos.timezone_offset 列（UTC 偏移分钟数）
  0007_people.sql          people 表、person_faces 表
  0008_geocache_hierarchy.sql geocache 新增 country/state/county 列
  0009_animals.sql         animals 表
  0010_people_status.sql   people.status 列（active / ignored / not_a_person）
  0011_rotation.sql        photos.rotation / flip_h / flip_v 列（用户手动旋转/翻转，DB 存储不改文件）
  0012_exif_orientation.sql photos.exif_orientation 列（EXIF Orientation tag 1-8，导入时写入）
  0013_people_fix.sql      修复已有 parent_id 自引用孤儿节点（`UPDATE people SET parent_id = NULL WHERE id = parent_id`）
  0014_photo_dimensions.sql photos.width / height 列（像素尺寸，导入时写入）
  0015_collections.sql     albums.kind 列（'auto' / 'curated'）
  0016_people_extras.sql   people 额外字段
  0017_activities.sql      activities 表（运动记录元数据）+ activity_track_points 表（GPS 轨迹点）
  0018_activity_sensors.sql  activities.sensors TEXT 列（ANT+ 传感器 JSON 数组）
docs/
  REQUIREMENTS.md
  PLAN.md
  ARCHITECTURE.md
  DESIGN.md（本文件）
photobridge/              iCloud Photos 导出伴侣工具（Swift Package）
  Sources/PhotoBridgeLib/
    LibraryEnumerator.swift   全量枚举 PHAsset + selectExportResource
    IncrementalEnumerator.swift  增量枚举（PHPersistentChangeFetchResult）
    AssetExporter.swift       exportDestinationURL() + writeAssetResource()
    AssetTimestamp.swift      applyTimestamp(to:date:)
    DiskSpaceCheck.swift      磁盘空间预检
    PicManagerRunner.swift    parseImportLog() + importBatch() 子进程
    SyncState.swift           同步状态持久化
  Sources/PhotoBridge/
    Commands/                 export / sync / status / fix-timestamps / fix-orientations / setup
tests/
  web_api.rs               Web API 集成测试（tower oneshot）
  fixtures/
    with_exif.jpg          带完整 EXIF（DateTimeOriginal 2024-06-15 10:30:00，GPS 旧金山，iPhone 15 Pro）
    with_exif_small.jpg    with_exif.jpg 50% 缩小版（验证 pHash 相似性）
    no_exif.jpg            无 EXIF（验证 taken_at/camera/gps 均为 None）
    different.jpg          视觉上不同的图片（验证 pHash 区分度）
    digitized_only.jpg     仅含 DateTimeDigitized（验证 EXIF 回退链）
    gps_time_only.jpg      仅含 GPS DateStamp+TimeStamp（验证 GPS 时间回退）
    datetime_only.jpg      仅含 DateTime IFD0 字段（验证最低优先级回退）
make_fixtures.py           Python 脚本，写入原始 EXIF 二进制生成上述 fixture
```

---

## 数据库 Schema

### photos

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| path | TEXT UNIQUE | 照片在库内的绝对路径（`{library}/{yyyy-mm-dd}/filename.jpg`） |
| sha256 | TEXT | 文件内容 SHA-256 哈希（用于精确去重，导入时检查） |
| phash | TEXT NULL | 感知哈希 Base64（pHash/dHash，导入后计算） |
| taken_at | TEXT NULL | 拍摄时间（EXIF 四字段回退链），格式 `YYYY-MM-DD HH:MM:SS` |
| gps_lat | REAL NULL | GPS 纬度（十进制度，南为负） |
| gps_lon | REAL NULL | GPS 经度（十进制度，西为负） |
| camera | TEXT NULL | `{Make} {Model}`，若 model 已含 make 则只取 model |
| format | TEXT | `jpeg` / `png` / `gif` / `webp` / `heic` / `arw` / `unknown` |
| import_status | TEXT | `imported` / `deleted`（软删除，由去重确认触发） |
| imported_at | TEXT | 导入时间（UTC，`datetime('now')`） |
| dedup_scanned_at | TEXT NULL | 最近一次 dedup 扫描时间；NULL = 尚未扫描（增量扫描依据） |
| timezone_offset | INTEGER NULL | UTC 偏移（分钟），如 480（东八区）、-300（EDT）；NULL = 未知 |
| rotation | INTEGER | 用户手动旋转角度（0 / 90 / 180 / 270），默认 0 |
| flip_h | INTEGER | 水平翻转（0 = 否，1 = 是），默认 0 |
| flip_v | INTEGER | 垂直翻转（0 = 否，1 = 是），默认 0 |
| exif_orientation | INTEGER NULL | EXIF Orientation tag 值（1–8），导入时写入；NULL = 无 EXIF 或 tag 缺失 |
| width | INTEGER NULL | 照片原始宽度（像素），导入时写入；NULL = 无法解析 |
| height | INTEGER NULL | 照片原始高度（像素），导入时写入；NULL = 无法解析 |

索引：`sha256`、`import_status`、`taken_at`

### faces

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| photo_id | INTEGER FK | 关联 photos.id，ON DELETE CASCADE |
| x | INTEGER | 检测框左上角 x（原图像素） |
| y | INTEGER | 检测框左上角 y |
| width | INTEGER | 检测框宽度 |
| height | INTEGER | 检测框高度 |
| confidence | REAL NULL | ultraface 置信度 0.0–1.0 |
| embedding | BLOB NULL | 512 × f32 小端序（2048 字节）；NULL 表示尚未提取 |
| embed_model | TEXT NULL | 提取所用模型标识，如 `arcface-mobilenet-v1` |
| detected_at | TEXT | 检测时间（UTC `datetime('now')`） |

索引：`idx_faces_photo_id`（photo_id）

### face_jobs

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| status | TEXT | `running` / `done` / `failed` |
| scope | TEXT NULL | NULL = 全库；JSON 整数数组 = 指定 photo_id 列表 |
| total | INTEGER NULL | 待处理照片总数 |
| processed | INTEGER | 已处理数（每处理一张 +1） |
| started_at | TEXT | 任务创建时间 |
| finished_at | TEXT NULL | 完成时间 |

### albums

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| name | TEXT | 相册名称（月份相册形如 `2024-06`） |
| kind | TEXT | `time` / `camera` / `location` / `manual` |
| created_at | TEXT | 创建时间 |

### photo_albums

多对多关联表，`(photo_id, album_id)` 为复合主键，两列均有 `ON DELETE CASCADE`。

### dedup_groups / dedup_members

`dedup_groups.status`：`pending`（待确认）/ `resolved`（已处理）

`dedup_members.keep`：`0`（未决定/删除）/ `1`（保留）

### photo_stats

单行计数器表（`id = 1` 约束），避免对 `photos` 表执行 `COUNT(*)` 全表扫描。

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 始终为 1（`CHECK (id = 1)`） |
| active_count | INTEGER | `import_status = 'imported'` 的照片数量 |

维护规则：
- 导入成功：`active_count += 1`（仅 `rows_affected > 0` 时）
- dedup resolve 软删除：`active_count -= 被删除数量`
- 迁移时从现有数据 seed：`INSERT ... SELECT 1, COUNT(*) FROM photos WHERE import_status='imported'`

### geocache

| 列 | 类型 | 说明 |
|----|------|------|
| lat_key | TEXT | 纬度保留 4 位小数的字符串（精度 ≈11 m） |
| lon_key | TEXT | 经度同上 |
| city | TEXT NULL | 城市名；NULL 表示反地理编码失败（已缓存，不再重试） |
| country | TEXT NULL | 国家名（Nominatim `address.country`） |
| state | TEXT NULL | 州/省（Nominatim `address.state`） |
| county | TEXT NULL | 县/区（Nominatim `address.county`） |
| cached_at | TEXT | 缓存时间 |

复合主键：`(lat_key, lon_key)`，`INSERT OR IGNORE` 保证幂等。

### people

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| name | TEXT NULL | 用户自定义名称；NULL = 未命名 |
| parent_id | INTEGER NULL FK | 关联 people.id；NULL = 顶级人物；非 NULL = 子节点 |
| status | TEXT | `active`（默认）/ `ignored`（忽略）/ `not_a_person`（非人物）；`CHECK(status IN (...))` 约束 |
| cover_face_id | INTEGER NULL FK | 代表性人脸（关联 faces.id）；NULL 时前端取第一张 |
| created_at | TEXT | 创建时间（UTC `datetime('now')`） |

支持任意深度树状结构，循环引用由应用层防止。`status` 列由迁移 0010 通过 `ALTER TABLE ADD COLUMN` 添加，默认值 `'active'`。

### person_faces

| 列 | 类型 | 说明 |
|----|------|------|
| person_id | INTEGER FK | 关联 people.id，ON DELETE CASCADE |
| face_id | INTEGER FK | 关联 faces.id，ON DELETE CASCADE |

复合主键：`(person_id, face_id)`，一张人脸同时只属于一个人物。

### animals

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| photo_id | INTEGER FK | 关联 photos.id |
| species | TEXT | COCO 类名，如 `cat`、`dog`、`bird` |
| confidence | REAL | YOLOv8 置信度 0.0–1.0 |
| x | INTEGER | 检测框左上角 x（原图像素） |
| y | INTEGER | 检测框左上角 y |
| width | INTEGER | 检测框宽度 |
| height | INTEGER | 检测框高度 |
| detected_at | TEXT | 检测时间（UTC `datetime('now')`） |

### 精选集（Collections）

精选集复用 `albums` 表，以 `kind='curated'` 区分。通过 `photo_albums` 表（相同的多对多关联）将照片关联到精选集，无需新表。

`GET /api/collections` 返回所有 `kind='curated'` 的相册，按 `created_at` 降序。创建、改名、删除与普通相册底层操作相同，仅通过 `/api/collections` 系列路径暴露。

---

## 配置系统

### 默认值

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `library_path` | `~/Pictures/PicManager` | 照片库根目录（`dirs::picture_dir()` 解析） |
| `db_path` | `{library_path}/picmanager.db` | 随 `library_path` 联动 |
| `thumb_cache_dir` | `{library_path}/.thumbs` | 缩略图磁盘缓存目录，随 `library_path` 联动 |
| `host` | `127.0.0.1` | Web 服务绑定地址 |
| `port` | `8080` | Web 服务端口 |
| `thumb_size` | `300` | 缩略图最大边长（像素） |

### 配置文件

路径：`~/Library/Application Support/picmanager/config.toml`（macOS，`dirs::config_dir()` 解析）

```toml
library_path = "/Volumes/NAS/Photos/PicManager"
host         = "0.0.0.0"
port         = 9090
thumb_size   = 400
```

所有字段均可选。优先级：**命令行参数 > 配置文件 > 内置默认值**。

配置文件解析失败时静默忽略，使用默认值。

### 辅助方法

```rust
Config::db_url()    -> String   // "sqlite:{db_path}"
Config::bind_addr() -> String   // "{host}:{port}"
```

---

## 错误处理

```rust
pub enum AppError {
    NotFound(String),
    UnsupportedFormat(String),
    Metadata(String),
    Database(#[from] sqlx::Error),
    Migration(#[from] sqlx::migrate::MigrateError),
    Io(#[from] std::io::Error),
    ModelNotFound(String),   // ONNX 模型文件不存在或加载失败
}
pub type Result<T> = std::result::Result<T, AppError>;
```

- `#[from]` 使 `?` 运算符自动转换 `sqlx::Error` 和 `std::io::Error`
- Web 层将 `AppError` 映射到 HTTP 状态码（`NotFound` → 404，其余 → 500）
- 导入流水线对单文件错误只记录 `tracing::warn`，不中断整批次导入

---

## 模块：importer

### 公开接口

```rust
pub async fn import_dir(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
) -> Result<ImportSummary>

// CLI 使用的带进度追踪版本
pub async fn import_dir_with_progress(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
    progress: SharedImportProgress,
) -> Result<ImportSummary>

pub struct ImportSummary {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}

// 原子计数器，供 CLI 进度循环轮询
pub struct ImportProgress {
    pub total: AtomicUsize,      // scan_dir 完成后立即写入
    pub processed: AtomicUsize,  // 每处理一个文件后递增（= imported + skipped + errors）
    pub imported: AtomicUsize,
    pub skipped: AtomicUsize,
    pub errors: AtomicUsize,
}
pub type SharedImportProgress = Arc<ImportProgress>;
```

### 流水线（`import_dir` → `import_one`）

```
scan_dir(source_dir)
  └─ walkdir 递归，按扩展名预过滤，再用 magic bytes 确认格式
     （扩展名白名单：jpg/jpeg/png/gif/webp/heic/heif/arw）

for each path:
  compute_sha256(path)
    └─ 分块读取，sha2::Sha256

  decide(pool, sha256)
    ├─ sha256 已在 DB → AlreadyImported → skipped += 1，跳过
    └─ 未见过 → New

  if New:
    metadata::extract_from_file(path)   // EXIF 四字段回退链 + GPS + 相机

    // 四级日期推断
    // effective_taken_at = meta.taken_at.or_else(|| mtime_to_naive_datetime(path))
    date = meta.taken_at.map(|dt| dt.date())
        ?? mtime_to_naive_datetime(path).map(|dt| dt.date())   // 文件 mtime（photobridge 已设为 PHAsset.creationDate）
        ?? metadata::infer_date(filename).map(|dt| dt.date())
        ?? None   // → unknown/

    placer::place(path, library_path, date, copy_only)
        └─ 将文件移动（copy_only=false）或复制（copy_only=true）到
           {library_path}/{yyyy-mm-dd}/filename.jpg
           或 {library_path}/unknown/filename.jpg
           目标文件名冲突时追加 _1/_2... 后缀

    dedup::hash::compute_phash(final_path)   // pHash（失败则 phash=NULL）

    INSERT OR IGNORE INTO photos (path=final_path, ...) import_status='imported'
    if rows_affected > 0:
        UPDATE photo_stats SET active_count = active_count + 1 WHERE id = 1
    photo_id = result.last_insert_rowid()
    imported += 1

    // best-effort face analysis（失败不中断导入）
    if let Ok(img) = image::open(&final_path) {
        face::analyze_one(pool, photo_id, &img).await;
        // best-effort animal detection（模型不存在时静默跳过）
        animal::detect_and_save(pool, photo_id, &img).await;
    }

after all files:
  album::group_by_month(pool)
  album::group_by_camera(pool)
  album::group_by_location(pool)
```

### scanner.rs

```rust
pub fn scan_dir(dir: &Path) -> Vec<PathBuf>
```

- 使用 `walkdir::WalkDir`，`follow_links(false)`
- 扩展名白名单：`jpg jpeg png gif webp heic heif arw`（大小写不敏感）
- 读取文件头 12 字节，调用 `format::detect()` 二次确认（排除扩展名欺骗）
- 返回所有通过两道过滤的文件路径，顺序不保证

### placer.rs

```rust
pub fn place(
    src: &Path,
    library_path: &Path,
    date: Option<NaiveDate>,
    copy_only: bool,
) -> Result<PathBuf>
```

- `date = Some(d)` → `{library_path}/{d:%Y-%m-%d}/`，`date = None` → `{library_path}/unknown/`
- `copy_only = false`：`std::fs::rename`；跨设备（EXDEV）时降级为 copy + delete
- `copy_only = true`：`std::fs::copy`，源文件保留
- 目标文件名冲突时在文件名末尾追加 `_1`、`_2`…

### state.rs

```rust
pub enum ImportDecision {
    New,
    AlreadyImported,
}

pub fn compute_sha256(path: &Path) -> Result<String>
pub async fn decide(pool: &SqlitePool, sha256: &str) -> Result<ImportDecision>
// 仅按 sha256 判断：EXISTS(SELECT 1 FROM photos WHERE sha256 = ?)
```

---

## 模块：metadata

### 公开接口

```rust
pub fn extract_from_file(path: &Path) -> Result<PhotoMeta>
pub fn infer_date(filename: &str) -> Option<NaiveDateTime>   // filename.rs

pub struct PhotoMeta {
    pub format: ImageFormat,
    pub taken_at: Option<NaiveDateTime>,
    pub camera: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

pub enum ImageFormat {
    Jpeg, Png, Gif, Webp, Heic, Arw, Unknown,
}
```

### format.rs — magic bytes 检测

| 格式 | magic bytes |
|------|-------------|
| JPEG | `FF D8 FF` |
| PNG  | `89 50 4E 47 0D 0A 1A 0A` |
| GIF  | `47 49 46 38` (`GIF8`) |
| WebP | `52 49 46 46 .. .. .. .. 57 45 42 50` |
| HEIC | bytes 4-7 == `ftyp`，bytes 8-11 含 `heic`/`heix`/`mif1`/`msf1` |
| ARW  | TIFF magic（`49 49 2A 00` 或 `4D 4D 00 2A`）**且**扩展名为 `.arw` |

### exif.rs — EXIF 解析

使用 `kamadak-exif`，`Reader::new().read_from_container()` 可处理 JPEG/HEIC/ARW 容器。

**时间（四字段回退链）**：

| 优先级 | Tag | Tag 编号 | 说明 |
|--------|-----|---------|------|
| 1 | DateTimeOriginal | 0x9003 | 快门时写入，最可靠 |
| 2 | DateTimeDigitized | 0x9004 | 数字化时间，通常与 Original 相同 |
| 3 | GPS DateStamp + TimeStamp | GPS IFD | UTC 时间（存在时区偏差） |
| 4 | DateTime | 0x0132 | 文件最后修改时间，可能被编辑软件改写 |

kamadak-exif 的 `display_value()` 将存储格式 `YYYY:MM:DD HH:MM:SS` 转换为 `YYYY-MM-DD HH:MM:SS`（破折号分隔），再用 `chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")` 解析。

**相机**：`Tag::Make` + `Tag::Model`，去双引号后拼接；若 model 已含 make 前缀则只用 model

**GPS**：`Tag::GPSLatitude` / `Tag::GPSLongitude`（各三个 Rational：度/分/秒）+ 方向参考（`S`/`W` 为负）

```
decimal = deg + min/60 + sec/3600
若 ref == 'S' 或 'W' 则取负值
```

### filename.rs — 文件名日期推断

```rust
pub fn infer_date(filename: &str) -> Option<NaiveDateTime>
```

从文件名（含扩展名）推断拍摄日期，按以下顺序尝试：

| 顺序 | 模式 | 示例 | 说明 |
|------|------|------|------|
| 1 | 纯数字 10 位 | `1718447400.jpg` | Unix 秒级时间戳，返回 UTC |
| 2 | 纯数字 13 位 | `1718447400000.jpg` | Unix 毫秒时间戳，返回 UTC |
| 3 | 紧凑日期时间 | `IMG_20240615_103000.jpg` | `YYYYMMDD[_-]HHMMSS`（前后可有其他字符） |
| 4 | 分隔符日期 | `2024-06-15_vacation.jpg` | `YYYY[-_]MM[-_]DD`（时间默认 00:00:00） |

无效日期（如月份 13、日期 32）经 `NaiveDate::from_ymd_opt` 拒绝，返回 `None`。

---

## 模块：dedup

### 公开接口

```rust
pub async fn scan(pool: &SqlitePool) -> Result<usize>       // 增量扫描
pub async fn scan_full(pool: &SqlitePool) -> Result<usize>  // 全量重扫（多索引分桶）
pub async fn list_groups(pool: &SqlitePool) -> Result<Vec<DedupGroup>>
pub async fn resolve(pool: &SqlitePool, group_id: i64, keep_ids: &[i64]) -> Result<()>

pub struct DedupGroup {
    pub group_id: i64,
    pub members: Vec<DedupMember>,
}
pub struct DedupMember {
    pub photo_id: i64,
    pub path: String,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
}
```

### hash.rs

```rust
pub fn compute_phash(path: &Path) -> Result<String>
// 使用 image_hasher，HashAlg::Gradient，返回 Base64 字符串（Layer 1）

pub fn hamming_distance(a: &str, b: &str) -> Option<u32>
// 解析两个 Base64 字符串为 ImageHash<Box<[u8]>>，调用 .dist()

pub fn compute_dcthash(path: &Path) -> Option<u64>
// DCT pHash（Layer 2）：缩至 32×32 灰度 → 2D DCT-II → 取左上 8×8 低频系数
// → 各值与均值比较 → 64-bit hash；图像无法读取时返回 None（跳过第二层）

pub fn dcthash_distance(a: u64, b: u64) -> u32
// (a ^ b).count_ones()

pub fn is_degenerate(phash: &str) -> bool
// set bits < MIN_HASH_BITS(10) 或 > HASH_TOTAL_BITS - MIN_HASH_BITS(54) 时为退化 hash
// 对称双侧检测：过稀疏（全黑/纯色）和过密集（系统性左亮右暗梯度）均排除

pub const SIMILARITY_THRESHOLD: u32 = 10;     // 连拍（≤ 60 s）Layer 1 阈值
pub const SIMILARITY_THRESHOLD_FAR: u32 = 8;  // 非连拍 Layer 1 阈值
pub const NEARBY_SECS: i64 = 60;              // 连拍时间窗口（秒）
pub const DCT_THRESHOLD: u32 = 8;             // Layer 2 DCT 阈值
```

### candidate.rs — 重复候选扫描

#### 两层去重架构

| 层 | 算法 | 目的 |
|----|------|------|
| Layer 1 | Gradient pHash（64-bit），时间感知 Hamming 距离阈值 | O(n log n) 快速粗筛候选对 |
| Layer 2 | DCT pHash（64-bit），Hamming 距离 ≤ `DCT_THRESHOLD` | 排除截图 vs 照片等内容类型差异导致的假阳性 |

**时间感知阈值**：`time_threshold(ts_a, ts_b)` 根据 `taken_at` 差值返回 `SIMILARITY_THRESHOLD`（≤ 60 s，连拍）或 `SIMILARITY_THRESHOLD_FAR`（> 60 s）。`taken_at` 为 NULL 时使用严格阈值。

**退化 hash 过滤**：set bits < 10 或 > 54 的 hash 在 Layer 1 前直接跳过。过稀疏（全黑/纯色图）或过密集（系统性梯度图像）的 hash 因 XOR 距离先天偏小而产生假阳性。

**DCT 验证**：Layer 2 仅对通过 Layer 1 的候选对调用，用 `HashMap<i64, Option<u64>>` 按照片 ID 缓存 DCT hash 避免重复计算。图像文件无法打开时跳过 Layer 2 不过滤。

**Union-Find 聚类**：所有匹配对收集完成后，用 Union-Find（带路径压缩）将传递相似的照片合并为连通分量。连拍序列（A≈B, B≈C）形成一个组而非 C(n,2) 对组。`scan_full` 调用 `write_clusters`，`scan` 调用 `write_clusters_incremental`（合并已有 pending 组）。

#### scan()（增量）

1. 查询 `dedup_scanned_at IS NULL AND import_status='imported' AND phash IS NOT NULL`（新照片，含 `path/taken_at/id`）
2. 若无新照片 → 立即返回 0（无 DB 写操作）
3. 查询所有已扫描照片（`dedup_scanned_at IS NOT NULL`，含 `path/taken_at/id`）
4. 比较「新 × 已有」和「新 × 新」，调用 `should_pair()` 过滤
   - Layer 1：`is_degenerate` → `hamming_distance` → `time_threshold`
   - Layer 2：`compute_dcthash` 验证（两者均 Some 时才过滤）
5. 收集所有候选对，调用 `write_clusters_incremental(pool, &pairs)` 写入 DB
6. `UPDATE photos SET dedup_scanned_at = datetime('now') WHERE id IN (...)`（仅新照片）
7. 返回新增/扩展组数

#### scan_full()（全量多索引分桶）

1. 删除所有 pending 状态的旧组（`DELETE FROM dedup_groups WHERE status='pending'`）
2. `UPDATE photos SET dedup_scanned_at = NULL`（重置所有）
3. 加载全部有效 pHash 及 `path/taken_at/id`，解码为 8 字节数组，跳过退化 hash
4. 构建 4 个倒排索引：`segment_i → HashMap<u16, Vec<idx>>`（4 × 16-bit 分段）
5. 对每张照片，枚举各分段的 Hamming 距离 ≤ 2 的 137 个邻值，在对应索引中查候选
6. 用 `HashSet<(min_id, max_id)>` 去重，调用 `should_pair()` 验证（Layer 1 + Layer 2）
7. 收集所有候选对，调用 `write_clusters(pool, &pairs)` 写入 DB（Union-Find 聚类）
8. 打时间戳，返回新增组数

**正确性保证（鸽巢原理）**：64-bit 距离 ≤ 10 → 4 个 16-bit 分段中至少一个距离 ≤ 2（否则总距离 ≥ 12）。

### resolve()

- 将 `keep_ids` 中的成员 `dedup_members.keep = 1`
- 将同组其余成员 `photos.import_status = 'deleted'`（软删除，不操作文件）
- `photo_stats.active_count -= COUNT(dedup_members WHERE group_id=? AND keep=0)`
- 将 `dedup_groups.status = 'resolved'`
- `group_id` 不存在时返回 `AppError::NotFound`

---

## 模块：album

### organize.rs

```rust
pub async fn group_by_month(pool: &SqlitePool) -> Result<()>
pub async fn group_by_camera(pool: &SqlitePool) -> Result<()>
```

**月份分组**：`substr(taken_at, 1, 7)` 提取 `YYYY-MM`，`kind='time'`，名称形如 `2024-06`

**相机分组**：`camera` 字段，`kind='camera'`；`camera IS NULL` 的照片跳过

两者均使用 `INSERT OR IGNORE INTO photo_albums` 保证幂等，多次调用不产生重复关联。

内部 `ensure_album()` 辅助函数：先 `SELECT`，不存在则 `INSERT`，返回 album id。

### location.rs

```rust
pub async fn group_by_location(pool: &SqlitePool) -> Result<()>
pub async fn count_missing_geo(pool: &SqlitePool) -> Result<i64>
// Returns count of imported GPS photos with no geocache entry.
```

**流程**：
1. 查询所有 `import_status='imported' AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL` 的照片
2. 对每张照片：
   - 将 lat/lon 格式化为 4 位小数字符串作为缓存 key（`coord_key(f64) -> String`）
   - 查 `geocache` 表（同时读取 `city`/`state`/`country`）：
     - 三字段均为 NULL → 历史上 Nominatim 调用临时失败，视为缓存未命中，触发重试
     - city 为 NULL 但 country 等有值 → 合法的"仅知国家/省"结果，直接返回 NULL（不重试）
     - city 有值但 state 为 NULL → 旧版数据（直辖市修复前），视为过期条目，触发重试
     - city 和 state 均有值 → 完整命中，直接返回 city
   - 精确 key 未命中（或为可重试条目）时，先做邻近查找（proximity lookup）：
     在 ±`PROXIMITY_DEG`（0.01°，约 1 km）边界框内按距离升序找最近的有效缓存条目
     （city/state/country 至少有一个非 NULL），若命中则 `INSERT OR REPLACE` 到当前精确 key
     并返回，不调用 Nominatim、不触发限速
   - 邻近也未命中则调用 Nominatim API（见下），结果写入 `geocache`（`INSERT OR REPLACE`）
   - 相邻 API 调用之间 `tokio::time::sleep(1s)` 限速
3. 得到城市名后，`ensure_location_album()` 创建 `kind='location'` 相册并关联照片

**Nominatim 调用**：

```
GET https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=json&zoom=10
User-Agent: PicManager/0.1 (family photo manager)
Timeout: 10s
```

响应 `address` 对象按优先级依次取：`city` → `town` → `village` → `county` → `state`。
任何网络/解析错误均返回 `None`（不返回错误，不中断流程）。

### merge.rs

```rust
pub async fn merge(pool: &SqlitePool, source_id: i64, target_id: i64) -> Result<()>
```

- `source_id == target_id` → 返回 `AppError::NotFound`（自合并无意义）
- source 或 target 不存在 → 返回 `AppError::NotFound`
- `INSERT OR IGNORE INTO photo_albums` 将 source 的照片关联到 target
- `DELETE FROM photo_albums WHERE album_id = source_id`
- `DELETE FROM albums WHERE id = source_id`

---

## 模块：face

### 公开接口

```rust
// mod.rs
pub async fn analyze_one(pool: &SqlitePool, photo_id: i64, img: &DynamicImage)

// detector.rs
pub struct FaceRegion { pub x: i32, pub y: i32, pub width: i32, pub height: i32, pub confidence: f32 }
pub fn detect(img: &DynamicImage) -> Vec<FaceRegion>

// embedder.rs
pub struct Embedder { /* Mutex<Session> */ }
impl Embedder {
    pub fn load(model_path: &Path) -> Result<Self>
    pub fn extract(&self, img: &DynamicImage, region: &FaceRegion) -> Result<Vec<f32>>
}
pub fn encode_embedding(v: &[f32]) -> Vec<u8>   // f32 小端序 BLOB
pub fn decode_embedding(bytes: &[u8]) -> Vec<f32>

// job.rs
pub async fn run_job(pool: &SqlitePool, scope: Option<Vec<i64>>) -> Result<i64>
pub async fn scope_for_missing(pool: &SqlitePool) -> Result<Vec<i64>>
// Returns IDs of imported photos that have no entry in the faces table.
// Used by `picmanager faces analyze --missing-only` to fill gaps without full re-analysis.
pub async fn scope_for_rotated_with_faces(pool: &SqlitePool) -> Result<Vec<i64>>
// Returns IDs of imported photos with user rotation/flip set AND existing face records.
// Used by `picmanager faces analyze --rotated-only` to repair stale embeddings.
pub(crate) async fn reanalyze_one_photo(pool: &SqlitePool, photo_id: i64)
// Clears cover_face_id references, deletes faces, reopens image, calls analyze_one.
// Must clear cover_face_id first; otherwise FK constraint causes DELETE to fail silently.
// Called by execute_job and by PATCH /api/photos/{id} after rotation/flip changes.
pub(crate) async fn execute_job(pool: &SqlitePool, job_id: i64, scope: Option<Vec<i64>>) -> Result<()>
```

### detector.rs — 检测流程

**模型文件路径**：`{config_dir}/picmanager/models/face_detector.onnx`

**模型**：ultraface-slim-320（Linzaer），约 1 MB，输入 `[1,3,240,320]` float32 BGR `(pixel-127)/128`，输出：
- `scores [1,4420,2]`：每候选框的背景/人脸概率；取 `scores[i*2+1]` 为置信度
- `boxes [1,4420,4]`：归一化 x1y1x2y2；`boxes[i*4+j]` 访问

**后处理**：
1. 过滤 `confidence ≥ 0.5`
2. 坐标反归一化：`x1 = box[0] * orig_w` 等
3. IoU NMS（阈值 0.45），保留置信度最高框
4. 按置信度降序排列

**全局 Session**：`OnceLock<Option<Mutex<Session>>>`
- 首次调用时尝试从文件加载模型
- 模型不存在：`tracing::warn` + 返回 `None`；`detect()` 返回 `[]`
- `Session::run()` 需要 `&mut Session`，因此用 `Mutex` 包裹

### embedder.rs — 提取流程

**模型文件路径**：`{config_dir}/picmanager/models/arcface_mobilenetv1.onnx`

**预处理**：
1. 按 `region` 裁剪（各边扩展 20% padding，超出图像边界则 clamp）
2. `resize_exact(112, 112)`，RGB → `[1,3,112,112]` float32，`(pixel-127.5)/127.5`
3. 构造 `TensorRef::from_array_view(&arr)`

**后处理**：
1. `try_extract_tensor::<f32>()` → `(_shape, &[f32])` 元组
2. L2 归一化：`v[i] / sqrt(sum(v^2))`；零向量原样返回

**BLOB 编解码**：每个 f32 按小端序 4 字节写入，512 维 = 2048 字节

### job.rs — 批量重分析

`run_job()` 立即插入 `face_jobs` 记录（`status='running'`）并返回 `job_id`，随后 `tokio::spawn` 异步调用 `execute_job()`。

`execute_job()` 流程：
1. 查询 `photos WHERE import_status='imported'`，按 scope 过滤
2. 对每张照片：`DELETE FROM faces WHERE photo_id=?` → `image::open` → `analyze_one`
3. 每处理一张：`UPDATE face_jobs SET processed=processed+1 WHERE id=?`
4. 全部完成：`UPDATE face_jobs SET status='done', finished_at=datetime('now')`

`execute_job` 标为 `pub(crate)` 以便测试直接同步调用，避免 `tokio::spawn` 引入的不确定时序。

### cluster.rs — 人物 DBSCAN 聚类

```rust
// cluster.rs
pub const EPS: f32 = 0.35;            // 余弦距离阈值
pub const MIN_CONFIDENCE: f32 = 0.70; // 参与 DBSCAN 核心聚类的最低置信度

pub fn cluster_faces(faces: &[(i64, Vec<f32>)], eps: f32, min_samples: usize) -> Vec<Vec<i64>>
pub async fn run_clustering(pool: &SqlitePool) -> Result<usize>
pub async fn run_incremental_clustering(pool: &SqlitePool) -> Result<usize>
```

**基础算法**
- `faces`：`(face_id, embedding)` 列表，embedding 为 L2 归一化 512D 向量
- 距离度量：`1.0 - dot(a, b)`（余弦距离）
- `region_query` 包含点本身（标准 DBSCAN，min_samples=2 意味着至少 1 个其他邻居）
- 噪点各自单独成组（不丢弃），返回所有聚类（含噪点组）的 face_id 列表

**`run_clustering(pool)` — 全量重建，两阶段算法**

DBSCAN 存在**链式合并（chaining）缺陷**：若人脸 A→B→C 两两距离均 < eps，即使 A 与 C 差异很大（dist > eps），三者也会被并入同一组。置信度低的检测（小脸、侧脸、模糊脸）embedding 质量差，容易充当"桥接点"，把原本不相关的真实人物串联成一个巨型簇。

为此采用两阶段算法：

1. **Phase 1 — DBSCAN 核心聚类（仅高置信度脸）**
   - 筛选 `confidence >= MIN_CONFIDENCE (0.70)` 的人脸，以 `eps = 0.35`、`min_samples = 2` 运行 DBSCAN
   - 低置信度检测排除在 DBSCAN 之外，避免桥接效应
   - 噪点各自单独建人物记录

2. **Phase 2 — 低置信度脸后向归入**
   - 对每张 `confidence < 0.70` 的脸，计算其与所有已建人物的最小余弦距离
   - 距离 < EPS：归入最近人物
   - 否则：单独建一条人物记录（同 DBSCAN 噪点处理逻辑）

**`run_incremental_clustering(pool)` — 增量聚类（非破坏性）**
- 找出所有尚未归入任何人物的 face（`NOT EXISTS person_faces`）
- 逐一与已有人物比对：最小距离 < EPS 则归入最近人物
- 剩余无法匹配的脸再运行 DBSCAN → 新建人物；不修改已有人物记录
- **注意**：已有人物库很大时，每张新脸都会与库中全量 embedding 比对（O(n×m)）；小批量导入场景下性能可接受

---

## 模块：animal

### 公开接口

```rust
// mod.rs
pub async fn detect_and_save(pool: &SqlitePool, photo_id: i64, img: &DynamicImage)

// detector.rs
pub struct AnimalDetection {
    pub species: String,
    pub confidence: f32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
pub fn detect(img: &DynamicImage) -> Vec<AnimalDetection>
```

### detector.rs — 检测流程

**模型文件路径**：`{config_dir}/picmanager/models/yolov8n.onnx`（约 6 MB）

**预处理**：`resize_exact(640, 640)`，RGB 转 `[1,3,640,640]` float32，归一化 `[0,1]`（与 face detector 的 `(px-127)/128` BGR 不同）

**输出解析**：`[1, 84, 8400]`，feature-first 布局
- 访问第 i 个检测框的第 f 个特征：`flat[f * 8400 + i]`
- 前 4 行（f=0–3）：cx / cy / w / h（归一化到 640）
- 后 80 行（f=4–83）：COCO 类别分数（已在模型内做 softmax/sigmoid，无需再处理）

**动物 COCO 类别（0-based）**：

| index | 英文 | 中文 |
|-------|------|------|
| 14 | bird | 鸟 |
| 15 | cat | 猫 |
| 16 | dog | 狗 |
| 17 | horse | 马 |
| 18 | sheep | 羊 |
| 19 | cow | 牛 |
| 20 | elephant | 象 |
| 21 | bear | 熊 |
| 22 | zebra | 斑马 |
| 23 | giraffe | 长颈鹿 |

**后处理**：过滤 confidence ≥ 0.4，IoU NMS（阈值 0.45），坐标反归一化到原图像素

**全局 Session**：`OnceLock<Option<Mutex<Session>>>`，与 face detector 同一模式；模型不存在时 `tracing::warn` + 返回 `[]`

---

## 模块：storage

```rust
pub async fn connect(db_url: &str) -> Result<SqlitePool>
```

- `SqliteConnectOptions::create_if_missing(true).foreign_keys(true)`
- `sqlx::migrate!("./migrations").run(&pool)` 自动运行所有迁移
- 外键约束在运行时强制启用（`PRAGMA foreign_keys = ON`）

测试用内存库：

```rust
SqlitePoolOptions::new()
    .max_connections(1)  // 内存库必须单连接，否则连接关闭后数据丢失
    .connect("sqlite::memory:")
```

---

## 模块：web

### AppState

```rust
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub import_status: Arc<Mutex<ImportStatus>>,
    pub geo_running: Arc<AtomicBool>,
}
```

`import_status` 用 `Arc<Mutex<ImportStatus>>` 在主线程（HTTP handler）与 `tokio::spawn` 后台任务之间共享进度。`geo_running` 用 `Arc<AtomicBool>` 标记反地理编码后台任务是否正在运行，防止并发重入（已在运行时返回 `{"status":"already_running"}`）。

### 路由表

```
GET    /api/photos                        → list_photos
GET    /api/photos/gps-points             → get_gps_points
POST   /api/photos/batch-update           → batch_update_photos
GET    /api/photos/{id}                   → get_photo
PATCH  /api/photos/{id}                   → patch_photo
GET    /api/photos/{id}/thumb             → get_thumb
GET    /api/photos/{id}/file              → get_photo_file
GET    /api/photos/{id}/faces             → list_photo_faces
GET    /api/photos/{id}/animals           → list_photo_animals
POST   /api/import                        → start_import
GET    /api/import/status                 → get_import_status
GET    /api/dedup                         → list_dedup_groups
POST   /api/dedup/{group_id}/resolve      → resolve_group
GET    /api/albums                        → list_albums
GET    /api/albums/{id}/photos            → list_album_photos
POST   /api/albums/merge                  → merge_albums
POST   /api/faces/analyze                 → start_analyze
GET    /api/faces/jobs/{id}               → get_job_status
GET    /api/faces/{id}/thumb              → get_face_thumb
GET    /api/geo/hierarchy                 → get_geo_hierarchy
POST   /api/geo/regeocode                 → start_regeocode
GET    /api/geo/regeocode/status          → get_regeocode_status
GET    /api/people                        → list_people
GET    /api/people/tree                   → get_people_tree
POST   /api/people/cluster                → cluster_people
POST   /api/people/cluster/incremental    → cluster_people_incremental
POST   /api/people/merge                  → merge_people
PATCH  /api/people/{id}                   → patch_person
POST   /api/people/batch-update           → batch_update_people
GET    /api/people/{id}                   → get_person_photos
POST   /api/people/{id}/reparent          → reparent_person
GET    /api/people/{id}/merge-suggestions → get_merge_suggestions
GET    /api/people/{id}/outlier-faces     → get_outlier_faces
POST   /api/people/{id}/eject-face        → eject_face
GET    /api/people/{id}/centroid-faces    → get_centroid_faces
GET    /api/people/{id}/embedding-map     → get_embedding_map
GET    /api/animals/species               → list_species
GET    /api/animals/{species}/photos      → list_species_photos
/*     (fallback)                         → embed::static_handler（rust-embed 内嵌）
```

### import.rs

**`POST /api/import`**：

请求体：`{"dir": "/path/to/photos", "copy": false}`（`copy` 可省略，默认 `false`）

- 若 `import_status.running == true`，返回 `409 Conflict`
- 否则将 status 置为 `running=true`，`tokio::spawn` 执行 `importer::import_dir`（传入 `config.library_path` 和 `copy`）
- 任务结束后更新 status（summary 或 error），`running=false`
- 返回 `{"status": "started", "dir": "..."}`

**`GET /api/import/status`**：返回当前 `ImportStatus` JSON

```json
{
  "running": false,
  "total": 128,
  "imported": 120,
  "skipped": 8,
  "errors": 0,
  "source_dir": "/path/to/photos"
}
```

### photos.rs

**`GET /api/photos?page=1&per_page=50`**：

按 `taken_at DESC NULLS LAST, id DESC` 排序，返回：

```json
{
  "photos": [{ "id": 1, "path": "...", "format": "jpeg", "taken_at": "...", "camera": "...", "import_status": "imported" }],
  "total": 128,
  "page": 1,
  "per_page": 50
}
```

**`GET /api/photos/{id}/thumb`**：
- 查 `photos.path`，不存在返回 404
- 检查 `{thumb_cache_dir}/{id}.jpg`：存在则 `spawn_blocking { fs::read }` 直接返回
- 未命中：`spawn_blocking { decode → resize_to_fill(thumb_size, thumb_size) 正方形中心裁剪 → encode JPEG → write cache → return bytes }`
- `Content-Type: image/jpeg`；任何错误返回 500

**`GET /api/photos/{id}/file`**：
- 查 `photos.path` 和 `photos.format`，不存在返回 404
- `tokio::fs::read(path)` 读取原始文件字节
- `Content-Type` 由 `format` 列推断（jpeg→`image/jpeg`，png→`image/png`，gif→`image/gif`，webp→`image/webp`，heic/heif→`image/heic`，tiff→`image/tiff`，其他→`application/octet-stream`）
- 文件不存在返回 404，读取失败返回 404

`GET /api/photos`：`total` 字段读自 `photo_stats.active_count`（不执行 `COUNT(*)`）

### dedup.rs

**`GET /api/dedup`**：返回所有 `status='pending'` 的组，含成员 path/taken_at/camera：

```json
[{ "group_id": 1, "members": [{ "photo_id": 3, "path": "...", "taken_at": "...", "camera": "..." }] }]
```

**`POST /api/dedup/{group_id}/resolve`**：

```json
{ "keep": [3] }
```

调用 `dedup::resolve()`，成功返回 200，`group_id` 不存在返回 404。

### albums.rs

**`GET /api/albums`**：

```json
[{ "id": 1, "name": "2024-06", "kind": "time", "photo_count": 42 }]
```

按 `kind, name` 排序（时间相册 → 相机相册 → 地点相册 → 手动相册）。

**`GET /api/albums/{id}/photos?page=1&per_page=50`**：

按 `taken_at NULLS LAST, id` 排序，id 不存在返回 404。

**`POST /api/albums/merge`**：

```json
{ "source": 2, "target": 1 }
```

调用 `album::merge()`，成功 200，不存在 404。

### people.rs

**`GET /api/people`**：返回人物列表，含 `face_count`、`photo_count`、`cover_face_id`、`status`。支持查询参数：
- `?status=active|ignored|not_a_person|all`（默认 `active`，只返回 active 人物）
- `?name_exact=<name>`：精确姓名查找（用于重名检测），不受 status 过滤限制

**`PATCH /api/people/{id}`**：修改人物属性：
```json
{ "name": "张三", "status": "ignored" }
```
字段均可选。`id` 不存在时返回 404。

**`POST /api/people/batch-update`**：批量修改多个人物的状态：
```json
{ "ids": [1,2,3], "status": "ignored" }
```
返回 `{ "updated": N }`。

**`GET /api/people/tree`**：返回嵌套 JSON 树（`id, name, children: [...]`）。

**`GET /api/people/{id}`**：分页照片列表（通过 `person_faces → faces.photo_id` 关联）。

**`POST /api/people/cluster`**：异步触发 DBSCAN 重聚类，返回 `{ "job_id": ... }`。

**`POST /api/people/merge`**：`{ "source_id": 2, "target_id": 1 }` — 将 source 的所有 person_faces 并入 target，删除 source。操作在事务内完成，分两种情况：
- **普通合并**（source 不是 target 的祖先）：将 source 的子节点改到 target 下，移人脸，删 source。
- **父→子合并**（source 是 target 的祖先，用递归 CTE 检测）：先将 target.parent_id 改为 source.parent_id（target 升级到 source 的位置），再将 source 的其余子节点改到 target 下，移人脸，删 source。避免产生 parent_id 自引用的孤儿节点。
前端在两处合并入口（建议合并面板、合并到…对话框）均显示包含源/目标名称的确认弹窗，防止误操作。
- **建议合并面板**：`?limit=20` 拉取建议，结果按**已命名 / 未命名**分两组展示（组间有分隔符），各组内按相似度降序排列；条目超出宽度横向滚动。
- **合并到…对话框**：打开时同时拉取 `merge-suggestions?limit=20` 获取相似度数据；候选列表按相似度降序排列（无相似度数据的追加在末尾，按照片数降序），有相似度的条目右侧显示"N% 相似"。

**`POST /api/people/{id}/reparent`**：`{ "new_parent_id": 3 }` — `new_parent_id` 为 null 时提升为顶级。

**`POST /api/people`**：新建人物，`{ "name": "张三", "parent_id": 5 }`（两字段均可选）。返回 `{ "id": N }`。重名检测由前端负责，后端不做校验。

**`DELETE /api/people/{id}`**：删除人物节点。若该人物在 `person_faces` 中仍有人脸记录，返回 409 Conflict；人物不存在返回 404；成功返回 200。设计用于撤销操作中删除空容器节点。

**`POST /api/people/{id}/transfer`**：`{ "target_person_id": T, "photo_ids": [p1, p2] }` — 将当前人物中属于指定照片的人脸（通过 `faces.photo_id` 匹配）转移给目标人物；若目标人物 `cover_face_id` 为 null 则自动填充。返回 `{ "faces_moved": N }`。

**`POST /api/people/{id}/lift`**：`{ "name": "父节点名" }` — 在当前人物上方插入一个新父节点：新人物继承当前人物原 `parent_id`，当前人物的 `parent_id` 改为新人物。在同一事务中执行。返回 `{ "new_person_id": N }`。

**`GET /api/people/{id}/merge-suggestions?limit=20`**：使用置信度优先的精炼质心算法（见下）计算目标人物质心，对所有其他 active 人物也计算精炼质心，按余弦距离升序返回最多 `limit` 条建议（默认 5，上限 20）。若目标人物无 embedding 则返回空列表。
Response 数组元素：`{ person_id, name, cover_face_id, photo_count, face_count, distance }`

**`GET /api/people/{id}/outlier-faces?limit=5&min_dist=0.50`**：计算该人物的精炼质心，返回距质心余弦距离超过 `min_dist`（默认 0.50）的人脸，按距离降序排列，最多 `limit` 条（上限 50）。`min_dist=0` 可获取全部人脸（按距离排序，无阈值），用于诊断。若有效 embedding 数量 < 2 则返回空列表。
Response 数组元素：`{ face_id, photo_id, distance, confidence, x, y, width, height }`

**`POST /api/people/{id}/eject-face`**：Body `{ "face_id": N }` — 将指定人脸从当前人物中移出（删除 person_faces 记录），并为该人脸创建新的未命名人物。若 face_id 不属于该人物返回 404。
Response：`{ "new_person_id": N }`

**`GET /api/people/{id}/centroid-faces`**：计算该人物的精炼质心，返回参与质心计算的人脸所对应的照片 ID，以及全部人脸到质心的距离分布统计。
Response：`{ photo_ids: [N, ...], emb_count: N, centroid_size: N, min_dist: f, p25_dist: f, median_dist: f, p75_dist: f, max_dist: f }`
`centroid_size` 为实际参与精炼质心的人脸数，`emb_count` 为该人物总 embedding 数。

**精炼质心算法**：分两步计算。①置信度预过滤：优先选 confidence ≥ 0.85 的人脸（不足 10 张降至 ≥ 0.70，仍不足用全部）。②几何精炼：若候选数量 > 50，先算粗质心，取距粗质心最近的 40% 再算精炼质心。所有涉及质心的端点（merge-suggestions、outlier-faces、centroid-faces）均使用此算法。

**`GET /api/people/{id}/embedding-map`**：返回该人物（含所有后代）的所有人脸 embedding 经 PCA 降维后的 2D 坐标，用于前端分布图可视化。使用递归 CTE 取子树全部人脸，在后端 `src/face/pca.rs` 中通过 power iteration 求前两个主成分，坐标归一化到 `[−1, 1]`。
Response：`{ points: [{face_id, photo_id, person_id, x, y, confidence, taken_at}], total: N }`。人脸数 < 2 时返回空 points。人物不存在返回 404。

**`GET /api/faces/{id}/thumb`**：从 DB 读取 `exif_orientation, rotation, flip_h, flip_v`，将原图先通过 `apply_exif_orientation` 再通过 `apply_transform` 变换到显示空间，再按 faces 表中的 bbox（坐标已是显示空间）裁剪，`resize_to_fill(160, 160)`，`spawn_blocking` 生成 JPEG，磁盘缓存至 `.thumbs/face_{id}.jpg`。

### geo.rs

**`GET /api/geo/hierarchy`**：返回嵌套地理层级，每级含照片数：

```json
{ "countries": [
    { "name": "China", "photo_count": 500,
      "states": [
        { "name": "Beijing", "photo_count": 300,
          "cities": [{ "name": "Beijing", "photo_count": 300 }] }
      ]}
  ]}
```

**`POST /api/geo/regeocode`**：为数据库中有 GPS 坐标但 `geocache` 表尚无对应条目的照片触发反地理编码。

- 若后台任务已在运行，返回 `{"status":"already_running"}`（不启动新任务）
- 否则统计待处理数量，`tokio::spawn` 调用 `album::group_by_location()`，立即返回 `{"status":"started","count":N}`
- `AppState.geo_running`（`Arc<AtomicBool>`）在任务启动时置 `true`，完成后置 `false`

**`GET /api/geo/regeocode/status`**：返回 `{"running":true/false}`，供前端轮询。

**`GET /api/geo/photos`**：按地理层级过滤照片，支持分页。

查询参数：`country`、`state`、`city`（均可选）、`page`（默认 1）、`per_page`（默认 50，最大 200）。

- 参数值 `__null__` 表示过滤该字段为 NULL 的记录（对应前端 "Unknown" 条目）
- 省略参数则不对该字段过滤（如只传 `country` 则返回该国所有照片）
- 返回 `{ "total": N, "page": N, "per_page": N, "photos": [{id, path, taken_at, camera}] }`

### animals.rs

**`GET /api/animals/species`**：返回所有检测到的动物种类及照片数：

```json
[{ "species": "cat", "chinese": "猫", "photo_count": 42 }]
```

**`GET /api/animals/{species}/photos`**：分页照片列表（含该动物种类的照片）。

**`GET /api/photos/{id}/animals`**：该照片的所有动物检测结果：

```json
[{ "id": 1, "species": "cat", "confidence": 0.87, "x": 100, "y": 80, "width": 150, "height": 200 }]
```

---

## REST API 参考

| Method | Path | 请求 Body | 成功响应 | 错误 |
|--------|------|-----------|----------|------|
| GET | `/api/photos` | — | `PhotoList` JSON | 500 |
| GET | `/api/photos/gps-points` | — | GPS 坐标列表 JSON | 500 |
| POST | `/api/photos/batch-update` | `{"photo_ids":[...],"taken_at":"...","timezone_offset":480,"rotation_delta":90,"flip_h_toggle":true,"flip_v_toggle":true}` | `{"updated":N}` | 500 |
| GET | `/api/photos/{id}` | — | 单张照片详情 JSON | 404 / 500 |
| PATCH | `/api/photos/{id}` | `{"taken_at":"...","timezone_offset":480,"rotation_delta":90,"flip_h_toggle":true,"flip_v_toggle":true}` | 200 | 404 / 500 |

rotation_delta/flip_h_toggle/flip_v_toggle 生效时，除删除缩略图缓存外，还会在后台（`tokio::spawn`）调用 `reanalyze_one_photo` 以更新人脸 embedding 和 bbox 坐标到新的显示空间。batch-update 同理，对所有 photo_ids 依次重分析。
| GET | `/api/photos/{id}/thumb` | — | JPEG bytes | 404 / 500 |
| GET | `/api/photos/{id}/file` | — | 原始文件字节（MIME 由 format 推断） | 404 |
| GET | `/api/photos/{id}/faces` | — | `FaceResponse[]` JSON | 500 |
| GET | `/api/photos/{id}/animals` | — | `AnimalResponse[]` JSON | 500 |
| POST | `/api/import` | `{"dir":"...","copy":false}` | `{"status":"started"}` | 409（已在运行） |
| GET | `/api/import/status` | — | `ImportStatus` JSON | — |
| GET | `/api/dedup` | — | `DedupGroup[]` JSON | 500 |
| POST | `/api/dedup/{group_id}/resolve` | `{"keep":[id,...]}` | 200 | 404 / 500 |
| GET | `/api/albums` | — | `AlbumRow[]` JSON | 500 |
| GET | `/api/albums/{id}/photos` | — | 分页照片 JSON | 404 / 500 |
| POST | `/api/albums/merge` | `{"source":id,"target":id}` | 200 | 404 / 500 |
| GET | `/api/collections` | — | `CollectionRow[]` JSON（kind='curated'，按 created_at 降序） | 500 |
| POST | `/api/collections` | `{"name":"..."}` | 201 `{"id":N,"name":"..."}` | 400 / 500 |
| PATCH | `/api/collections/{id}` | `{"name":"..."}` | 200 | 400 / 404 / 500 |
| DELETE | `/api/collections/{id}` | — | 204 | 404 / 500 |
| GET | `/api/collections/{id}/photos` | `?page=N&per_page=N` | 分页照片 JSON（taken_at 排序） | 404 / 500 |
| POST | `/api/collections/{id}/photos` | `{"photo_ids":[...]}` | `{"added":N}` | 404 / 500 |
| DELETE | `/api/collections/{id}/photos` | `{"photo_ids":[...]}` | `{"removed":N}` | 404 / 500 |
| GET | `/api/faces/{id}/thumb` | — | JPEG bytes（人脸裁剪图） | 404 / 500 |
| GET | `/api/geo/hierarchy` | — | 地理层级嵌套 JSON | 500 |
| GET | `/api/geo/photos` | `?country=X&state=Y&city=Z&page=N&per_page=N` | `{total,photos[]}` | 500 |
| POST | `/api/geo/regeocode` | — | `{"status":"started","count":N}` 或 `{"status":"already_running"}` | 500 |
| GET | `/api/geo/regeocode/status` | — | `{"running":true/false}` | — |
| GET | `/api/people` | — | `PersonRow[]` JSON（默认 status=active） | 500 |
| GET | `/api/people/tree` | — | 嵌套树 JSON | 500 |
| POST | `/api/people/cluster` | — | `{"job_id":N}` | 500 |
| POST | `/api/people/cluster/incremental` | — | `{"clustered":N}` | 500 |
| POST | `/api/people/merge` | `{"source_id":N,"target_id":N}` | 200 | 404 / 500 |
| PATCH | `/api/people/{id}` | `{"name":"...","status":"..."}` | 200 | 404 / 500 |
| POST | `/api/people/batch-update` | `{"ids":[...],"status":"..."}` | `{"updated":N}` | 500 |
| GET | `/api/people/{id}` | — | 分页照片 JSON | 404 / 500 |
| POST | `/api/people/{id}/reparent` | `{"new_parent_id":N}` | 200 | 404 / 500 |
| GET | `/api/people/{id}/merge-suggestions` | `?limit=20` | `[{person_id,name,cover_face_id,photo_count,face_count,distance}]` | 404 / 500 |
| GET | `/api/people/{id}/outlier-faces` | `?limit=5&min_dist=0.50` | `[{face_id,photo_id,distance,confidence,x,y,width,height}]` | 404 / 500 |
| POST | `/api/people/{id}/eject-face` | `{"face_id":N}` | `{"new_person_id":N}` | 404 / 500 |
| GET | `/api/people/{id}/centroid-faces` | — | `{photo_ids,emb_count,centroid_size,min_dist,p25_dist,median_dist,p75_dist,max_dist}` | 404 / 500 |
| GET | `/api/people/{id}/embedding-map` | — | `{points:[{face_id,photo_id,person_id,x,y,confidence,taken_at}],total}` | 404 / 500 |
| GET | `/api/animals/species` | — | 种类列表 JSON | 500 |
| GET | `/api/animals/{species}/photos` | — | 分页照片 JSON | 500 |
| POST | `/api/faces/analyze` | `{"photo_ids":[...],"missing_only":false}` | `{"job_id":42}` | 500 |
| GET | `/api/faces/jobs/{id}` | — | `JobStatusResponse` JSON | 404 / 500 |
| GET | `/api/activities` | — | `ActivityList` JSON（可选 `?type=running` 过滤） | 500 |
| GET | `/api/activities/{id}` | — | `ActivityItem` JSON（元数据，不含轨迹点） | 404 / 500 |
| GET | `/api/activities/{id}/track` | — | `TrackResponse` JSON（轨迹点数组，>7200点时 RDP 压缩） | 404 / 500 |
| GET | `/api/activities/{id}/photos` | — | `PhotosResponse` JSON（时间范围内≤500m 轨迹的照片） | 404 / 500 |
| POST | `/api/activities/{id}/trim` | `{"start_time":"…","end_time":"…"}` | `ActivityItem` JSON（更新后的运动元数据） | 404 / 500 |
| POST | `/api/activities/merge` | `{"ids":[1,2],"title":"可选标题"}` | `ActivityItem` JSON（新合并记录）；400 `type_mismatch`/`time_overlap`/`missing_times`/`too_few` | 400 / 404 / 500 |

**FaceResponse 结构（`GET /api/photos/{id}/faces` 返回，含人物关联）：**

```json
{ "id": 1, "x": 120, "y": 80, "width": 200, "height": 200, "confidence": 0.97, "person_id": 3, "person_name": "张三" }
```

`person_id` 和 `person_name` 在人脸未归入任何人物时为 `null`。

**AnimalResponse 结构：**

```json
{ "id": 1, "species": "cat", "confidence": 0.87, "x": 100, "y": 80, "width": 150, "height": 200 }
```

**JobStatusResponse 结构：**

```json
{ "id": 42, "status": "running", "total": 1000, "processed": 312 }
```

---

## 前端

纯静态文件，无构建步骤。使用 `rust-embed` 在编译时将 `frontend/` 目录嵌入二进制，通过 `web/embed.rs` 的 `static_handler` fallback 路由提供服务（不依赖运行时工作目录）。

**导航标签页**：照片 | 人物 | 地点 | 动物 | 运动

**侧边栏区块**：导入照片 | 去重 | 元数据补全 | 相册

### 主要 JS 函数（app.js）

| 函数 | 说明 |
|------|------|
| `loadPhotos()` | 调用 `/api/photos` 或 `/api/albums/{id}/photos`，调用 `renderGrid()` |
| `renderGrid(photos)` | 生成 `<img loading="lazy">` 网格卡片，支持选中模式 |
| `loadAlbums()` | 调用 `/api/albums`，渲染侧边栏列表 |
| `selectAlbum(albumId, li)` | 切换选中相册，重置分页，调用 `loadPhotos()` |
| `startImport()` | POST `/api/import`，启动轮询 |
| `pollImport()` | 每 1.5s GET `/api/import/status`，结束后刷新相册和照片 |
| `openDedupModal()` | GET `/api/dedup`，渲染重复组弹窗（点击选中，确认保留） |
| `openPhotoDetail(id)` | GET `/api/photos/{id}`，弹出详情模态框（大图 + 元信息 + 人脸/动物 overlay） |
| `patchPhoto(id, data)` | PATCH `/api/photos/{id}`，保存时间/时区修改 |
| `batchUpdatePhotos(ids, data)` | POST `/api/photos/batch-update`，批量时间/时区调整 |
| `fillMissingFaces()` | POST `/api/faces/analyze`（`missing_only:true`）；每 2s 轮询 `/api/faces/jobs/{id}`，状态栏显示"X/Y / 完成 / 失败"；完成后若当前为人物视图则刷新 |
| `fillMissingGeo()` | POST `/api/geo/regeocode`；每 2s 轮询 `/api/geo/regeocode/status`，状态栏显示"处理中（N 张）/ 完成（N 张）"；完成后若当前为地点视图则刷新 |
| `loadPeopleList()` | GET `/api/people`，渲染人物卡片网格；选中的卡片（`.person-card.selected`）始终显示紫色勾选指示符（不依赖鼠标悬停） |
| `clusterPeople()` | POST `/api/people/cluster`，触发重聚类，轮询进度 |
| `loadPeopleTree()` | GET `/api/people/tree`，渲染层级树 |
| `loadGeoHierarchy()` | GET `/api/geo/hierarchy`，渲染三列钻取面板；点击任意层级触发 `loadGeoPhotos()` |
| `loadGeoPhotos()` | GET `/api/geo/photos?country=X&state=Y&city=Z`，直接按地理过滤照片；`city=__null__` 对应 Unknown 条目 |
| `initMap()` | 初始化 Leaflet.js 地图，GET `/api/photos/gps-points`，加载 marker cluster |
| `loadAnimals()` | GET `/api/animals/species`，渲染种类卡片网格 |
| `loadSpeciesPhotos(species)` | GET `/api/animals/{species}/photos`，渲染该种类照片 |
| `openTrimModal(id)` | 打开运动剪辑弹窗；加载轨迹数据，初始化 Leaflet 地图、canvas 图表、双把手拖拽 |
| `saveTrim()` | 弹出不可撤销确认框，POST `/api/activities/{id}/trim`，成功后刷新详情 |
| `openMergeModal()` | 读取 `activitiesState.selected`，计算合并预览统计，弹出合并确认框 |
| `saveMerge(acts)` | POST `/api/activities/merge`，成功后退出多选模式并刷新列表 |
| `exitMultiSelect()` | 清除多选状态，隐藏底部工具栏，重新渲染列表 |
| `updateMultiSelectBar()` | 更新「已选 N 个」计数 + 合并按钮的 disabled 状态 |

### 状态对象（state）

```js
{ page, perPage, total, albumId, importPollId }
```

---

## PhotoBridge（Swift）

`photobridge` 是独立的 Swift Package，通过 PhotoKit 从 iCloud Photos 导出照片文件，并可选地调用 `picmanager import` 子进程完成端到端导入。

### 关键接口

- `LibraryEnumerator.enumerate()` — 全量枚举 PHAsset，`selectExportResource` 过滤（只选 JPEG/HEIC/PNG 等主资源，跳过 RAW alternate、burst 未选帧）
- `IncrementalEnumerator.enumerate(token:)` — 使用 `PHPersistentChangeToken` 只获取上次同步后新增的资产；token 为 nil 时回退到 `LibraryEnumerator`
- `writeAssetResource(_:to:)` — `PHAssetResourceManager` 下载到指定路径；若文件已存在则报错（不覆盖）
- `writeAssetResourceOrientationFixed(_:for:to:)` — 调用 `writeAssetResource` 后，查询 Photos 显示方向并与文件 EXIF 比对，不一致时用 exiftool 修复；export / sync 均使用此函数替代 `writeAssetResource`
- `applyTimestamp(to:date:)` — `FileManager.setAttributes` 设置 mtime+ctime 为 `PHAsset.creationDate`
- `PicManagerRunner.importBatch(stagingDir:batchSize:)` — 调用 `picmanager import --log <file>`，解析 NDJSON 日志返回 `ImportResult`
- `parseImportLog(_ text: String) -> ImportResult` — 纯函数，解析 NDJSON 到 succeeded/skipped/failed 三桶
- `checkDiskSpace(stagingDir:assets:)` — 估算下载量与可用空间对比，返回 `DiskWarning?`
- `SyncState` — `Codable` 持久化到 `Application Support/PhotoBridge/state.json`

### 文件命名规则

导出文件名 = `localIdentifier` 中 `/` 替换为 `_`，拼接 UTI 对应扩展名：

```
"B5A8F3C2.../L0/001" → "B5A8F3C2..._L0_001.heic"
```

`fix-timestamps` 和 `PicManagerRunner` 均依赖此规则，不要在其他地方硬编码命名逻辑。

### NDJSON 日志格式

`picmanager import --log` 输出（每行一条 JSON）：

```json
{"path":"/staging/file.heic","status":"imported","sha256":"...","error":null,"ts":"..."}
```

`status` 值：
- `imported` — 导入成功，可删暂存文件
- `skipped` — SHA-256 重复，可删暂存文件
- `failed` — 导入失败，保留等重试

### macOS TCC 权限归属

`PHPhotoLibrary.requestAuthorization` 在命令行工具中调用时，macOS TCC 把权限请求归属到**启动该工具的终端 App**（如 iTerm2、Terminal.app），而不是 `photobridge` 二进制本身。首次运行需在终端中授权弹框；之后重跑即生效，无需重启。

---

## 测试策略

### 单元/集成测试（`cargo nextest run`，共 312 个，另有 1 个 `#[ignore]`）

| 模块 | 测试数 | 测试方式 |
|------|--------|----------|
| `error` | 3 | 错误消息格式验证 |
| `config` | 8 | 默认值、TOML 解析（`tempfile::NamedTempFile`） |
| `metadata::format` | 7 | magic bytes 识别（直接传字节切片） |
| `metadata::exif` | 6 | fixture 文件（7 个 fixture，含 EXIF 回退链） |
| `metadata::filename` | 12 | 纯逻辑，覆盖全部日期模式及非法日期拒绝 |
| `metadata::types` | 2 | `as_str()` / `is_supported()` |
| `importer::placer` | 5 | `tempdir` 临时目录，测试移动/复制/冲突/unknown |
| `importer::scanner` | 4 | `tempdir` 临时目录 + fixture |
| `importer::state` | 4 | 内存 SQLite（sha256 唯一性判断） |
| `importer` | 5 | 内存 SQLite + fixture / tempdir（含 copy_only、unknown/ 验证） |
| `dedup::hash` | 7 | fixture 文件（Gradient hash + DCT pHash，含缩放/不同图片对比） |
| `dedup::candidate` | 19 | 内存 SQLite（含增量/全量/时间感知阈值/鸽巢原理验证） |
| `dedup` | 5 | 内存 SQLite |
| `album::organize` | 4 | 内存 SQLite |
| `album::location` | 6 | 内存 SQLite，预填 geocache（无网络） |
| `album::merge` | 4 | 内存 SQLite |
| `storage::db` | 9 | 内存 SQLite（含所有迁移表验证） |
| `face::detector` | 9 | 纯函数（iou/nms/preprocess/FaceRegion）；2 个 `#[ignore]` 需模型 |
| `face::embedder` | 6 | 纯函数（l2_normalize/encode_decode/preprocess）；1 个 `#[ignore]` 需模型 |
| `face::cluster` | 5 | 纯函数（DBSCAN，无需模型）：明显分离/边界/空输入等场景 |
| `face` | 1 | 内存 SQLite；1 个 `#[ignore]` 需模型 |
| `face::job` | 6 | 内存 SQLite：3 个原有任务测试 + 3 个 `scope_for_missing` 测试 |
| `album::location` | 10 | 内存 SQLite：6 个原有地点相册测试 + 4 个 `count_missing_geo` 测试 |
| `animal::detector` | 5 | 纯函数（NMS/preprocess）；1 个 `#[ignore]` 需模型 |
| `web_api`（集成） | 32 | `tower::ServiceExt::oneshot`，内存 SQLite（含 people/geo/animals 新端点，Step 22 新增 7 个） |

### 内存 SQLite 约定

```rust
SqlitePoolOptions::new()
    .max_connections(1)      // 必须，否则内存数据不共享
    .connect("sqlite::memory:")
    .await.unwrap();
sqlx::migrate!("./migrations").run(&pool).await.unwrap();
```

### 测试构建优化

`[profile.test] opt-level = 2`（`Cargo.toml`）：图像处理测试从 40+ 秒降至 ~1.5 秒。

---

## 关键约束与边界情况

| 约束 | 说明 |
|------|------|
| 导入默认移动文件 | `import_dir(copy_only=false)` 将源文件 rename 到库目录；跨设备时降级为 copy+delete |
| `--copy` 保留源文件 | `copy_only=true` 时只复制，源文件保留；适合导入重要原始存档 |
| 日期推断四级回退 | EXIF 四字段 → 文件 mtime → 文件名模式 → unknown/；每级失败才尝试下一级 |
| 去重软删除 | `import_status='deleted'` 仅标记状态，不操作文件系统 |
| 导入幂等性 | 相同 sha256 的文件被 `decide()` 检测为 `AlreadyImported` 直接跳过（path 无关） |
| 文件名冲突处理 | 库内同名文件存在时追加 `_1`/`_2` 后缀，循环直至找到空位 |
| 相册关联幂等性 | `INSERT OR IGNORE INTO photo_albums` 保证多次分组不产生重复行 |
| Nominatim 限速 | 1 req/s，缓存命中不计入限速；NULL 结果也缓存以避免重试 |
| pHash 缺失 | 无法被 `image` crate 解码的格式（HEIC 等）pHash 为 NULL，跳过去重比较 |
| 后台导入并发 | 同时只允许一个导入任务（`status.running` 检查），重复请求返回 409 |
| 缩略图无缓存 | 每次请求都实时解码和缩放原图，无磁盘缓存 |
| ARW 格式 | magic bytes 为 TIFF，需同时满足"扩展名为 `.arw`"才被识别为 ARW |
| HEIC 渲染 | `image` crate 不解码 HEIC，缩略图生成会失败返回 500；pHash 也为 NULL |
| EXIF 存储格式 | kamadak-exif 将 `YYYY:MM:DD HH:MM:SS` 转为带破折号展示值再解析；fixture 用 Python 原始 EXIF 二进制生成（绕过 exiftool 的 XMP 写入问题） |
| ONNX 模型不存在 | `detector::detect()` 直接返回 `[]`，`Embedder::load()` 返回 `Err(ModelNotFound)`；导入流程中 embedding 为 NULL，不影响其他功能 |
| ort `Session::run()` 需要 `&mut self` | 全局 Session 必须用 `OnceLock<Option<Mutex<Session>>>`，取用时 `.lock().unwrap()` |
| ndarray 版本必须匹配 ort | ort 2.0.0-rc.12 依赖 ndarray 0.17；版本不一致导致 `TensorArrayData` trait bound 不满足，务必在 `Cargo.toml` 中固定 `ndarray = "0.17"` |
| 批量重分析任务同步测试 | `execute_job` 为 `pub(crate)` 供测试直接调用；测试不应同时调用 `run_job`（会 spawn 后台任务）和 `execute_job`，否则 `processed` 计数会累加两次 |
| `timezone_offset` 仅写数据库 | 修改 `timezone_offset` 或 `taken_at` 不回写 EXIF，原始文件永不修改 |
| DBSCAN min_samples 含点自身 | `region_query` 包含点自身，`min_samples=2` 意味着至少 1 个其他邻居 |
| YOLOv8 输出布局 feature-first | 输出 `[1,84,8400]`，访问第 i 框第 f 特征用 `flat[f * 8400 + i]`，不是 `flat[i * 84 + f]` |
| 两个检测器模型不存在时均静默 | face detector 返回 `[]`，animal detector 返回 `[]`；导入流程不中断，相关字段留 NULL |
