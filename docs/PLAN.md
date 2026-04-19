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

## Step 12 — 文件名日期推断

**目标**：当 EXIF 中没有拍摄时间时，尝试从文件名中解析日期，作为日期推断的第二优先级。

**背景**：手机和相机导出的文件名常带有日期信息（如 `IMG_20240615_103000.jpg`）；部分工具将 Unix 时间戳作为文件名。此步骤独立实现，方便单独测试，后续在 Step 13 中集成进导入流水线。

- 新增 `metadata/filename.rs` 模块，暴露：
  ```rust
  pub fn infer_date(filename: &str) -> Option<NaiveDateTime>
  ```
- 按顺序尝试以下规则（任意一条匹配即返回）：
  1. **Unix 时间戳**：文件名（去除扩展名）全为数字，10 位（秒级）或 13 位（毫秒级），转换为 UTC 日期时间
  2. **紧凑日期时间**：匹配 `YYYYMMDD_HHMMSS` 或 `YYYYMMDD-HHMMSS`（如 `20240615_103000`），允许前后有其他字符
  3. **分隔符日期**：匹配 `YYYY-MM-DD` 或 `YYYY_MM_DD`（如 `2024-06-15`），时间部分可选
  4. 以上均不匹配 → 返回 `None`
- 只解析合法日期（月 1–12、日 1–31），拒绝如 `20241332` 这样的无效数字串
- 单元测试覆盖：
  - `IMG_20240615_103000.jpg` → `2024-06-15 10:30:00`
  - `2024-06-15_vacation.jpg` → `2024-06-15 00:00:00`
  - `1718443800.jpg`（Unix 秒）→ 正确 UTC 时间
  - `1718443800000.jpg`（Unix 毫秒）→ 正确 UTC 时间
  - `DSC_0001.jpg` → `None`
  - `20241332_photo.jpg`（非法日期）→ `None`

**验收**：`cargo nextest run` 全部通过，`metadata::filename` 单元测试覆盖上述全部用例。

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
