# 开发计划

目标：以最小可运行增量推进，每步结束后都能编译并有可验证的输出。

---

## Step 1 — 项目脚手架

**目标**：建好骨架，能编译，能跑。

- `cargo new picmanager`，配置 `Cargo.toml`，引入本项目需要的全部依赖
- 建立模块目录结构（importer / metadata / dedup / album / storage / web）
- 写 `error.rs`（统一 `AppError` 枚举）和 `config.rs`（库路径、端口等配置结构体）
- `main.rs` 仅打印版本号退出，各模块 `mod.rs` 为空占位

**验收**：`cargo build` 无报错，`cargo clippy` 无警告。

---

## Step 2 — 数据库 Schema 与连接

**目标**：建好数据层，后续模块都往这里写。

- 编写 `migrations/` 下的初始迁移文件，建表：
  - `photos`（id, path, sha256, phash, taken_at, gps_lat, gps_lon, camera, import_status）
  - `albums`（id, name, kind, created_at）
  - `photo_albums`（photo_id, album_id）
  - `dedup_groups`（id, status）、`dedup_members`（group_id, photo_id, keep）
- `storage/db.rs`：用 sqlx 建立 SQLitePool，运行迁移，暴露 `get_pool()` 函数
- 写一个简单的集成测试：建连接、插一条 photo 记录、查回来

**验收**：`cargo nextest run` 测试通过，`sqlite3` 能打开生成的 `.db` 文件看到表结构。

---

## Step 3 — 元数据提取

**目标**：给一张照片文件，能拿到时间、相机、GPS。

- `metadata/format.rs`：按 magic bytes 识别 JPG / PNG / WebP / GIF / HEIC / ARW
- `metadata/exif.rs`：用 `kamadak-exif` 读取 DateTimeOriginal、Make+Model、GPS IFD；
  HEIC 走 `libheif-rs`；ARW 复用 EXIF 通道（ARW 内嵌标准 EXIF）
- `metadata/types.rs`：定义 `PhotoMeta { taken_at, camera, lat, lon, format }` 结构体
- 暴露 `metadata::extract(path) -> Result<PhotoMeta>` 作为统一入口
- 单元测试：用 `tests/fixtures/` 中的样本图片验证提取结果

**验收**：对一张带 GPS 的 JPG 调用 `extract()`，能正确拿到时间和坐标。

---

## Step 4 — 导入器

**目标**：扫描指定目录，把照片导入库，跟踪状态。

- `importer/scanner.rs`：`walkdir` 递归目录，调用 `format::detect()` 过滤格式
- `importer/state.rs`：计算 SHA-256，查 DB 判断是否已导入，返回 `ImportDecision`
  （`New` / `AlreadyImported` / `Duplicate`）
- `importer/mod.rs`：串联扫描 → 元数据提取 → 写库 → 自动建时间相册，
  返回 `ImportSummary { total, imported, skipped, errors }`
- CLI 子命令 `picmanager import <dir>` 调用导入器，打印摘要

**验收**：`picmanager import ~/Desktop/test_photos/`，DB 中出现对应记录，重复执行不重复写入。

---

## Step 5 — Web 服务器骨架 + 照片列表 API

**目标**：跑起 Web 服务，能通过 HTTP 查到导入的照片。

- `web/server.rs`：Axum 启动，绑定 `127.0.0.1:8080`，共享 `AppState { pool, config }`
- `web/routes.rs`：注册路由
- `web/handlers/photos.rs`：
  - `GET /api/photos`：分页返回照片列表（id, path, taken_at, camera, import_status）
  - `GET /api/photos/:id/thumb`：读取原图，用 `image` crate 缩到 300px，返回 JPEG
- `web/handlers/import.rs`：`POST /api/import` 触发后台导入任务（tokio::spawn），
  `GET /api/import/status` 返回进度
- CLI 子命令 `picmanager serve` 启动 Web 服务

**验收**：`picmanager serve` 启动后，`curl http://localhost:8080/api/photos` 返回 JSON 列表；
浏览器能看到缩略图。

---

## Step 6 — 感知哈希（pHash）与重复候选发现

**目标**：能识别视觉相似的图片（缩放、轻微裁剪后的副本）。

- 添加 `image-hasher` 依赖，在 `dedup/hash.rs` 中实现：
  - `compute_phash(path) -> Result<String>`：计算 64 位感知哈希，存为 hex 字符串
  - `hamming_distance(a: &str, b: &str) -> u32`：计算两个哈希的汉明距离
- 在导入流程（`importer/mod.rs`）中写入 `photos.phash` 字段
- `dedup/candidate.rs`：扫描所有照片，找出 hamming distance ≤ 10 的组合，
  写入 `dedup_groups` / `dedup_members` 表，状态为 `pending`
- 暴露 `dedup::scan(pool) -> Result<usize>`，返回发现的重复组数量
- 单元测试：同一图片的缩略版 pHash 距离应 ≤ 10；不同图片距离应 > 10

**验收**：将同一张照片缩放后的副本放入测试目录，`dedup::scan()` 能将其归入同一组。

---

## Step 7 — 去重确认工作流

**目标**：用户可以查看重复候选，选择保留哪张，确认后删除副本。

- `dedup/mod.rs`：
  - `list_groups(pool) -> Result<Vec<DedupGroup>>`：查询待确认的重复组（含组内各照片元数据）
  - `resolve(pool, group_id, keep_ids: &[i64]) -> Result<()>`：
    标记保留项，将其余项从 `photos` 表软删除（`import_status = 'deleted'`），**不操作文件系统**
- Web API：
  - `GET /api/dedup`：返回待确认重复组列表（含组内照片信息）
  - `POST /api/dedup/:group_id/resolve`：body `{ "keep": [photo_id, ...] }`，执行确认
- CLI 子命令 `picmanager dedup`：打印重复组，交互式逐组确认（y/n/skip）
- 单元测试：resolve 后保留项状态不变，其余项状态变为 `deleted`

**验收**：`curl POST /api/dedup/1/resolve` 后重新查询，该组状态变为 `resolved`，被删除项不再出现在照片列表中。

---

## Step 8 — 相册自动分组

**目标**：导入后按时间和相机自动建相册，并可通过 API 查询。

- `album/organize.rs`：
  - `group_by_month(pool) -> Result<()>`：按 `taken_at` 年月建相册（形如 `2024-06`），
    将照片写入 `photo_albums`，已存在的相册追加而非重建
  - `group_by_camera(pool) -> Result<()>`：按 `camera` 字段建相册，无相机信息的照片跳过
- 在 `importer::import_dir()` 完成后自动调用两个分组函数
- Web API：
  - `GET /api/albums`：返回相册列表（id, name, kind, photo_count）
  - `GET /api/albums/:id/photos`：返回相册内照片列表（支持分页）
- 单元测试：导入 2 张不同月份的照片后，应生成 2 个时间相册；
  导入同相机照片后，相机相册中含正确数量

**验收**：`picmanager import <dir>` 后，`GET /api/albums` 返回按月份和相机划分的相册列表。

---

## Step 9 — Web 前端基础界面

**目标**：在浏览器中能看到照片网格、相册列表、触发导入。

- `frontend/` 目录下纯静态文件（HTML + CSS + 原生 JS，无构建步骤）：
  - `index.html`：主页面骨架（左侧相册导航 + 右侧照片网格）
  - `app.js`：调用已有 REST API，渲染缩略图网格（懒加载）、相册列表、分页
  - `style.css`：最小化样式（网格布局、响应式）
- Axum 添加静态文件服务，将 `frontend/` 挂载到 `/`（使用 `tower-http::ServeDir`）
- 导入面板：输入框填写目录路径，提交后轮询 `/api/import/status` 显示进度
- 单元测试不适用前端逻辑；验证静态文件路由能正确返回 `index.html`

**验收**：浏览器打开 `http://localhost:8080`，能看到照片网格，点击相册能过滤显示，能从界面触发导入并看到进度。

---

## Step 10 — 配置文件支持 + 相册手动合并

**目标**：支持持久化配置，用户可以合并自动生成的相册。

- 配置文件：`~/.config/picmanager/config.toml`，支持覆盖库路径、端口、缩略图尺寸；
  启动时自动加载，命令行参数优先级高于配置文件；
  添加 `toml` 和 `serde` 依赖完成解析
- `album/merge.rs`：`merge(pool, source_id, target_id) -> Result<()>`——
  将 source 相册的所有照片并入 target，删除 source 相册记录
- Web API：`POST /api/albums/merge`，body `{ "source": id, "target": id }`
- CLI 子命令 `picmanager config show` 打印当前生效配置（含来源：默认值 / 配置文件 / 命令行）
- 单元测试：merge 后 source 相册不存在，target 相册包含两者全部照片，无重复关联

**验收**：编辑 `~/Library/Application Support/picmanager/config.toml` 修改端口后重启生效；
`POST /api/albums/merge` 合并后相册数量减一，照片全部保留。

---

## Step 11 — 按地点（GPS）自动划分相册

**目标**：GPS 坐标已提取并入库，现在让有 GPS 的照片按拍摄地点自动归集为地点相册。

**背景**：需求原文为"按照时间和地点、拍摄相机划分相册"，当前只实现了时间和相机两个维度，地点维度尚缺。

- 新增 `reqwest` 依赖（带 `json` feature），用于调用 OSM Nominatim 免费反地理编码 API
- 新建迁移文件，添加 `geocache` 表（`lat_key TEXT, lon_key TEXT, city TEXT, cached_at TEXT`）缓存坐标→地名映射，避免重复请求
- `album/location.rs`：
  - `reverse_geocode(lat, lon, pool) -> Result<Option<String>>`：先查 `geocache`，命中则直接返回；否则调用 `https://nominatim.openstreetmap.org/reverse`，解析 `city` / `town` / `county` 字段，写入缓存
  - 严格限速 1 req/s（Nominatim 使用条款要求），连续请求间插入 `tokio::time::sleep(1s)`
  - `group_by_location(pool) -> Result<()>`：查询所有有 GPS 且尚未归入地点相册的照片，逐一反解地名，按地名建 `kind = 'location'` 相册，将照片写入 `photo_albums`（`INSERT OR IGNORE`，保持幂等）
  - 无 GPS 的照片直接跳过，不报错
- 在 `importer::import_dir()` 末尾与 `group_by_month` / `group_by_camera` 一同调用
- 现有 `GET /api/albums` 接口无需改动，地点相册会自动出现在返回列表中
- 单元测试：
  - 有 GPS 坐标的照片导入后，`geocache` 中有对应记录
  - 同一坐标两次调用只触发一次网络请求（缓存命中）
  - 无 GPS 的照片导入后不产生地点相册

**验收**：导入一批带 GPS 的照片后，`GET /api/albums` 返回列表中出现 `kind = 'location'` 的相册，相册名为可识别的城市/地区名；无 GPS 的照片不影响导入流程。

---

## Step 12 — 拍摄日期推断增强（EXIF 多字段回退 + 文件名解析）

**目标**：构建完整的日期推断链，依次尝试 EXIF 四个时间字段、文件名模式，均失败才归入 `unknown/`。

**背景**：当前 `metadata/exif.rs` 只读取 `DateTimeOriginal`，漏掉了 `DateTimeDigitized`、GPS 时间和 `DateTime`；部分手机导出文件缺少 EXIF 但文件名含日期。此步骤统一实现所有推断逻辑，供 Step 13 的文件放置模块直接调用。

### 12a — 更新 `metadata/exif.rs`：EXIF 多字段回退

修改 `parse_datetime(exif)` 函数，按优先级依次尝试：

| 优先级 | 字段 | Tag | 说明 |
|--------|------|-----|------|
| 1 | DateTimeOriginal | 0x9003 | 相机按快门时写入，最可靠 |
| 2 | DateTimeDigitized | 0x9004 | 数字化时间，数码相机通常与 Original 相同 |
| 3 | GPS DateStamp + TimeStamp | GPS IFD | UTC 时间，需合并两个字段；时区偏差可接受 |
| 4 | DateTime | 0x0132 | 最后修改时间，可能被编辑软件改写，最后兜底 |

- GPS 时间合并：`GPSDateStamp`（`YYYY:MM:DD`）+ `GPSTimeStamp`（三个 Rational：时/分/秒）拼接为 `NaiveDateTime`
- 任意字段解析失败则继续尝试下一个；全部失败返回 `None`
- 单元测试：增加 fixture 或构造场景，覆盖 DateTimeDigitized 回退和 GPS 时间解析

### 12b — 新增 `metadata/filename.rs`：文件名日期推断

暴露：
```rust
pub fn infer_date(filename: &str) -> Option<NaiveDateTime>
```

按顺序尝试以下规则（任意一条匹配即返回）：

1. **Unix 时间戳**：文件名（去除扩展名）全为数字，10 位（秒级）或 13 位（毫秒级），转换为 UTC NaiveDateTime
2. **紧凑日期时间**：在文件名中扫描 `YYYYMMDD[_-]HHMMSS` 模式（如 `IMG_20240615_103000`），允许前后有其他字符
3. **分隔符日期**：在文件名中扫描 `YYYY-MM-DD` 或 `YYYY_MM_DD`（如 `2024-06-15_photo`），时间部分可选（默认 00:00:00）
4. 以上均不匹配 → 返回 `None`

只接受合法日期（月 1–12、日 1–31），拒绝 `20241332` 等无效数值。

单元测试覆盖：
- `IMG_20240615_103000.jpg` → `2024-06-15 10:30:00`
- `2024-06-15_vacation.jpg` → `2024-06-15 00:00:00`
- `1718443800.jpg`（Unix 秒）→ 正确 UTC 时间
- `1718443800000.jpg`（Unix 毫秒）→ 正确 UTC 时间
- `DSC_0001.jpg` → `None`
- `20241332_photo.jpg`（非法日期）→ `None`

**验收**：`cargo nextest run` 全部通过；`metadata::exif` 覆盖多字段回退，`metadata::filename` 覆盖上述全部用例。

---

## Step 13 — 导入重构：移动文件到 library，按日期目录组织

**目标**：将导入行为从"只记录路径"改为"将文件物理移入 library，按拍摄日期组织目录结构"。

**背景**：当前实现只在数据库中记录源文件路径，不移动文件。新需求要求导入即整理：文件移动到 `{library_path}/{yyyy-mm-dd}/` 下，数据库记录新路径。

**目录结构变更：**
```
{library_path}/
  2024-06-15/
    IMG_20240615_103000.jpg
    DSC_0042.arw
  2024-07-01/
    photo.heic
  unknown/
    DSC_0001.jpg   ← 无法判断日期的文件
```

**实现要点：**

- `importer/placer.rs`：新模块，负责文件的物理移动/复制：
  - `place(src, library_path, date, copy_only) -> Result<PathBuf>`
    - `date` 为 `Option<NaiveDate>`：有值则放入 `yyyy-mm-dd/`，`None` 放入 `unknown/`
    - `copy_only = false`（默认）：`std::fs::rename`，跨设备时降级为复制后删除源文件
    - `copy_only = true`：`std::fs::copy`，保留源文件
    - 目标路径已存在同名文件时在文件名末尾追加 `_1`、`_2` 等后缀避免覆盖
  - 返回文件在 library 中的最终绝对路径

- **日期来源整合**（`import_one` 中）：
  1. 先尝试 EXIF `taken_at`
  2. EXIF 无结果时调用 `metadata::filename::infer_date(filename)`
  3. 两者均无则 `date = None` → 放入 `unknown/`

- **数据库**：`photos.path` 存储 library 内的新路径（而非源路径）

- **CLI 变更**：
  ```
  picmanager import [--copy] <dir>
  ```
  `--copy` 对应 clap 的 `bool` flag

- **Web API 变更**：`POST /api/import` body 增加可选字段 `"copy": true`

- **已导入文件的幂等性**：SHA-256 相同的文件继续跳过，不重复移动

- **单元测试**：
  - 文件被移动到正确的 `yyyy-mm-dd/` 子目录
  - `--copy` 时源文件保留，目标文件存在
  - 无日期文件进入 `unknown/`
  - 目标文件名冲突时自动加后缀
  - 跨设备 rename 降级为 copy+delete（用 tempdir 在不同挂载点模拟）

**验收**：`picmanager import ~/Desktop/test_photos/` 执行后，照片出现在 `{library_path}/2024-06-15/` 等目录下，源目录中对应文件消失；`picmanager import --copy ~/Desktop/test_photos/` 执行后源文件保留。

---

## Step 14 — 人脸检测与特征提取

**目标**：导入时自动检测人脸并提取特征向量，同时支持对全库或指定范围照片触发重分析，为后续人物相册奠定数据基础。

### 14a — DB Schema 扩展

新增迁移文件 `migrations/0003_faces.sql`：

```sql
-- 人脸区域：每张照片中每张脸一行
CREATE TABLE IF NOT EXISTS faces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    photo_id    INTEGER NOT NULL REFERENCES photos(id),
    x           INTEGER NOT NULL,   -- 检测框左上角 x（原图像素坐标）
    y           INTEGER NOT NULL,   -- 检测框左上角 y
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    confidence  REAL,               -- 检测置信度 0.0–1.0
    embedding   BLOB,               -- 特征向量（f32 数组，小端序），NULL = 尚未提取
    embed_model TEXT,               -- 提取模型标识，如 "arcface-mobilenet-v1"
    detected_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 人脸分析任务：跟踪批量（重）分析进度
CREATE TABLE IF NOT EXISTS face_jobs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    status      TEXT NOT NULL DEFAULT 'running',  -- running / done / failed
    scope       TEXT,               -- NULL = 全库；JSON 整数数组 = 指定 photo_id 列表
    total       INTEGER,
    processed   INTEGER NOT NULL DEFAULT 0,
    started_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT
);
```

**embedding 编码约定**：`Vec<f32>` 直接按小端序逐元素写入 BLOB，每个 f32 占 4 字节，512 维向量共 2048 字节。

**单元测试**：迁移运行后两张表均可正常 INSERT / SELECT。

---

### 14b — 人脸检测模块

**crate**：`ort 2.x`（直接使用，不经过 `rust-faces`）

`rust-faces` 内部依赖 `ort 1.x`，与 14c 所需的 `ort 2.x` 存在主版本冲突，两套 ONNX Runtime 会同时出现在依赖树中，macOS 动态库加载也会冲突。因此 14b/14c 统一使用 `ort 2.x`，检测预处理和后处理自行实现。

**模型**：ultraface-slim-320（Linzaer），输入 `[1, 3, 240, 320]` float32 BGR `(pixel-127)/128`，输出 `scores [1, 4420, 2]` + `boxes [1, 4420, 4]`（归一化 x1y1x2y2），后处理简单，模型约 1 MB。

**模型文件路径**：`{config_dir}/models/face_detector.onnx`

新建 `src/face/` 模块：

- `src/face/mod.rs`：re-export `detector`、`embedder`、`job` 子模块
- `src/face/detector.rs`：
  ```rust
  pub struct FaceRegion { pub x: i32, pub y: i32, pub width: i32, pub height: i32, pub confidence: f32 }
  pub fn detect(img: &image::DynamicImage) -> Vec<FaceRegion>
  ```
  - 预处理：`resize_exact(320, 240)`，BGR 转 `[1,3,H,W]` float32，`(px-127)/128`
  - `OnceLock<Option<Session>>` 懒加载模型；模型不存在时返回空 Vec + `tracing::warn`
  - 后处理：过滤 confidence ≥ 0.5，IoU NMS（阈值 0.45），按置信度降序
  - 任何推理失败返回空 Vec，不 panic
- 纯函数可独立测试：`iou()`、`nms()`、`preprocess()`

**单元测试**（无需模型文件）：
- `FaceRegion` 字段读写
- `iou()` 无重叠→0、完全重叠→1、半重叠→1/3
- `nms()` 保留最高置信度、抑制高重叠框、保留无重叠框
- 极小图（4×4）直接返回空

**集成测试**（`#[ignore]`，需要模型文件）：
- 已知含人脸的 JPEG 样张 → 至少 1 个 FaceRegion，confidence ≥ 0.5
- 纯白图 → 空列表

---

### 14c — 人脸特征提取模块

**crate**：`ort 2.x`（与 14b 共用同一 ONNX Runtime 实例）

**模型**：ArcFace-MobileNetV1（来自 insightface buffalo_sc），输入 112×112 RGB，输出 512D float32，模型文件约 10 MB，不打包进二进制。

**模型文件路径**：`{config_dir}/models/arcface_mobilenetv1.onnx`（`config_dir` 即 `~/Library/Application Support/picmanager/`）

`src/face/embedder.rs`：
```rust
pub struct Embedder { /* ort Session */ }
impl Embedder {
    pub fn load(model_path: &Path) -> Result<Self>
    pub fn extract(&self, img: &DynamicImage, region: &FaceRegion) -> Result<Vec<f32>>
}
```
提取流程：
1. 按 `region` 裁剪人脸区域（略微扩边 20% 以包含额头/下巴）
2. Resize 到 112×112，RGB 转 [-1.0, 1.0] 归一化
3. ort 推理，取输出张量第 0 行得到 512D 向量
4. L2 归一化后返回（保证后续余弦相似度计算等价于点积）

**模型文件不存在时**的行为：`Embedder::load` 返回 `Err(AppError::ModelNotFound)`；导入流程中 embedding 留 NULL，仅记录检测框；打印 `tracing::warn` 提示用户运行 `picmanager models fetch`。

**新增 `AppError` 变体**：`ModelNotFound(String)`（模型名称）。

**单元测试**：
- 对已检测到的 FaceRegion 调用 `extract()`，返回长度 512 的 f32 Vec，向量 L2 范数在 0.99–1.01 之间
- BLOB 序列化/反序列化后与原向量数值一致（浮点相等）

---

### 14d — 导入集成 + 批量重分析 API

**导入集成（`importer/mod.rs`）**：

在每张照片写入 `photos` 表后，调用 `face::analyze_one(pool, config, photo_id, img)`：
1. 调用 `detector::detect()` 获取 `Vec<FaceRegion>`
2. 将所有 FaceRegion 批量写入 `faces` 表（embedding = NULL）
3. 如果 `Embedder` 可加载，逐个提取 embedding 并 UPDATE 对应行
4. 任何步骤失败只 `tracing::warn!`，不中断导入

**批量重分析（`src/face/job.rs`）**：

```rust
pub async fn run_job(
    pool: &SqlitePool,
    config: &Config,
    scope: Option<Vec<i64>>,  // None = 全库
) -> Result<i64>  // 返回 face_jobs.id
```
- 在 `face_jobs` 插入运行中记录，`tokio::spawn` 异步执行
- 对每张照片：先 `DELETE FROM faces WHERE photo_id = ?`，再重新检测+提取
- 每处理一张更新 `face_jobs.processed`；全部完成后更新 `status = 'done'`、`finished_at`

**Web API（`web/handlers/faces.rs`）**：

```
POST /api/faces/analyze
    body: { "photo_ids": [1, 2, 3] }   # 省略或空数组 = 全库
    resp: { "job_id": 42 }

GET  /api/faces/jobs/:id
    resp: { "id": 42, "status": "running", "total": 1000, "processed": 312 }

GET  /api/photos/:id/faces
    resp: [{ "id": 1, "x": 120, "y": 80, "width": 200, "height": 200, "confidence": 0.97 }, ...]
    注：不返回 embedding（体积大，前端不需要）
```

**CLI（`main.rs`）**：

```
picmanager faces analyze [--photo-ids 1,2,3]   # 全库或指定照片重分析
picmanager models fetch                         # 下载模型文件到 config_dir/models/
```

`models fetch` 实现：用 `reqwest` 从固定 URL 下载 ONNX 文件，写入目标路径，打印下载进度。

**单元测试**：
- 导入含人脸的照片后 `SELECT COUNT(*) FROM faces WHERE photo_id = ?` ≥ 1
- 重分析后 faces 行数不累加（旧行已先删除）
- `GET /api/photos/:id/faces` 返回正确字段，不含 embedding 字段
- 无人脸的照片导入后 faces 表中无对应行，不报错

**验收**：导入 `tests/samples/IMG_9886.HEIC` 后，`GET /api/photos/:id/faces` 返回至少 1 条记录，bounding box 覆盖图中人脸；`POST /api/faces/analyze` 触发全库重分析，`GET /api/faces/jobs/:id` 显示进度直至 `done`。
