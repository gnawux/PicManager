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
9. [模块：storage](#模块-storage)
10. [模块：web](#模块-web)
11. [REST API 参考](#rest-api-参考)
12. [前端](#前端)
13. [测试策略](#测试策略)
14. [关键约束与边界情况](#关键约束与边界情况)

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
    scanner.rs             递归目录扫描，magic bytes 格式过滤
    state.rs               SHA-256 计算，ImportDecision 枚举
  metadata/
    mod.rs                 re-export extract_from_file
    exif.rs                EXIF 解析（时间/GPS/相机）
    format.rs              magic bytes 格式检测
    types.rs               ImageFormat 枚举，PhotoMeta 结构体
  dedup/
    mod.rs                 list_groups(), resolve(), scan()
    hash.rs                dHash 计算，hamming_distance()
    candidate.rs           O(n²) pHash 比较，写入 dedup_groups
  album/
    mod.rs                 re-export 三个分组函数和 merge
    organize.rs            group_by_month(), group_by_camera()
    location.rs            group_by_location()，Nominatim 反地理编码
    merge.rs               merge(source_id, target_id)
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
frontend/
  index.html               主页面（侧边栏 + 照片网格 + 弹窗骨架）
  style.css                布局与主题（Catppuccin 深色侧边栏）
  app.js                   原生 JS，调用 REST API
migrations/
  0001_initial.sql         基础表（photos, albums, photo_albums, dedup_*, import_sessions）
  0002_geocache.sql        geocache 表（GPS → 城市名缓存）
tests/
  web_api.rs               Web API 集成测试（tower oneshot）
  fixtures/
    with_exif.jpg          带 EXIF（时间 2024-06-15 10:30:00，GPS 旧金山，相机 iPhone 15 Pro）
    no_exif.jpg            无 EXIF（同一场景，EXIF 已剥离）
    with_exif_small.jpg    with_exif.jpg 缩小版（验证 pHash 相似性）
    different.jpg          视觉上不同的图片（验证 pHash 区分度）
```

---

## 数据库 Schema

### photos

| 列 | 类型 | 说明 |
|----|------|------|
| id | INTEGER PK | 自增主键 |
| path | TEXT UNIQUE | 源文件绝对路径（不复制文件） |
| sha256 | TEXT | 文件内容 SHA-256 哈希（用于精确去重） |
| phash | TEXT NULL | 感知哈希 Base64（dHash，导入时计算） |
| taken_at | TEXT NULL | EXIF DateTimeOriginal，格式 `YYYY-MM-DD HH:MM:SS` |
| gps_lat | REAL NULL | GPS 纬度（十进制度，南为负） |
| gps_lon | REAL NULL | GPS 经度（十进制度，西为负） |
| camera | TEXT NULL | `{Make} {Model}`，若 model 已含 make 则只取 model |
| format | TEXT | `jpeg` / `png` / `gif` / `webp` / `heic` / `arw` / `unknown` |
| import_status | TEXT | `pending` / `imported` / `duplicate` / `deleted` |
| imported_at | TEXT | 导入时间（UTC，`datetime('now')`） |

索引：`sha256`、`import_status`、`taken_at`

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
pub async fn import_dir(pool: &SqlitePool, source_dir: &Path) -> Result<ImportSummary>

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
     （supported extensions: jpg/jpeg/png/gif/webp/heic/heif/arw）

for each path:
  compute_sha256(path)
    └─ 分块读取，sha2::Sha256

  decide(pool, path, sha256)
    ├─ 数据库中存在相同 sha256 且 path 相同 → AlreadyImported
    ├─ 数据库中存在相同 sha256 但 path 不同 → Duplicate { existing_path }
    └─ 其他 → New

  if AlreadyImported / Duplicate:
    INSERT OR IGNORE（幂等），返回 skipped += 1

  if New:
    metadata::extract_from_file(path)   // EXIF
    dedup::hash::compute_phash(path)    // dHash（失败则 phash=NULL）
    INSERT INTO photos ... import_status='imported'
    返回 imported += 1

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

### state.rs

```rust
pub enum ImportDecision {
    New,
    AlreadyImported,
    Duplicate { existing_path: String },
}

pub fn compute_sha256(path: &Path) -> Result<String>
pub async fn decide(pool: &SqlitePool, path: &Path, sha256: &str) -> Result<ImportDecision>
```

---

## 模块：metadata

### 公开接口

```rust
pub fn extract_from_file(path: &Path) -> Result<PhotoMeta>

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

**时间**：`Tag::DateTimeOriginal`，格式 `%Y-%m-%d %H:%M:%S`（kamadak-exif 展示值）

**相机**：`Tag::Make` + `Tag::Model`，去双引号后拼接；若 model 已含 make 前缀则只用 model

**GPS**：`Tag::GPSLatitude` / `Tag::GPSLongitude`（各三个 Rational：度/分/秒）+ 方向参考（`S`/`W` 为负）

```
decimal = deg + min/60 + sec/3600
若 ref == 'S' 或 'W' 则取负值
```

---

## 模块：dedup

### 公开接口

```rust
pub async fn scan(pool: &SqlitePool) -> Result<usize>
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

`scan()` 流程：
1. 查询所有 `import_status = 'imported'` 且 `phash IS NOT NULL` 的照片
2. O(n²) 两两比较汉明距离，找出 `distance <= SIMILARITY_THRESHOLD` 的对
3. 对每对检查是否已存在 `pending` 的 `dedup_group`（避免重复写入）
4. 新建 `dedup_groups`（`status='pending'`）+ `dedup_members`
5. 返回新增组数

> **注意**：n 大时性能为 O(n²)，当前实现未做分桶优化。

### resolve()

- 将 `keep_ids` 中的成员 `dedup_members.keep = 1`
- 将同组其余成员 `photos.import_status = 'deleted'`（软删除，不操作文件）
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
POST /api/import                    → start_import
GET  /api/import/status             → get_import_status
GET  /api/dedup                     → list_dedup_groups
POST /api/dedup/{group_id}/resolve  → resolve_group
GET  /api/albums                    → list_albums
GET  /api/albums/{id}/photos        → list_album_photos
POST /api/albums/merge              → merge_albums
/*   (fallback)                     → ServeDir("frontend/")
```

### import.rs

**`POST /api/import`**：
- 若 `import_status.running == true`，返回 `409 Conflict`
- 否则将 status 置为 `running=true`，`tokio::spawn` 执行 `importer::import_dir`
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
- `image::ImageReader::open().decode().thumbnail(thumb_size, thumb_size)`
- 编码为 JPEG，`Content-Type: image/jpeg`
- 解码/编码失败返回 500（不缓存）

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
| POST | `/api/import` | `{"dir":"..."}` | `{"status":"started"}` | 409（已在运行） |
| GET | `/api/import/status` | — | `ImportStatus` JSON | — |
| GET | `/api/dedup` | — | `DedupGroup[]` JSON | 500 |
| POST | `/api/dedup/{group_id}/resolve` | `{"keep":[id,...]}` | 200 | 404 / 500 |
| GET | `/api/albums` | — | `AlbumRow[]` JSON | 500 |
| GET | `/api/albums/{id}/photos` | — | 分页照片 JSON | 404 / 500 |
| POST | `/api/albums/merge` | `{"source":id,"target":id}` | 200 | 404 / 500 |

---

## 前端

纯静态文件，无构建步骤，由 `tower-http::ServeDir("frontend/")` 托管。

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

### 单元/集成测试（`cargo nextest run`，共 76 个）

| 模块 | 测试数 | 测试方式 |
|------|--------|----------|
| `error` | 3 | 错误消息格式验证 |
| `config` | 8 | 默认值、TOML 解析（`tempfile::NamedTempFile`） |
| `metadata::format` | 7 | magic bytes 识别（直接传字节切片） |
| `metadata::exif` | 3 | fixture 文件（`with_exif.jpg` / `no_exif.jpg`） |
| `metadata::types` | 2 | `as_str()` / `is_supported()` |
| `importer::scanner` | 4 | `tempdir` 临时目录 + fixture |
| `importer::state` | 5 | 内存 SQLite |
| `importer` | 4 | 内存 SQLite + fixture / tempdir |
| `dedup::hash` | 4 | fixture 文件（含缩放/不同图片对比） |
| `dedup::candidate` | 4 | 内存 SQLite |
| `dedup` | 5 | 内存 SQLite |
| `album::organize` | 4 | 内存 SQLite |
| `album::location` | 6 | 内存 SQLite，预填 geocache（无网络） |
| `album::merge` | 4 | 内存 SQLite |
| `storage::db` | 4 | 内存 SQLite |
| `web_api`（集成） | 7 | `tower::ServiceExt::oneshot`，内存 SQLite |

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
| 原始文件不修改 | 所有写操作只修改数据库；源文件和"已删除"文件均不做 FS 操作 |
| 去重软删除 | `import_status='deleted'` 仅标记状态，`/api/photos` 仍会返回这些记录 |
| 导入幂等性 | 相同 sha256 + 相同 path 会被 `INSERT OR IGNORE` 跳过 |
| 相册关联幂等性 | `INSERT OR IGNORE INTO photo_albums` 保证多次分组不产生重复行 |
| Nominatim 限速 | 1 req/s，缓存命中不计入限速；NULL 结果也缓存以避免重试 |
| pHash 缺失 | 不支持 EXIF 的格式（无法被 `image` crate 解码）pHash 为 NULL，跳过去重比较 |
| 后台导入并发 | 同时只允许一个导入任务（`status.running` 检查），重复请求返回 409 |
| 缩略图无缓存 | 每次请求都实时解码和缩放原图，无磁盘缓存 |
| ARW 格式 | magic bytes 为 TIFF，需同时满足"扩展名为 `.arw`"才被识别为 ARW |
| HEIC 渲染 | `image` crate 不解码 HEIC，缩略图生成会失败返回 500；pHash 也为 NULL |
