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
    hash.rs                dHash 计算，hamming_distance()
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
  storage/
    mod.rs                 re-export connect
    db.rs                  connect()，SQLitePool，运行迁移
  web/
    mod.rs                 AppState, router(), serve()
    handlers/
      import.rs            ImportStatus, start_import, get_import_status
      photos.rs            list_photos, get_thumb
      dedup.rs             list_dedup_groups, resolve_group
      albums.rs            list_albums, list_album_photos, merge_albums
      faces.rs             start_analyze, get_job_status, list_photo_faces
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
| cached_at | TEXT | 缓存时间 |

复合主键：`(lat_key, lon_key)`，`INSERT OR IGNORE` 保证幂等。

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

pub struct ImportSummary {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}
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

    // 三级日期推断
    date = meta.taken_at.map(|dt| dt.date())
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
// 使用 image_hasher，HashAlg::Gradient（dHash），返回 Base64 字符串

pub fn hamming_distance(a: &str, b: &str) -> Option<u32>
// 解析两个 Base64 字符串为 ImageHash<Box<[u8]>>，调用 .dist()

pub const SIMILARITY_THRESHOLD: u32 = 10;
```

### candidate.rs — 重复候选扫描

#### scan()（增量）

1. 查询 `dedup_scanned_at IS NULL AND import_status='imported' AND phash IS NOT NULL`（新照片）
2. 若无新照片 → 立即返回 0（无 DB 写操作）
3. 查询所有已扫描照片（`dedup_scanned_at IS NOT NULL`）
4. 比较「新 × 已有」和「新 × 新」，调用 `maybe_create_group()`
5. `UPDATE photos SET dedup_scanned_at = datetime('now') WHERE id IN (...)`（仅新照片）
6. 返回新增组数

#### scan_full()（全量多索引分桶）

1. `UPDATE photos SET dedup_scanned_at = NULL`（重置所有）
2. 加载全部有效 pHash，解码为 8 字节数组
3. 构建 4 个倒排索引：`segment_i → HashMap<u16, Vec<idx>>`（4 × 16-bit 分段）
4. 对每张照片，枚举各分段的 Hamming 距离 ≤ 2 的 137 个邻值，在对应索引中查候选
5. 用 `HashSet<(idx_a, idx_b)>` 去重，验证全 64-bit 距离 ≤ `SIMILARITY_THRESHOLD`
6. 通过验证的对调用 `create_group_if_absent()`
7. 打时间戳，返回新增组数

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
```

**流程**：
1. 查询所有 `import_status='imported' AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL` 的照片
2. 对每张照片：
   - 将 lat/lon 格式化为 4 位小数字符串作为缓存 key（`coord_key(f64) -> String`）
   - 查 `geocache` 表：命中则直接用缓存的城市名（`city` 可能为 NULL，表示历史失败）
   - 未命中则调用 Nominatim API（见下），结果写入 `geocache`（`INSERT OR IGNORE`）
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
}
```

`import_status` 用 `Arc<Mutex<ImportStatus>>` 在主线程（HTTP handler）与 `tokio::spawn` 后台任务之间共享进度。

### 路由表

```
GET  /api/photos                    → list_photos
GET  /api/photos/{id}/thumb         → get_thumb
GET  /api/photos/{id}/faces         → list_photo_faces
POST /api/import                    → start_import
GET  /api/import/status             → get_import_status
GET  /api/dedup                     → list_dedup_groups
POST /api/dedup/{group_id}/resolve  → resolve_group
GET  /api/albums                    → list_albums
GET  /api/albums/{id}/photos        → list_album_photos
POST /api/albums/merge              → merge_albums
POST /api/faces/analyze             → start_analyze
GET  /api/faces/jobs/{id}           → get_job_status
/*   (fallback)                     → embed::static_handler（rust-embed 内嵌）
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
- 未命中：`spawn_blocking { decode → thumbnail(thumb_size) → encode JPEG → write cache → return bytes }`
- `Content-Type: image/jpeg`；任何错误返回 500

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

---

## REST API 参考

| Method | Path | 请求 Body | 成功响应 | 错误 |
|--------|------|-----------|----------|------|
| GET | `/api/photos` | — | `PhotoList` JSON | 500 |
| GET | `/api/photos/{id}/thumb` | — | JPEG bytes | 404 / 500 |
| GET | `/api/photos/{id}/faces` | — | `FaceResponse[]` JSON | 500 |
| POST | `/api/import` | `{"dir":"...","copy":false}` | `{"status":"started"}` | 409（已在运行） |
| GET | `/api/import/status` | — | `ImportStatus` JSON | — |
| GET | `/api/dedup` | — | `DedupGroup[]` JSON | 500 |
| POST | `/api/dedup/{group_id}/resolve` | `{"keep":[id,...]}` | 200 | 404 / 500 |
| GET | `/api/albums` | — | `AlbumRow[]` JSON | 500 |
| GET | `/api/albums/{id}/photos` | — | 分页照片 JSON | 404 / 500 |
| POST | `/api/albums/merge` | `{"source":id,"target":id}` | 200 | 404 / 500 |
| POST | `/api/faces/analyze` | `{"photo_ids":[...]}` | `{"job_id":42}` | 500 |
| GET | `/api/faces/jobs/{id}` | — | `JobStatusResponse` JSON | 404 / 500 |

**FaceResponse 结构：**

```json
{ "id": 1, "x": 120, "y": 80, "width": 200, "height": 200, "confidence": 0.97 }
```

**JobStatusResponse 结构：**

```json
{ "id": 42, "status": "running", "total": 1000, "processed": 312 }
```

---

## 前端

纯静态文件，无构建步骤。使用 `rust-embed` 在编译时将 `frontend/` 目录嵌入二进制，通过 `web/embed.rs` 的 `static_handler` fallback 路由提供服务（不依赖运行时工作目录）。

### 主要 JS 函数（app.js）

| 函数 | 说明 |
|------|------|
| `loadPhotos()` | 调用 `/api/photos` 或 `/api/albums/{id}/photos`，调用 `renderGrid()` |
| `renderGrid(photos)` | 生成 `<img loading="lazy">` 网格卡片 |
| `loadAlbums()` | 调用 `/api/albums`，渲染侧边栏列表 |
| `selectAlbum(albumId, li)` | 切换选中相册，重置分页，调用 `loadPhotos()` |
| `startImport()` | POST `/api/import`，启动轮询 |
| `pollImport()` | 每 1.5s GET `/api/import/status`，结束后刷新相册和照片 |
| `openDedupModal()` | GET `/api/dedup`，渲染重复组弹窗（点击选中，确认保留） |

### 状态对象（state）

```js
{ page, perPage, total, albumId, importPollId }
```

---

## 测试策略

### 单元/集成测试（`cargo nextest run`，共 123 个，另有 4 个 `#[ignore]`）

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
| `dedup::hash` | 4 | fixture 文件（含缩放/不同图片对比） |
| `dedup::candidate` | 4 | 内存 SQLite |
| `dedup` | 5 | 内存 SQLite |
| `album::organize` | 4 | 内存 SQLite |
| `album::location` | 6 | 内存 SQLite，预填 geocache（无网络） |
| `album::merge` | 4 | 内存 SQLite |
| `storage::db` | 9 | 内存 SQLite（含 faces/face_jobs 表验证） |
| `face::detector` | 9 | 纯函数（iou/nms/preprocess/FaceRegion）；2 个 `#[ignore]` 需模型 |
| `face::embedder` | 6 | 纯函数（l2_normalize/encode_decode/preprocess）；1 个 `#[ignore]` 需模型 |
| `face` | 1 | 内存 SQLite；1 个 `#[ignore]` 需模型 |
| `face::job` | 3 | 内存 SQLite，直接调用 `execute_job`（同步，避免 spawn 竞争） |
| `web_api`（集成） | 13 | `tower::ServiceExt::oneshot`，内存 SQLite |

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
| 日期推断三级回退 | EXIF 四字段 → 文件名模式 → unknown/；每级失败才尝试下一级 |
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
