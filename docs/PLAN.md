# 开发计划

> **状态**：Steps 1–38 全部已完成（2026-05，详见 CLAUDE.md）。Steps 39–41 为 2.0 运动记录功能。

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

---

## Step 15a — 缩略图磁盘缓存 + spawn_blocking

**目标**：翻页速度从 500–700 ms 降至 <50 ms（重复访问），同时不再阻塞 Tokio async runtime。

**背景**：当前 `generate_thumb()` 是同步函数，每次 `GET /api/photos/:id/thumb` 都从原始文件重新解码，且直接在 async worker 线程中调用，阻塞其他任务。详见 `docs/PERFORMANCE.md` §1。

**实现**：

缓存目录：`{library_path}/.thumbs/{photo_id}.jpg`

```
GET /api/photos/:id/thumb 处理逻辑：
1. cache_path = library_path/.thumbs/{id}.jpg
2. 若 cache_path 存在 → 直接 return fs::read(cache_path)（仍用 spawn_blocking）
3. 否则：spawn_blocking { 读原文件 → 解码 → resize → 编码 → 写 cache_path → 返回字节 }
```

- `generate_thumb()` 改为返回 `Vec<u8>`，接受 `output_path: &Path` 参数，解码完成后写缓存文件
- `AppState` 增加 `thumb_cache_dir: PathBuf`（指向 `{library_path}/.thumbs/`，启动时创建目录）
- 整个缩略图操作（含文件读写）全部包在 `tokio::task::spawn_blocking` 中

**数据结构变更**：无 DB 改动。`.thumbs/` 目录加入 `.gitignore`。

**单元测试**：
- 首次请求生成缓存文件，响应 200 + `Content-Type: image/jpeg`
- 第二次请求命中缓存（可 mock `generate_thumb` 计数器验证只调用一次）
- 缓存文件不存在时降级到实时生成（不崩溃）

**验收**：50 张图翻页，Network 面板总耗时从 500+ ms → 首次 ~500 ms（冷）、再次 <50 ms（热）。

---

## Step 15b — COUNT(*) 替换为计数器表

**目标**：每次分页查询节省 3–50 ms（百万行时明显），避免全表扫描。

**背景**：当前 `GET /api/photos` 每次执行 `SELECT COUNT(*) FROM photos WHERE import_status='active'`，O(n) 全表扫描。详见 `docs/PERFORMANCE.md` §2a。

**实现**：

新增迁移 `migrations/0004_photo_stats.sql`：

```sql
CREATE TABLE photo_stats (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    active_count INTEGER NOT NULL DEFAULT 0
);
INSERT INTO photo_stats (id, active_count) SELECT 1, COUNT(*) FROM photos WHERE import_status = 'active';
```

维护点：
- 导入成功：`UPDATE photo_stats SET active_count = active_count + 1 WHERE id = 1`
- 软删除（dedup resolve）：`UPDATE photo_stats SET active_count = active_count - 1 WHERE id = 1`
- `GET /api/photos` 改为 `SELECT active_count FROM photo_stats WHERE id = 1`

**单元测试**：
- 导入 3 张照片后 `active_count = 3`
- resolve dedup 软删除 1 张后 `active_count = 2`
- `GET /api/photos` 返回的 `total` 与 `active_count` 一致

**验收**：10 万张照片时，`GET /api/photos?page=1` 响应时间 <5 ms（之前约 15 ms）。

---

## Step 16a — dedup 增量扫描

**目标**：常规使用中（每次导入后运行一次 dedup）从小时级降至秒级。

**背景**：当前 `scan()` 对全库所有照片做 O(n²) pHash 比较。导入 1,000 张新照片到已有 10,000 张的库时，只需比较新照片与已有照片（10,000 × 1,000 = 1,000 万次），而不是全库 (11,000)² / 2 ≈ 6,000 万次。详见 `docs/PERFORMANCE.md` §3。

**实现**：

`photos` 表新增字段 `dedup_scanned_at TIMESTAMP`（迁移 `0005_dedup_incremental.sql`）：

```sql
ALTER TABLE photos ADD COLUMN dedup_scanned_at TIMESTAMP;
```

`scan(pool)` 改为增量模式：
1. 读取所有 `dedup_scanned_at IS NULL` 的照片（新照片）
2. 若无新照片 → 直接返回（0 次比较）
3. 读取所有已有照片的 pHash（`dedup_scanned_at IS NOT NULL`）
4. 仅对「新 × 已有」+「新 × 新」做 pHash 比较（不重复对已扫描照片两两比较）
5. 写入 `dedup_groups` / `dedup_members`（逻辑不变）
6. 将本批新照片的 `dedup_scanned_at` 更新为 `NOW()`

CLI `picmanager dedup --full` 参数：重置所有 `dedup_scanned_at = NULL` 并全量重扫。

**单元测试**：
- 导入 2 张相似照片，首次 scan → 生成 1 个 dedup group，两张照片 `dedup_scanned_at` 已设置
- 再次 scan（无新照片）→ 不产生新 group，不做任何比较
- 再导入 1 张与第 1 张相似的照片，scan → 将新照片加入已有 group（或新建 group）

**验收**：10,000 张已扫描 + 100 张新导入，`picmanager dedup` 耗时 <5 s（之前约 2 小时）。

---

## Step 16b — dedup pHash 前缀分桶

**目标**：`picmanager dedup --full` 全库重扫从 O(n²) 降至接近 O(n log n)。

**背景**：pHash 为 64 位整数，汉明距离 ≤ 10 的两张照片在前 4 位（高位 nibble）上大概率相同或相邻。按前缀分桶后，只需在同桶或相邻桶内比较。详见 `docs/PERFORMANCE.md` §3。

**实现**：

分桶策略：按 pHash 高 8 位（`phash >> 56`，取值 0–255）分为 256 个桶。汉明距离 ≤ 10 的两张照片中，高 8 位最多差 8 位，因此只需比较当前桶 ± 邻桶（按高 8 位汉明距离 ≤ 8 筛选邻桶，实际约 70 个桶需比较）。

`scan_full(pool)` 算法：
```
1. 从 DB 读取全部 (id, phash) — O(n)
2. 按 phash >> 56 分桶，256 个 HashMap<u8, Vec<(id, phash)>>
3. 对每个桶 b，枚举桶 b 本身 + hamming_distance(b, b') ≤ 8 的所有邻桶 b'
4. 在选出的照片子集内做 O(k²) 比较（k = 桶平均大小 ≈ n/256）
5. 写入 dedup groups（同 Step 16a）
```

理论复杂度：O(n × 70 × k) ≈ O(n × n/256 × 70) ≈ O(n² / 3.6) — 分桶使比较次数降至约 1/3.6；配合 16a 增量扫描，全量重算场景大幅改善。

**单元测试**：
- 1,000 张随机 pHash，`scan_full` 结果与暴力 O(n²) 比较结果一致（无漏报）
- 两张汉明距离恰好为 10 的照片（高 8 位差 8 位）：在分桶后仍被发现
- 两张汉明距离为 11 的照片：不被报告为候选

**验收**：50,000 张照片全库重扫 `--full`，耗时 <5 分钟（之前估算数天）。

---

## Step 17 — 照片详情视图与时间编辑

**目标**：提供照片详情页（大图 + 完整元信息 + 人脸位置标注），并支持单张及批量修改拍摄时间和时区（仅写数据库）。

---

### 17a — DB: photos.timezone_offset 字段

新增迁移 `migrations/0006_timezone_offset.sql`：

```sql
ALTER TABLE photos ADD COLUMN timezone_offset INTEGER;  -- UTC 偏移分钟数，NULL = 未知
```

- `timezone_offset` 存储 UTC 偏移（分钟），如 `+480`（东八区）、`-300`（EDT）
- 后续 API 响应中 `taken_at` 与 `timezone_offset` 一并返回

**单元测试**：迁移后 INSERT 含 `timezone_offset` 的记录并 SELECT 验证字段可读写。

**验收**：`cargo nextest run` 全部通过，`photos` 表存在 `timezone_offset` 列。

---

### 17b — API: 照片元信息编辑接口

在 `web/handlers/photos.rs` 新增两个端点：

```
PATCH /api/photos/:id
    body: { "taken_at": "2024-06-15T10:30:00", "timezone_offset": 480 }
    resp: 200 OK 或 404

POST /api/photos/batch-update
    body: { "photo_ids": [1, 2, 3], "taken_at": "...", "timezone_offset": 480 }
    resp: { "updated": 3 }
```

- 字段均可选，只更新请求体中出现的字段（`taken_at` / `timezone_offset`）
- `taken_at` 格式：ISO 8601 `YYYY-MM-DDTHH:MM:SS`
- 不存在的 photo_id 跳过（batch）或返回 404（单张）

**单元测试**（`tower::ServiceExt::oneshot`）：
- PATCH 单张 → DB 中字段已变更
- POST 批量 → 所有指定 ID 均更新，返回 `updated = N`
- PATCH 不存在的 ID → 404

**验收**：`curl -X PATCH /api/photos/1 -d '{"timezone_offset":480}'` 后查 DB，值已更新。

---

### 17c — 前端: 照片详情模态框

点击缩略图后弹出详情模态框（`frontend/` 纯 JS 实现）：

- 展示大图（原图或最大缩略图）
- 元信息面板：`taken_at`（含 `timezone_offset` 换算展示）、相机型号、GPS 坐标、图片格式
- 人脸标注：SVG overlay 叠加在图片上，数据来自 `GET /api/photos/:id/faces`，绘制 bounding box
- 键盘导航：← / → 切换上下张，Esc 关闭

**验收**：点击网格中任意照片，模态框弹出，元信息正确，有人脸的照片显示标注框。

---

### 17d — 前端: 时间/时区编辑 UI

在详情模态框与照片网格中分别实现编辑入口：

- **详情模态框**：点击时间字段进入编辑模式，显示日期时间输入框和时区偏移选择器，保存后调用 `PATCH /api/photos/:id`，关闭后即时刷新显示值
- **照片网格批量操作**：缩略图左上角出现勾选框（hover 或点击时），顶部浮出操作栏：
  - "调整时间"按钮 → 弹出对话框，支持"相对偏移（+N 小时）"或"设置绝对时间"，提交后调用 `POST /api/photos/batch-update`

**验收**：详情页修改时间后刷新显示值；批量选中 3 张照片调整时区，DB 中三张 `timezone_offset` 均已变更。

---

## Step 18 — 人物聚类与管理

**目标**：基于已有 ArcFace embedding 自动聚类人物，提供人物管理视图，支持合并聚类与树状子相册组织。

---

### 18a — DB Schema: people + 树状结构

新增迁移 `migrations/0007_people.sql`：

```sql
CREATE TABLE IF NOT EXISTS people (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT,                             -- 用户自定义名称，NULL = 未命名
    parent_id     INTEGER REFERENCES people(id),   -- NULL = 顶级人物；非 NULL = 子节点
    cover_face_id INTEGER REFERENCES faces(id),    -- 代表性人脸；NULL 时前端取第一张
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS person_faces (
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    face_id   INTEGER NOT NULL REFERENCES faces(id)  ON DELETE CASCADE,
    PRIMARY KEY (person_id, face_id)
);
```

- `parent_id` 支持任意深度树状结构；循环引用由应用层防止
- 一张人脸同时刻只属于一个 person（`person_faces` 主键约束保证）

**单元测试**：多层嵌套 people 的 INSERT / SELECT；`parent_id` 外键完整性；`person_faces` 重复插入被拒绝。

---

### 18b — 人物聚类算法（DBSCAN）

新建 `src/face/cluster.rs`：

```rust
/// faces: (face_id, embedding)；返回各聚类含有的 face_id 列表，噪点各自单独成组
pub fn cluster_faces(faces: &[(i64, Vec<f32>)], eps: f32, min_samples: usize) -> Vec<Vec<i64>>
```

- 距离度量：`1.0 - dot(a, b)`（L2 归一化后余弦距离等价于点积）
- 默认参数：`eps = 0.4`，`min_samples = 2`
- 噪点（未归入任何核心点）各自单独成组，便于用户后续手动合并
- 不引入新依赖，手动实现 DBSCAN（约 80 行）

暴露 `face::cluster::run_clustering(pool) -> Result<usize>`：
1. 读取所有 `embedding IS NOT NULL` 的 faces
2. 运行 DBSCAN，得到聚类分组
3. 清空 `people` / `person_faces`，重新写入
4. 返回生成的人物数量

**单元测试**：
- 3 组明显分离的 embedding → 3 个聚类
- 2 个极近点 + 1 个远点 → 2 组（1 聚类 + 1 噪点）
- 空输入 → 空输出

---

### 18c — 人物管理 API + 人脸缩略图接口

新建 `web/handlers/people.rs`，注册路由：

```
GET  /api/people
     resp: [{ "id", "name", "parent_id", "face_count", "photo_count", "cover_face_id" }, ...]

GET  /api/people/tree
     resp: 嵌套 JSON 树（id, name, children: [...]）

GET  /api/people/:id/photos
     resp: 分页照片列表（通过 person_faces → faces.photo_id 关联）

POST /api/people/cluster
     resp: { "job_id": ... }  — 异步重聚类，写法同 face_jobs

POST /api/people/merge
     body: { "source_id": 2, "target_id": 1 }
     — 将 source 的所有 person_faces 并入 target，删除 source

POST /api/people/:id/reparent
     body: { "new_parent_id": 3 }  — null = 提升为顶级

GET  /api/faces/:id/thumb
     — 按 faces 表中 bbox 裁剪原图，返回 JPEG
     — spawn_blocking，磁盘缓存至 .thumbs/face_{id}.jpg
```

**单元测试**：
- merge 后 source 不存在，target `face_count` 为两者之和
- reparent 后 `parent_id` 已更新
- `/api/faces/:id/thumb` 返回 200 + `Content-Type: image/jpeg`

---

### 18d — 前端: 人物列表视图

导航栏新增"人物"标签页：

- 人物网格：每张卡片显示代表性人脸裁剪图（`GET /api/faces/:id/thumb`）+ 姓名（可点击编辑）+ 照片数
- 顶部"重新聚类"按钮 → `POST /api/people/cluster`，轮询进度条直至完成后刷新
- 支持层级浏览：顶级人物展示时，有子节点的人物显示展开箭头
- 点击卡片进入人物详情页

---

### 18e — 前端: 人物详情页 + 合并/树状管理

人物详情页（路由 `#/people/:id`）：

- 上方：代表性头像 + 姓名输入框（失焦自动保存）+ "合并到…" 按钮
- 下方：该人物的照片网格（分页）
- "合并到…" 对话框：搜索选择目标人物，确认后调用 `POST /api/people/merge`，成功后跳转到目标人物页
- 子人物面板（侧边栏）：展示直接子节点列表，提供"移出"操作（调用 `POST /api/people/:id/reparent` 将 `new_parent_id` 设为 null）

---

## Step 19 — 地理位置层级视图

**目标**：将现有地点相册升级为可钻取的行政层级视图，并提供可选地图打点视图。

---

### 19a — geocache 行政层级扩展 + API

新增迁移 `migrations/0008_geocache_hierarchy.sql`：

```sql
ALTER TABLE geocache ADD COLUMN country TEXT;
ALTER TABLE geocache ADD COLUMN state   TEXT;  -- 州/省
ALTER TABLE geocache ADD COLUMN county  TEXT;  -- 县/区（对应 Nominatim address.county）
-- 原有 city 字段保留，作为最精确地名
```

更新 `album/location.rs`：
- `reverse_geocode()` 解析 Nominatim JSON 中 `address.country`、`address.state`、`address.county`、`address.city`（回退 `town` / `village`），写入 geocache
- 对已缓存但新字段为 NULL 的行，后台补充更新一次（限速 1 req/s）

新增 API 端点 `GET /api/geo/hierarchy`，返回嵌套层级与各级照片数：

```json
{ "countries": [
    { "name": "United States", "photo_count": 1234,
      "states": [
        { "name": "California", "photo_count": 800,
          "cities": [{ "name": "San Francisco", "photo_count": 200 }] }
      ]}
  ]}
```

**单元测试**：
- 含 GPS 照片导入后 geocache `country` / `state` / `city` 均不为 NULL
- `/api/geo/hierarchy` 返回正确的层级嵌套和计数

---

### 19b — 前端: 地理层级列表视图

导航栏新增"地点"标签页，默认展示层级列表：

- 三列钻取面板（Country → State → City），点击左列条目展开右列子项
- 每项显示地名 + 照片数
- 选中叶节点后右侧展示该城市的照片网格
- 面包屑导航显示当前路径（如 "USA > California > San Francisco"）

---

### 19c — 前端: 地图打点视图

"地点"标签页顶部增加"列表 / 地图"切换：

- 地图视图使用 Leaflet.js（CDN 加载），底图 OpenStreetMap（免费、无 API key）
- 数据来源：`GET /api/photos?has_gps=true&fields=id,taken_at,gps_lat,gps_lon`
- 100 张以上时使用 Leaflet.markercluster 聚合，缩小时合并为计数气泡
- 点击单个 marker 弹出 popover 显示缩略图 + 拍摄时间

新增 API 查询参数：`GET /api/photos` 支持 `has_gps=true`（过滤）和 `fields=...`（仅返回指定字段，减少传输量）。

---

## Step 20 — 动物检测

**目标**：导入时用 YOLOv8-nano 识别照片中的动物种类，在动物视图中按种类浏览。

---

### 20a — DB Schema + YOLOv8-nano 集成

新增迁移 `migrations/0009_animals.sql`：

```sql
CREATE TABLE IF NOT EXISTS animals (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    photo_id    INTEGER NOT NULL REFERENCES photos(id),
    species     TEXT    NOT NULL,   -- COCO 类名，如 "cat", "dog", "bird"
    confidence  REAL    NOT NULL,
    x           INTEGER NOT NULL,
    y           INTEGER NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    detected_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
```

新建 `src/animal/` 模块（`detector.rs` + `mod.rs`）：

- 模型：YOLOv8-nano ONNX（`{config_dir}/models/yolov8n.onnx`，约 6 MB）
- 只关注 COCO 动物类（class 14–23：bird, cat, dog, horse, sheep, cow, elephant, bear, zebra, giraffe）
- 输入 `[1, 3, 640, 640]` float32 归一化 [0,1]，输出 `[1, 84, 8400]`，解析 bbox + class scores，NMS（IoU ≥ 0.45），过滤 confidence ≥ 0.4
- 导入流程（`importer/mod.rs`）中，在人脸检测之后调用 `animal::detect_and_save(pool, photo_id, img)`
- 模型不存在时跳过并 `tracing::warn!`，不中断导入
- `picmanager models fetch` 补充下载 `yolov8n.onnx`

**单元测试**：
- NMS、预处理纯函数（不需要模型）
- 含猫的照片检测到 `species = "cat"`（`#[ignore]`，需模型文件）

---

### 20b — 动物 API + 前端视图

新增 API（`web/handlers/animals.rs`）：

```
GET /api/animals/species
    resp: [{ "species": "cat", "chinese": "猫", "photo_count": 42 }, ...]

GET /api/animals/:species/photos
    resp: 分页照片列表（含该动物种类的照片）

GET /api/photos/:id/animals
    resp: [{ "id", "species", "confidence", "x", "y", "width", "height" }, ...]
```

前端（导航栏新增"动物"标签页）：

- 种类卡片网格：动物图标（emoji 或 SVG）+ 中文种名 + 照片数
- 点击进入该种类的照片网格（复用已有网格组件）
- 在照片详情模态框中，有动物检测结果时展示 bounding box overlay（与人脸标注并列）

---

## Step 21 — CLI 元数据补全命令（fill-missing）

**目标**：提供命令行一键补全全库缺失的人脸和地理元数据，支持每分钟进度打印与完成汇总，方便脚本化或首次模型下载后使用。

### 设计

```
picmanager fill-missing [--faces] [--geo]
```

- 不带标志时同时补充人脸和地理
- `--faces`：仅补充从未分析人脸的照片（`faces` 表无记录）
- `--geo`：仅对有 GPS 坐标但 `geocache` 表无对应条目的照片触发 Nominatim 反地理编码

### 实现步骤（TDD）

**1. 新增库函数（先写测试，再实现）**

在 `src/face/job.rs` 新增：

```rust
/// 返回所有已导入但从未进行人脸分析的照片 ID。
pub async fn scope_for_missing(pool: &SqlitePool) -> Result<Vec<i64>>
```

**测试用例**：
- 3 张照片，仅 1 张有 faces 记录 → 返回另外 2 张 ID
- 已删除照片（`import_status='deleted'`）不在结果中

在 `src/album/location.rs` 新增：

```rust
/// 返回有 GPS 但 geocache 中无对应条目的已导入照片数量。
pub async fn count_missing_geo(pool: &SqlitePool) -> Result<i64>
```

**测试用例**：
- 无 GPS 照片 → 返回 0
- 3 张 GPS 照片，1 张已缓存 → 返回 2
- 所有 GPS 照片已缓存 → 返回 0

**2. CLI 命令（`src/main.rs`）**

新增 `Command::FillMissing { faces: bool, geo: bool }`：

```
// Phase 1 — 统计待处理数量，打印提示
// Phase 2 — 启动任务
//   人脸：用 scope_for_missing 查到 ID，调用 face::job::run_job(Some(ids))
//   地理：tokio::spawn(album::group_by_location(pool))
// Phase 3 — 轮询循环（每 5s 检查，每 60s 或完成时打印进度）
//   人脸进度：查 face_jobs.processed / total
//   地理进度：count_missing_geo() 变化量
// Phase 4 — 完成汇总
//   人脸：分析张数、新增人脸数
//   地理：编码成功数、失败数（无城市信息）
```

**进度输出格式**：
```
开始补全缺失元数据…
  待补充人脸分析：75 张
  待补充地理编码：23 张

[00:01:00] 人脸：12/75 (16%) ｜ 地理：3/23 (13%)
[00:02:00] 人脸：36/75 (48%) ｜ 地理：15/23 (65%)
[00:03:45] 人脸：75/75 (100%) ｜ 地理：20/23 (87%)

补全完成（耗时 3 分 45 秒）：
  人脸：分析了 75 张照片，新增 203 个人脸记录
  地理：编码了 20 个新位置，3 张无城市信息（已跳过）
```

### 验收

- `cargo nextest run` 全部通过（含新增的 `scope_for_missing` 和 `count_missing_geo` 测试）
- `picmanager fill-missing` 在空库上立即输出"无需补全，退出"
- 在含未分析照片的库上运行，每分钟打印进度，结束时有汇总

---

## Step 22 — 人物页编辑增强

**目标**：在已有人物列表和详情页基础上，补全单人物编辑、多选批量操作、操作撤销、树状结构深度编辑、以及重复姓名保护，形成完整的人物管理工作流。

---

### 22a — DB Schema 扩展 + 后端 API 补全

**目标**：在数据库中记录人物状态，并补齐前端所需的全部 REST 接口。

**DB 迁移**（`migrations/0010_people_status.sql`）：

```sql
ALTER TABLE people ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'ignored', 'not_a_person'));
```

**新增 / 变更接口**（`web/handlers/people.rs`）：

```
PATCH /api/people/:id
    body: { "name": "张三", "status": "ignored" }   -- 字段均可选，只更新出现的字段
    resp: 200 OK | 404

POST /api/people/batch-update
    body: { "ids": [1,2,3], "status": "not_a_person" }
    resp: { "updated": 3 }

GET  /api/people
    新增可选查询参数 status=active|ignored|not_a_person|all
    默认只返回 status='active' 的人物（保持向后兼容）

GET  /api/people?name_exact=张三
    精确名称查找，返回同名人物列表（含 cover_face_id），供重复检测使用
```

**单元测试**：
- `PATCH /api/people/:id` 更新 `name` 后 DB 字段已变更
- `PATCH /api/people/:id` 更新 `status` 为 `ignored` 后 `GET /api/people`（默认）不再返回该人物
- `POST /api/people/batch-update` 批量改状态，返回 `updated = N`
- `GET /api/people?status=all` 返回全部人物（含 ignored / not_a_person）
- `GET /api/people?name_exact=张三` 返回同名列表

**验收**：`cargo nextest run` 全部通过；DB 中 `people` 表含 `status` 列。

---

### 22b — 前端：内联改名 + 单人物操作菜单

**目标**：在人物网格的每张卡片上，实现点击改名和"…"扩展菜单。

**人物卡片改动（`frontend/app.js`）**：

- 姓名区域改为可点击：点击后替换为 `<input>` 文本框，失焦或回车调用 `PATCH /api/people/:id`（name 字段）后还原为文字
- 卡片右上角增加"…"按钮，点击弹出浮层菜单：
  - 忽略此人 → `PATCH /api/people/:id { status: 'ignored' }`
  - 标记为非人物 → `PATCH /api/people/:id { status: 'not_a_person' }`
  - 确认对话框：`"确定要忽略/标记？此操作可撤销。"`

**人物列表刷新**：操作成功后从列表中移除该卡片（因为默认只显示 active）。

**验收**：点击人物名称进入编辑态，输入后保存显示新名称；"…" 菜单忽略某人后该卡片从列表消失；`?status=ignored` 筛选可见被忽略的人物。

---

### 22c — 前端：多选 + 批量操作

**目标**：支持在人物网格中框选多个聚类，统一命名合并或批量改状态。

**多选模式**：

- 每张人物卡片 hover 时显示勾选框；点击勾选框进入多选模式，其余卡片也显示勾选框
- 顶部浮出批量操作栏，显示已选人数，提供：
  1. **命名并合并**：输入姓名，从已选中的聚类里选定"主体"（默认照片数最多的一个），其余并入主体；可选择设置合并后的父节点（搜索框），留空为顶级；调用 `POST /api/people/merge`（多次，把其他 source 依次并入 target）+ `PATCH /api/people/:id { name: ... }`
  2. **批量忽略** → `POST /api/people/batch-update { ids, status: 'ignored' }`
  3. **批量标记非人物** → `POST /api/people/batch-update { ids, status: 'not_a_person' }`
  4. **取消选择**

**操作完成**后清空选中状态，刷新人物列表。

**验收**：选中 3 个聚类，命名合并后人物列表只剩 1 张该人的卡片，名称正确；批量忽略后所有选中卡片消失。

---

### 22d — 前端：操作撤销

**目标**：为所有人物编辑提供"撤销"按钮，防止误操作。

**设计**：

撤销完全在前端实现，维护一个操作历史栈（数组），每次操作成功后压入一条撤销记录：

| 原操作 | 撤销操作 |
|--------|----------|
| PATCH name | PATCH 回旧 name |
| PATCH status → ignored/not_a_person | PATCH status → active |
| 批量 status 变更 | 批量 status 变回原值（需在操作前记录每条旧状态）|
| merge A+B→B | reparent 已删除的 A 无法恢复 → 特殊处理：改为"撤销合并"需调用 `POST /api/people/cluster` 重新聚类，风险较高，改为**仅在确认对话框提示"合并不可撤销"**，不进入撤销栈 |
| reparent | reparent 回旧 parent_id |

页面右上角显示"撤销"按钮（有历史时高亮），点击执行栈顶逆操作，并从栈中弹出。页面刷新后历史栈清空。

**注意**：合并操作（人脸从 source 并入 target）在数据库层面不可精确逆转，故在确认时单独提示"合并操作无法撤销，确认？"，不加入撤销栈。其余所有操作均可撤销。

**验收**：改名后点击"撤销"恢复旧名；忽略一个人物后点击"撤销"该人物重新出现；合并时出现不可撤销提示。

---

### 22e — 前端：人物详情页树状结构编辑

**目标**：点击人物缩略图进入详情页，支持在页面内直接调整树状结构和移出操作。

**详情页改动（路由 `#/people/:id`，已有骨架，本步深化）**：

- **设置父节点**：详情页顶部显示当前所在路径（`顶级 > 父节点名` 面包屑）；点击"更改父节点"弹出搜索框，输入人名后从 `GET /api/people` 中实时过滤，选定后调用 `POST /api/people/:id/reparent { new_parent_id }`；选"顶级"则传 `null`
- **子节点面板**（侧边栏或折叠区）：列出该人物的所有直接子节点（姓名 + 代表性头像），每项提供：
  - "移至顶级" → `POST /api/people/:id/reparent { new_parent_id: null }`
  - "移至其他人物" → 同"设置父节点"弹窗，但操作对象为子节点
- **操作均进入撤销栈**（同 22d）

**验收**：进入人物详情后，面包屑显示正确层级；更改父节点后面包屑更新；子节点"移至顶级"后该子节点在父节点面板消失，刷新人物列表后出现在顶级。

---

### 22f — 重复姓名检测

**目标**：命名或改名时，若与已有人物同名，弹对话框展示双方缩略图，由用户决定是否合并。

**触发场景**：
1. 内联改名（22b）失焦/回车时
2. 多选命名合并输入名称时（22c）

**流程**：

1. 即将保存名称时，先调用 `GET /api/people?name_exact=<名称>` 查重
2. 若返回空列表 → 直接保存，流程结束
3. 若返回一个或多个同名人物 → 弹出确认对话框：
   - 对话框标题："已存在同名人物"
   - 并排展示：当前待命名人物的代表性人脸缩略图（`GET /api/faces/:id/thumb`） + 已有同名人物的缩略图（多个时逐行展示）
   - 每行操作按钮："是同一人（合并）" → `POST /api/people/merge { source: 当前, target: 已有 }` + 合并不可撤销提示；"不同人（保留重名）" → 直接保存名称
4. 用户关闭对话框（不选）等同于"不同人（保留重名）"

**单元测试**（`GET /api/people?name_exact=`）：
- 精确匹配返回同名人物列表
- 大小写严格匹配（张三 ≠ 张 三，前后无空格处理由前端 `trim()` 保证）
- 空名称时不触发查重（前端校验）

**验收**：将已有"张三"聚类再命名一个聚类为"张三"，弹出对话框并展示双方缩略图；选"是同一人"后两者合并；选"不同人"后人物列表中出现两个"张三"。

---

## Step 39 — PhotoBridge 项目脚手架

**目标**：在仓库内建立独立 Swift Package，能编译并通过 PhotoKit 权限检查。

### 39a — Swift Package 骨架

在 `photobridge/` 下初始化 Swift Package Manager 项目：

```
photobridge/
  Package.swift
  Sources/
    PhotoBridge/
      main.swift         CLI 入口
      Commands/          各子命令占位文件
      Core/              业务逻辑占位
  Tests/
    PhotoBridgeTests/
```

`Package.swift` 配置：
- Swift tools version 6.0，platform `.macOS(.v26)`
- 依赖：`swift-argument-parser`（Apple 官方 CLI 框架）
- Target：executable `PhotoBridge` + testTarget `PhotoBridgeTests`

`main.swift` 使用 `ArgumentParser` 定义根命令及三个子命令占位：`export`、`sync`、`status`，暂时打印 "Not yet implemented"。

**单元测试**：`PhotoBridgeTests` 空测试，确保 `swift test` 通过。

**验收**：`swift build` 无报错；`photobridge --help` 显示三个子命令；`swift test` 通过。

---

### 39b — Entitlements 与权限配置

配置照片库访问权限（macOS App Sandbox / Hardened Runtime）：

- `photobridge.entitlements`：声明 `com.apple.security.personal-information.photos-library`
- `Info.plist`（或 `PhotoBridge-Info.plist`）：`NSPhotoLibraryUsageDescription` 写入说明文字
- 代码层：`PhotoLibraryAuth.swift`，封装 `PHPhotoLibrary.requestAuthorization(for:)` 异步授权流程，返回 `Result<Void, AuthError>`；授权被拒绝时打印友好提示并以非零退出码退出

**单元测试**：mock `PHAuthorizationStatus` 各状态（notDetermined / authorized / denied / restricted / limited），验证 `AuthError` 枚举覆盖全部分支。

**验收**：首次运行 `photobridge status` 时，macOS 弹出照片库访问授权对话框；授权后再运行无对话框。

---

## Step 40 — 照片资产枚举与文件导出引擎

**目标**：能从 Photos 库枚举全部符合条件的照片资产，并将文件写出到指定目录。

### 40a — 资产枚举与资源类型过滤

新建 `Core/AssetEnumerator.swift`：

```swift
struct AssetEnumerator {
    /// 返回 Photos 库中所有需要导出的 (PHAsset, PHAssetResource) 对
    func enumerate() -> [(PHAsset, PHAssetResource)]
}
```

过滤规则（按优先级）：

1. `PHAsset.mediaType == .image`（跳过视频）
2. 每个 PHAsset 调用 `PHAssetResource.assetResources(for:)` 取资源列表，按以下逻辑选取**唯一一个**导出资源：
   - 若存在 `.photo` 类型资源（JPEG 或 HEIC），取 `.photo`；跳过 `.alternatePhoto`（RAW）和 `.pairedVideo`（MOV）
   - 若只有 `.alternatePhoto`（纯 RAW 资产，无 JPEG），**跳过整个资产**并记录日志
3. 连拍资产（`asset.representsBurst == true`）：只保留 `burstSelectionTypes` 包含 `.userPick` 的资产；若无精选张，跳过整个连拍组

**单元测试**：
- 模拟各类资源列表（JPEG-only / HEIC-only / RAW+JPEG / Live Photo / 纯RAW），验证选取结果
- 模拟连拍组（含 userPick / 不含 userPick），验证过滤行为

**验收**：在测试照片库上运行枚举，输出资产数量，与 Photos.app 中显示的图片数一致（不含视频）。

---

### 40b — PHAssetResourceManager 文件写出

新建 `Core/AssetExporter.swift`：

```swift
struct AssetExporter {
    /// 将单个资产资源写出到 destURL，支持网络访问（下载云端资产）
    func export(resource: PHAssetResource, to destURL: URL) async throws
    
    /// 批量导出，报告进度
    func exportBatch(
        assets: [(PHAsset, PHAssetResource)],
        stagingDir: URL,
        progress: @escaping (Int, Int) -> Void
    ) async throws -> [ExportedAsset]
}

struct ExportedAsset {
    let localIdentifier: String   // PHAsset.localIdentifier
    let fileURL: URL              // 写出路径
    let takenAt: Date?
}
```

实现细节：
- `PHAssetResourceRequestOptions.isNetworkAccessAllowed = true`（允许从 iCloud 下载）
- 使用 `PHAssetResourceManager.default().writeData(for:toFile:options:completionHandler:)` 写出到 `stagingDir/<localIdentifier>.<ext>`，使用 localIdentifier 而非原始文件名，避免重名冲突
- 导出文件的扩展名从 `PHAssetResource.uniformTypeIdentifier` 推断（`public.jpeg` → `.jpg`，`public.heic` → `.heic`）
- 写出时并发数上限 `--max-concurrent-downloads`（默认 4），避免同时触发大量 iCloud 下载

**单元测试**：使用 stub PHAssetResource 验证 URL 构造逻辑和扩展名推断。

**验收**：`photobridge export --output ~/staging --dry-run` 打印待导出资产数量；实际导出后 staging 目录中出现文件，每个文件可被 `image` 工具打开。

---

### 40c — export 子命令接线

将 40a/40b 接入 `Commands/ExportCommand.swift`：

```
photobridge export
    [--output <dir>]          暂存目录（默认 ~/Library/Application Support/PhotoBridge/staging）
    [--batch-size <n>]        每批数量（默认 200）
    [--max-concurrent <n>]    并发下载数（默认 4）
    [--dry-run]               只统计，不写文件
    [--picmanager <path>]     picmanager 可执行文件路径（默认 PATH 搜索）
```

全量导出：枚举全库 → 按 batch-size 分批写出文件 → 每批调用 picmanager（Step 42 实现，本步先跳过调用，只写文件） → 打印进度。

**验收**：`photobridge export --dry-run` 打印总数；`photobridge export` 在 staging 目录写出文件。

---

## Step 41 — 增量同步状态管理

**目标**：持久化 `PHPersistentChangeToken`，实现只处理上次同步以来新增照片的增量模式。

### 41a — 状态文件模型

新建 `Core/SyncState.swift`：

```swift
struct SyncState: Codable {
    var lastChangeTokenData: Data?          // PHPersistentChangeToken 序列化数据
    var exportedIdentifiers: Set<String>    // 已成功入库的 localIdentifier
    var lastSyncDate: Date?
    var lastSyncCount: Int
}
```

- 持久化路径：`~/Library/Application Support/PhotoBridge/state.json`
- 线程安全：文件读写在主 actor 序列化
- 提供 `SyncState.load() throws -> SyncState` 和 `save()` 方法

**单元测试**：
- 序列化 → 反序列化 roundtrip 验证字段完整性
- 文件不存在时 `load()` 返回空状态（不抛出）

---

### 41b — PHPersistentChangeFetchRequest 增量枚举

新建 `Core/IncrementalEnumerator.swift`：

```swift
struct IncrementalEnumerator {
    /// 返回上次 changeToken 以来新增的 PHAsset，并更新 token
    func fetchChanges(since token: PHPersistentChangeToken?) async throws
        -> (assets: [PHAsset], newToken: PHPersistentChangeToken)
}
```

实现细节：
- 若 `token == nil`（首次），调用 `PHPhotoLibrary.shared().currentChangeToken` 仅获取当前令牌，不枚举资产（首次应使用全量 `AssetEnumerator`）
- 若 `token != nil`，使用 `PHPersistentChangeFetchRequest(changeToken:)` 获取变更，过滤出 `insertions`（新增资产），对 `PHAssetChangeDetail.objectLocalIdentifiers` 批量 fetch PHAsset
- 将新 token 回写到 `SyncState` 后再返回，确保令牌不丢失

**单元测试**：mock `PHPersistentChangeFetchResult` 验证新增/删除/修改分类逻辑。

---

### 41c — sync 子命令接线

将 41a/41b 接入 `Commands/SyncCommand.swift`：

```
photobridge sync
    [--output <dir>]
    [--batch-size <n>]
    [--max-concurrent <n>]
    [--picmanager <path>]
```

流程：
1. 加载 `SyncState`
2. 若无历史令牌 → 提示用户先运行 `photobridge export`，退出
3. 用 `IncrementalEnumerator.fetchChanges(since:)` 获取新增资产
4. 跳过已在 `exportedIdentifiers` 中的资产（防御性去重）
5. 走与 `export` 相同的 40b/40c 写文件流程
6. 保存新 token 到状态文件

**验收**：首次 `photobridge export` 后，向 Photos 库加入 1 张新照片，运行 `photobridge sync` 只处理这 1 张，进度显示 "1/1"。

---

### 41d — status 子命令

`Commands/StatusCommand.swift`：

```
photobridge status
```

输出示例：
```
PhotoBridge 状态
  上次同步：2026-05-04 14:30  （5 小时前）
  已导出：12,453 张
  Photos 库当前资产数：12,460 张
  待同步：约 7 张（上次以来新增）
  状态文件：~/Library/Application Support/PhotoBridge/state.json
```

若从未同步过，打印：`尚未同步。请先运行 photobridge export 进行首次全量导出。`

**验收**：分别在首次运行前、export 后、sync 后运行 `photobridge status`，输出数字与预期一致。

---

## Step 42 — PicManager 批量导入集成

**目标**：PhotoBridge 在每批文件写出后自动调用 `picmanager import`，解析 NDJSON 日志确认成功，更新状态文件，清空暂存文件。

### 42a — PicManager 子进程调用与日志解析

新建 `Core/PicManagerRunner.swift`：

```swift
struct PicManagerRunner {
    let executablePath: URL

    /// 对 stagingDir 调用 picmanager import --copy --batch-size N --log logFile，
    /// 等待完成，解析 NDJSON 日志，返回成功 / 失败文件的 localIdentifier 列表
    func importBatch(
        stagingDir: URL,
        batchSize: Int,
        logFile: URL
    ) async throws -> ImportResult
}

struct ImportResult {
    let succeededIdentifiers: [String]    // 对应 ExportedAsset.localIdentifier
    let failedIdentifiers: [String]
    let skippedIdentifiers: [String]      // SHA-256 重复，已在库中
}
```

NDJSON 日志格式（来自 `importer/log.rs`）：

```json
{"status":"Imported","path":"/tmp/staging/01ABC.heic","sha256":"..."}
{"status":"Skipped","path":"/tmp/staging/01DEF.jpg","reason":"AlreadyImported"}
{"status":"Failed","path":"/tmp/staging/01GHI.heic","error":"..."}
```

实现细节：
- 暂存文件命名为 `<localIdentifier>.<ext>`，从 `path` 字段的文件名反推 localIdentifier
- `picmanager` 可执行文件路径：先查 `--picmanager` 参数，再搜 `PATH`，找不到时给出安装提示
- 子进程退出码非零时，保留暂存文件不清理，下次运行重试

**单元测试**（不需要真实 picmanager 二进制）：
- 解析各类 NDJSON 记录，验证分类正确
- 测试 localIdentifier 从文件名反推逻辑

---

### 42b — 端到端批量循环

在 `AssetExporter.exportBatch` 完成后，串联以下流程（在 `ExportCommand` 和 `SyncCommand` 中复用）：

```
枚举待导出资产
    └─→ 分批（每批 batch-size 张）
            ├─→ PHAssetResourceManager 写文件到 staging/
            ├─→ PicManagerRunner.importBatch()
            │       ├─→ 成功：更新 SyncState.exportedIdentifiers
            │       ├─→ 跳过（已在库中）：也加入 exportedIdentifiers
            │       └─→ 失败：记录警告，本批次暂存文件保留
            └─→ 清理本批成功/跳过文件，保留失败文件
    └─→ 保存 SyncState
    └─→ 打印汇总
```

汇总格式：
```
导出完成（耗时 3 分 42 秒）
  处理：1,200 张  导入：1,187 张  跳过（重复）：11 张  失败：2 张
  失败文件已保留在暂存目录，下次运行自动重试：~/Library/Application Support/PhotoBridge/staging/
```

**验收**：完整运行 `photobridge export`，PicManager DB 中出现对应照片记录；重新运行 `photobridge sync` 后无新导入（全部已跳过）。

---

### 42c — 磁盘空间预检

`export` / `sync` 启动时预检暂存目录剩余空间：

- 用 `PHAsset.pixelWidth × pixelHeight × 3 / 8`（JPEG 典型压缩率）估算所有待处理资产的磁盘占用
- 若估算值 > 暂存目录剩余空间的 80%，打印警告但继续（不强制中止，估算可能偏高）
- 若 Photos Library 所在路径检测为系统卷（`/` 或 `/System`），打印警告：
  ```
  ⚠️  Photos Library 位于系统卷。导入过程中 Photos 框架会临时将云端照片下载到系统卷缓存。
      建议将 Photos Library 迁移至外置硬盘后再导入。详见：photobridge help setup
  ```

---

## Step 43 — CLI 完善与文档

**目标**：完善用户体验、帮助文档，补充 README 导入指南。

### 43a — setup 引导子命令

`photobridge setup` 打印分步设置向导（纯文本，不执行任何操作）：

```
PhotoBridge 首次设置向导
══════════════════════════════

步骤 1：迁移 Photos Library 到外置硬盘（推荐）
  ① 打开 Photos.app → 偏好设置 → 通用
  ② 点击"更改"，选择外置硬盘上的目标路径
  ③ Photos 会自动移动整个库（iCloud 内容留在云端，不影响访问）

步骤 2：授予照片库访问权限
  ① 运行：photobridge status
  ② 系统弹出授权对话框，选择"完整访问"

步骤 3：首次全量导出
  ① 确保 PicManager 已启动并配置好库路径：picmanager serve
  ② 运行：photobridge export --batch-size 200
  ③ 等待完成（大型库可能需要数小时，可中断后续跑）

步骤 4：日常增量同步
  在 iPhone/iPad 拍摄并同步到 iCloud 后，运行：
  photobridge sync
  或配置 launchd 定时任务自动执行。
```

---

### 43b — launchd plist 生成

`photobridge setup --install-launchd [--interval-hours <n>]` 生成并安装 launchd 定时任务：

- 生成 `~/Library/LaunchAgents/com.picmanager.photobridge-sync.plist`
- 默认每 6 小时运行一次 `photobridge sync`
- 打印 `launchctl load` 命令供用户手动执行（不自动 load，避免不必要权限提升）

---

### 43c — README 导入指南

在项目根目录 `README.md` 的"导入"章节添加 iCloud 照片导入段落，引用流程图（从 REQUIREMENTS.md 复制）和 `photobridge setup` 命令。

---

### 43d — CLAUDE.md / 文档同步

更新 `CLAUDE.md`：
- 在"实际项目结构"中添加 `photobridge/` 目录描述
- 在"技术栈"中注明 Swift + PhotoKit 工具
- 记录 PhotoBridge 关键实现细节（PHPersistentChangeToken 序列化、localIdentifier 命名约定、资源类型过滤规则）

---

## 2.0 运动记录功能（Steps 39–41）

> TDD 原则：每个 Step 先写测试，测试失败后写实现，测试通过后提交。

---

## Step 39a — DB 迁移 + 核心类型定义

**目标**：建好数据层，后续模块都有地方写。

### 迁移文件

新增 `migrations/0015_activities.sql`：

```sql
CREATE TABLE activities (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    sha256           TEXT NOT NULL UNIQUE,
    source_path      TEXT NOT NULL,
    file_format      TEXT NOT NULL CHECK(file_format IN ('fit','gpx')),
    title            TEXT NOT NULL,
    activity_type    TEXT NOT NULL,
    start_time       TEXT NOT NULL,   -- UTC ISO-8601
    end_time         TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL,
    distance_meters  REAL NOT NULL,
    elevation_gain_meters REAL,
    avg_heart_rate   INTEGER,
    max_heart_rate   INTEGER,
    calories         INTEGER,
    device           TEXT,
    import_status    TEXT NOT NULL DEFAULT 'imported'
);

CREATE TABLE activity_track_points (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id INTEGER NOT NULL REFERENCES activities(id) ON DELETE CASCADE,
    ts          TEXT NOT NULL,   -- UTC ISO-8601
    lat         REAL NOT NULL,
    lon         REAL NOT NULL,
    elevation   REAL,
    heart_rate  INTEGER,
    cadence     INTEGER,
    speed       REAL
);

CREATE INDEX idx_track_points_activity ON activity_track_points(activity_id);
CREATE INDEX idx_activities_start_time ON activities(start_time DESC);
```

### 新建模块 `src/activities/`

```
src/activities/
  mod.rs       -- pub use + ActivityData / TrackPoint 类型定义
  fit.rs       -- FIT 文件解析
  gpx.rs       -- GPX 文件解析
  importer.rs  -- 导入器（去重 + 写库）
  geo.rs       -- 地理计算工具（haversine / RDP）
```

**核心类型**（`src/activities/mod.rs`）：

```rust
pub struct TrackPoint {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub lat: f64,
    pub lon: f64,
    pub elevation: Option<f64>,
    pub heart_rate: Option<u8>,
    pub cadence: Option<u8>,
    pub speed: Option<f64>,
}

pub struct ActivityData {
    pub title: String,
    pub activity_type: String,     // "running" / "hiking" / "cycling" / ...
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub duration_seconds: u32,
    pub distance_meters: f64,
    pub elevation_gain_meters: Option<f64>,
    pub avg_heart_rate: Option<u8>,
    pub max_heart_rate: Option<u8>,
    pub calories: Option<u32>,
    pub device: Option<String>,
    pub track: Vec<TrackPoint>,
}
```

### 地理工具（`src/activities/geo.rs`）

**haversine 距离**（两点间米数）：

```rust
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64
```

**轨迹到点的最近距离**：

```rust
/// 点 (lat, lon) 到轨迹（有序轨迹点列表）的最近 haversine 距离（米）
pub fn min_distance_to_track(lat: f64, lon: f64, track: &[(f64, f64)]) -> f64
```

**RDP 轨迹简化**：

```rust
/// Ramer-Douglas-Peucker，epsilon 单位为米
/// 返回保留的索引集合
pub fn rdp_simplify(points: &[(f64, f64)], epsilon_m: f64) -> Vec<usize>
```

### TDD 测试（`src/activities/geo.rs` 内 `#[cfg(test)]`）

| 测试 | 验收条件 |
|------|----------|
| `haversine_known_distance` | 北京（39.9042°N, 116.4074°E）到上海（31.2304°N, 121.4737°E）≈ 1068 km，误差 < 1% |
| `min_distance_to_track_on_point` | 点在轨迹上时距离 ≈ 0 |
| `min_distance_to_track_nearby` | 点在已知轨迹旁 500m 处，距离 < 510m |
| `min_distance_to_track_far` | 点在 2km 外，距离 > 1000m |
| `rdp_simplify_straight_line` | 直线上的中间点被过滤掉 |
| `rdp_simplify_preserves_endpoints` | 始终保留首尾两点 |

---

## Step 39b — FIT 文件解析

**目标**：能从 `.fit` 文件提取完整 `ActivityData`，含轨迹点和运动元数据。

### 依赖

```toml
# Cargo.toml
fitparser = "0.8"   # 或 fit-parser，根据实际可用版本
```

### 实现（`src/activities/fit.rs`）

```rust
pub fn parse_fit(path: &Path) -> anyhow::Result<ActivityData>
```

解析逻辑：
- 读取 FIT 文件的 `activity` message → `activity_type`（`sport` 字段映射）、`device`
- 读取 `session` message → `total_elapsed_time`、`total_distance`、`total_ascent`、`avg_heart_rate`、`max_heart_rate`、`total_calories`
- 读取所有 `record` message → `TrackPoint` 列表（timestamp / lat / lon / altitude / heart_rate / cadence / speed）
- Garmin FIT 坐标为 semicircles（整数），转换公式：`degrees = semicircles × (180.0 / 2^31)`
- 若无 `activity` message 中的 name，title 默认 `{date} {activity_type}`

**FIT sport 字段映射**：

| FIT sport 值 | activity_type |
|---|---|
| running (1) | running |
| cycling (2) | cycling |
| hiking (17) | hiking |
| walking (11) | walking |
| trail_running (sub_sport=3) | trail_running |
| swimming (5) | swimming |
| 其余 | other |

### TDD 测试（`src/activities/fit.rs` 内 `#[cfg(test)]`）

使用用户提供的真实 FIT 文件（`tests/samples/sample.fit`）：

| 测试 | 验收条件 |
|------|----------|
| `parse_fit_returns_ok` | `parse_fit(sample_fit)` 不报错 |
| `parse_fit_has_track_points` | `data.track.len() > 0` |
| `parse_fit_track_valid_coords` | 所有轨迹点 lat 在 [-90, 90]，lon 在 [-180, 180] |
| `parse_fit_has_valid_times` | `start_time < end_time`，duration > 0 |
| `parse_fit_distance_positive` | `distance_meters > 0` |
| `parse_fit_activity_type_known` | `activity_type` 是合法枚举值之一 |

---

## Step 39c — GPX 文件解析

**目标**：能从 `.gpx` 文件提取完整 `ActivityData`。

### 依赖

```toml
gpx = "0.10"
```

### 实现（`src/activities/gpx.rs`）

```rust
pub fn parse_gpx(path: &Path) -> anyhow::Result<ActivityData>
```

解析逻辑：
- 读取 `<trk><name>` → title
- 遍历 `<trkseg><trkpt>` → `TrackPoint`（lat/lon/ele/time）
- 读取心率扩展（`<gpxtpx:hr>` 或 `<ns3:hr>`）
- `start_time` = 第一个点的时间，`end_time` = 最后一个点的时间
- `distance_meters` = 所有相邻轨迹点 haversine 距离之和
- `elevation_gain` = 所有正爬升累计（相邻点高度差 > 0 时累加）
- `activity_type` 从 `<type>` 标签读取，映射同 FIT；无法识别时 = "other"

### TDD 测试（`src/activities/gpx.rs` 内 `#[cfg(test)]`）

使用合成 GPX fixture（`tests/samples/sample.gpx`，由测试代码内嵌字符串创建，无需外部文件）：

| 测试 | 验收条件 |
|------|----------|
| `parse_gpx_minimal_valid` | 最简 GPX（2个轨迹点）能解析不报错 |
| `parse_gpx_track_count` | 解析后 track.len() 与 trkpt 数量一致 |
| `parse_gpx_distance_computed` | 两点间距离 ≈ haversine 手算值，误差 < 1% |
| `parse_gpx_elevation_gain` | 已知爬升场景（点序列高度交替升降），累计爬升值与预期一致 |
| `parse_gpx_time_range` | start_time < end_time |
| `parse_gpx_heart_rate_extension` | 含 gpxtpx:hr 扩展时 heart_rate 字段有值 |

合成 fixture 示例（嵌在测试中）：
```xml
<?xml version="1.0"?>
<gpx version="1.1" creator="test">
  <trk><name>Test Run</name><type>running</type>
    <trkseg>
      <trkpt lat="39.9042" lon="116.4074"><ele>50.0</ele><time>2026-05-01T06:00:00Z</time></trkpt>
      <trkpt lat="39.9100" lon="116.4150"><ele>55.0</ele><time>2026-05-01T06:30:00Z</time></trkpt>
    </trkseg>
  </trk>
</gpx>
```

---

## Step 39d — 导入器

**目标**：将解析好的 `ActivityData` 写入数据库，支持去重和文件管理。

### 实现（`src/activities/importer.rs`）

```rust
pub struct ImportActivityResult {
    pub imported: usize,
    pub skipped: usize,   // SHA-256 重复
    pub failed: usize,
}

/// 导入单个文件：返回 Some(activity_id) 表示新导入，None 表示跳过（重复）
pub async fn import_activity_file(
    pool: &SqlitePool,
    path: &Path,
    activities_dir: &Path,
) -> anyhow::Result<Option<i64>>

/// 批量导入目录：递归扫描 .fit/.gpx，逐一调用 import_activity_file
pub async fn import_activities_dir(
    pool: &SqlitePool,
    source: &Path,
    activities_dir: &Path,
) -> anyhow::Result<ImportActivityResult>
```

实现要点：
1. 计算文件 SHA-256；查询 `activities` 表，若已存在则返回 `None`（跳过）
2. 按扩展名选 FIT/GPX parser，解析得到 `ActivityData`
3. 将文件复制到 `activities_dir/{yyyy}/{filename}`（目录按年份，年份来自 `start_time`）
4. 事务内写入 `activities` 行 + 批量插入 `activity_track_points` 行
5. 返回新插入的 `activity_id`

### TDD 测试（`tests/web_api.rs` 或 `src/activities/importer.rs`）

| 测试 | 验收条件 |
|------|----------|
| `import_fit_creates_activity_row` | 导入 sample.fit 后 `SELECT COUNT(*) FROM activities` = 1 |
| `import_fit_creates_track_points` | `SELECT COUNT(*) FROM activity_track_points WHERE activity_id = 1` > 0 |
| `import_fit_dedup` | 同一文件导入两次，第二次返回 None，activities 表仍只有 1 行 |
| `import_fit_copies_file` | 导入后 `activities_dir/{yyyy}/` 下有对应文件 |
| `import_gpx_creates_activity_row` | 类似 FIT，用合成 GPX fixture |
| `import_unknown_extension_returns_error` | 导入 .txt 文件返回 Err |
| `import_dir_counts_correctly` | 含 1 个 FIT + 1 个 GPX 的目录，ImportActivityResult.imported = 2 |

---

## Step 39e — CLI 命令

**目标**：`picmanager activities import <path>` 可用，输出清晰汇总。

### 实现（`src/main.rs`）

在 `clap` 的 CLI 结构中新增 `activities` 子命令：

```
picmanager activities import <path> [--dry-run]
```

输出格式：
```
扫描到 12 个运动文件（10 个 FIT，2 个 GPX）
导入 10，跳过（重复）2，失败 0
耗时 3.2 秒
```

`--dry-run` 时只打印扫描结果，不写库：
```
（预览）扫描到 12 个运动文件（10 个 FIT，2 个 GPX），实际导入前请移除 --dry-run
```

### Config 扩展（`src/config.rs`）

新增字段：
```rust
pub activities_dir: PathBuf,   // 默认 {library}/activities/
```

### TDD 测试

CLI 命令通过 `Command::new("picmanager").get_matches_from(...)` 单元测试验证参数解析；导入逻辑测试复用 Step 39d 的测试。

---

## Step 40a — API：运动列表

**目标**：`GET /api/activities` 返回分页列表，支持类型过滤。

### 路由

```
GET /api/activities?page=1&per_page=50&type=running
```

**响应 JSON**：

```json
{
  "total": 123,
  "page": 1,
  "per_page": 50,
  "activities": [
    {
      "id": 1,
      "title": "2026-05-01 跑步",
      "activity_type": "running",
      "start_time": "2026-05-01T06:00:00Z",
      "duration_seconds": 3600,
      "distance_meters": 10234.5,
      "elevation_gain_meters": 85.0,
      "avg_heart_rate": 152,
      "max_heart_rate": 178,
      "calories": 620
    }
  ]
}
```

### TDD 测试（`tests/web_api.rs`）

| 测试 | 验收条件 |
|------|----------|
| `get_activities_empty` | 无数据时返回 200，total=0，activities=[] |
| `get_activities_returns_imported` | 导入 1 条后，total=1，activities.len()=1 |
| `get_activities_pagination` | 导入 3 条，per_page=2，page=1 返回 2 条，page=2 返回 1 条 |
| `get_activities_filter_by_type` | 导入 running+hiking 各 1 条，type=running 只返回 1 条 |
| `get_activities_sorted_desc` | 返回按 start_time 降序排列 |

---

## Step 40b — API：运动详情

**目标**：`GET /api/activities/:id` 返回单次运动完整元数据。

### 路由

```
GET /api/activities/:id
```

**响应**：同列表中单条记录，额外包含 `device`、`file_format`、`source_path` 字段。

### TDD 测试

| 测试 | 验收条件 |
|------|----------|
| `get_activity_detail_ok` | 存在的 id 返回 200 + 正确字段 |
| `get_activity_detail_not_found` | 不存在的 id 返回 404 |

---

## Step 40c — API：轨迹点（含 RDP 降采样）

**目标**：`GET /api/activities/:id/track` 返回适合前端渲染的轨迹点数组。

### 路由

```
GET /api/activities/:id/track
```

**响应 JSON**：

```json
{
  "activity_id": 1,
  "total_points": 7280,
  "returned_points": 1200,
  "sampled": true,
  "points": [
    {"ts": "2026-05-01T06:00:00Z", "lat": 39.9042, "lon": 116.4074, "elevation": 50.0},
    ...
  ]
}
```

实现逻辑：
- 从 DB 读取该活动全部轨迹点（按 ts 排序）
- 若点数 > 7200，对 lat/lon 序列用 RDP 算法降采样，epsilon 取 10m
- 返回降采样后的点；`sampled: true` 表示做了降采样

### TDD 测试

| 测试 | 验收条件 |
|------|----------|
| `get_track_ok` | 存在的 id 返回 200，points 非空 |
| `get_track_not_found` | 不存在的 id 返回 404 |
| `get_track_no_downsampling_below_threshold` | 导入 100 点的合成活动，`sampled=false`，returned=total |
| `get_track_downsampling_above_threshold` | 导入 8000 点的合成活动，`sampled=true`，returned < 8000 |
| `get_track_preserves_endpoints` | 降采样后首尾点与原始首尾一致 |

---

## Step 40d — API：关联照片

**目标**：`GET /api/activities/:id/photos` 返回时间 + GPS 双重过滤后的照片列表。

### 路由

```
GET /api/activities/:id/photos
```

**响应 JSON**：

```json
{
  "activity_id": 1,
  "photos": [
    {
      "id": 42,
      "path": "2026-05-01/IMG_1234.HEIC",
      "taken_at": "2026-05-01T06:23:11Z",
      "gps_lat": 39.9120,
      "gps_lon": 116.4200,
      "distance_to_track_m": 32.5
    }
  ]
}
```

实现逻辑：
1. 查活动的 `start_time` / `end_time` + 全部轨迹点（lat/lon）
2. 查 `photos` 表：`taken_at BETWEEN start_time AND end_time AND gps_lat IS NOT NULL`
3. 对每张候选照片，调用 `min_distance_to_track(photo_lat, photo_lon, track_latlons)`
4. 过滤掉距离 > 500m 的照片
5. 返回通过过滤的照片列表（含 `distance_to_track_m` 字段）

### TDD 测试

| 测试 | 验收条件 |
|------|----------|
| `get_activity_photos_empty` | 无照片时返回空数组 |
| `get_activity_photos_time_match` | 时间在区间内、GPS 在轨迹 100m 内的照片被关联 |
| `get_activity_photos_time_outside` | 时间在区间外的照片不关联，即使 GPS 位置很近 |
| `get_activity_photos_too_far` | 时间在区间内但 GPS 距轨迹 > 500m 的照片不关联 |
| `get_activity_photos_no_gps` | 无 GPS 的照片不关联 |
| `get_activity_photos_sorted_by_time` | 返回列表按 taken_at 升序排列 |

---

## Step 41a — 前端：运动标签页 + 列表视图

**目标**：导航栏新增「运动」标签，点击后展示运动列表。

### HTML/JS 变更

- 顶部导航新增 `<button id="tab-activities">运动</button>`，对应视图 `#view-activities`
- 视图内：顶部类型筛选栏（全部 / 跑步 / 徒步 / 骑行 / 其他）+ 分页列表
- 每条列表项展示：类型图标、日期、标题、距离、时长、爬升（有数据时）、平均心率（有数据时）
- 点击列表项跳转运动详情视图

**辅助函数**：
- `formatDuration(seconds)` → "1:23:45"
- `formatDistance(meters)` → "10.23 km"
- `formatPace(seconds, meters)` → "5'32\"/km"（跑步/徒步用）或 "28.3 km/h"（骑行用）
- `activityTypeIcon(type)` → emoji 或 SVG 图标

**验收**：
- 导航切换正常，无布局错位
- 列表加载、分页、筛选功能正常
- 空状态（无运动数据）有提示文字

---

## Step 41b — 前端：运动详情 + Leaflet 地图 + 元数据

**目标**：点击列表项后进入分屏详情视图，地图展示轨迹，右侧展示元数据。

### 布局

```
┌─────────────────────────────┬──────────────────┐
│                             │  运动元数据       │
│         Leaflet 地图        │  ─────────────── │
│       （轨迹折线）          │  距离 / 时长     │
│                             │  配速 / 心率     │
│                             │  爬升 / 卡路里   │
│                             │  设备 / 文件     │
│                             │  ─────────────── │
│                             │  关联照片（N张） │
│                             │  [缩略图网格]    │
└─────────────────────────────┴──────────────────┘
```

### Leaflet 集成

- 调用 `GET /api/activities/:id/track` 获取轨迹点，用 `L.polyline` 绘制，蓝色
- 调用 `fitBounds` 自动缩放到轨迹包围盒
- 不需要额外引入 Leaflet（项目地理视图已有）

### 元数据渲染

- 距离：`formatDistance(distance_meters)`
- 时长：`formatDuration(duration_seconds)`
- 配速/速度：依 `activity_type` 选择格式（running/hiking/trail_running → 配速，cycling → 速度）
- 心率、爬升、卡路里、设备：有值时显示，null 时该行不渲染

**验收**：
- 地图正常渲染轨迹折线
- 元数据信息齐全，格式正确
- 左右分屏布局在常见窗口宽度下不错位

---

## Step 41c — 前端：照片标记 + 关联照片网格

**目标**：地图上显示关联照片图标，右侧展示照片缩略图，点击可跳转详情。

### 地图照片标记

- 调用 `GET /api/activities/:id/photos` 获取关联照片（仅取有 GPS 的）
- 每张照片用 `L.marker` 标注在地图上，图标为相机 emoji 或小缩略图
- 点击标记弹出 Leaflet popup，显示 100×100 缩略图（复用 `GET /api/photos/:id/thumb`）
- popup 内「查看照片」链接，点击切换到该照片的照片详情视图

### 关联照片网格

- 右侧元数据区下方展示「本次运动照片（N张）」
- 同照片浏览视图缩略图网格样式（3列，正方形裁剪）
- 按拍摄时间升序排列
- 点击缩略图跳转照片详情
- 0张时显示「本次运动无关联照片」

**验收**：
- 地图标记位置与照片 GPS 一致
- popup 缩略图正常加载
- 右侧网格与地图标记数量一致（GPS 照片同时出现在两处，无 GPS 照片仅出现在网格）
- 点击跳转正常

---

## Step 41d — 文档同步

**目标**：所有文档与实现保持一致。

- **REQUIREMENTS.md**：确认运动记录章节与实现一致（特别是 API 路径、字段名）
- **DESIGN.md**：新增 4 个 API 端点说明（`/api/activities` 相关）；新增 `activities` / `activity_track_points` 表 schema
- **ARCHITECTURE.md**：新增 `src/activities/` 模块描述
- **CLAUDE.md**：
  - `src/` 目录结构中新增 `activities/` 模块
  - 新增 FIT/GPX 解析关键细节（crate 版本、坐标转换、字段映射）
  - 新增 `tests/samples/sample.fit` 说明
  - 更新当前测试数量

