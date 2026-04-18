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
