# PicManager

家庭照片与图片管理工具。支持自动整理、去重、按时间/地点/相机分相册，提供 Web 界面和命令行界面。

## 技术栈

- 语言：Rust（edition 2024，当前工具链 1.95+）
- HTTP 框架：Axum 0.8
- 数据库：SQLite via sqlx 0.8（编译期查询检查）
- 异步运行时：tokio
- EXIF 解析：kamadak-exif 0.5
- 图像处理：image 0.25 + image_hasher 3
- 静态文件：rust-embed 8（前端编译进二进制）
- CLI：clap 4

## 实际项目结构

```
src/
  main.rs              CLI 入口（import [--copy] / dedup / serve / config）
  lib.rs               库根
  config.rs            Config 结构体；配置文件 ~/Library/Application Support/picmanager/config.toml
  error.rs             AppError 枚举（NotFound / UnsupportedFormat / Metadata / Database / Io）
  importer/
    mod.rs             import_dir(pool, source_dir, library_path, copy_only) -> ImportSummary
    placer.rs          place(src, library_path, date, copy_only) -> PathBuf
    scanner.rs         scan_dir() 递归扫描，magic bytes 过滤
    state.rs           compute_sha256()，decide() 按 sha256 判断是否已导入
  metadata/
    mod.rs             re-export extract_from_file, infer_date
    exif.rs            EXIF 四字段回退链 + GPS + 相机
    filename.rs        文件名日期推断（Unix 时间戳/紧凑/分隔符）
    format.rs          magic bytes 格式检测
    types.rs           ImageFormat 枚举，PhotoMeta 结构体
  dedup/
    mod.rs             scan(), list_groups(), resolve()
    hash.rs            compute_phash(), hamming_distance()
    candidate.rs       O(n²) pHash 比较，写入 dedup_groups
  album/
    mod.rs
    organize.rs        group_by_month(), group_by_camera()
    location.rs        group_by_location()，Nominatim 反地理编码 + geocache
    merge.rs           merge(source_id, target_id)
  storage/
    mod.rs
    db.rs              connect()，运行迁移
  web/
    mod.rs             AppState, router(), serve()
    embed.rs           rust-embed 静态文件服务
    handlers/
      import.rs        POST /api/import（body: {dir, copy?}），GET /api/import/status
      photos.rs        GET /api/photos, GET /api/photos/{id}/thumb
      dedup.rs         GET /api/dedup, POST /api/dedup/{group_id}/resolve
      albums.rs        GET /api/albums, GET /api/albums/{id}/photos, POST /api/albums/merge
frontend/              HTML + CSS + JS（编译进二进制，不依赖运行时工作目录）
migrations/
  0001_initial.sql     photos, albums, photo_albums, dedup_groups, dedup_members, import_sessions
  0002_geocache.sql    geocache 表（GPS 坐标 → 城市名缓存）
tests/
  web_api.rs           Web API 集成测试（tower::ServiceExt::oneshot）
  fixtures/            测试 fixture JPEG 文件（由 make_fixtures.py 生成）
  make_fixtures.py     生成所有 fixture（Pillow + 原始 EXIF 二进制写入）
docs/
  REQUIREMENTS.md      需求文档
  PLAN.md              开发计划（Steps 1–13 已全部完成）
  ARCHITECTURE.md      架构设计
  DESIGN.md            详细设计（模块接口、DB schema、API 参考、测试策略）
```

## 开发原则

- 照片安全优先：去重删除需人工确认（软删除 `import_status='deleted'`），不自动操作文件
- 导入默认 **移动** 文件到库目录；`--copy` 保留源文件
- 数据库 `photos.path` 存储库内最终路径，不是源路径
- 单文件导入失败只记录 `tracing::warn`，不中断整批次

## 开发状态

**已完成：Steps 1–13（docs/PLAN.md 全部步骤）**

| Step | 内容 |
|------|------|
| 1 | 项目脚手架 |
| 2 | 数据库 Schema（sqlx migrate） |
| 3 | 元数据提取（EXIF / GPS / 相机） |
| 4 | 导入器（扫描 + SHA-256 去重 + 写库） |
| 5 | Web 服务器骨架 + 照片列表 + 缩略图 API |
| 6 | 感知哈希（pHash）+ 重复候选发现 |
| 7 | 去重确认工作流（Web + CLI） |
| 8 | 相册自动分组（按月份 / 相机） |
| 9 | Web 前端（照片网格、相册导航、导入面板） |
| 10 | 配置文件支持 + 相册手动合并 |
| 11 | GPS 地点相册（Nominatim + geocache） |
| 12a | EXIF 四字段回退链 |
| 12b | 文件名日期推断（metadata/filename.rs） |
| 13 | 导入重构：移动文件到 library，按日期目录组织，--copy 选项 |

当前测试数：**97 个**（`cargo nextest run` 全部通过）

## 关键实现细节（避免踩坑）

### 测试 fixture 生成

- **不要用 exiftool 修改现有 JPEG 的 ExifIFD 字段**（DateTimeDigitized、DateTime 等）——exiftool 会把这些写入 XMP（APP2），而 kamadak-exif 只读 EXIF APP1，导致字段读不到
- 所有 fixture 由 `tests/make_fixtures.py` 生成（Python + Pillow + 原始 TIFF 二进制），需要时执行 `python3 tests/make_fixtures.py` 重新生成
- fixture 测试使用 `copy_only=true`，否则测试会把 fixture 文件移走

### TIFF inline value 规则

EXIF 中，IFD 条目 count×type_size ≤ 4 字节时，值直接存在 ValueOffset 字段（inline），不是偏移量。GPS Ref 字段（"N"/"S"/"W"/"E"，2 字节 ASCII）必须 inline 存储，否则 kamadak-exif 读出乱码。

### kamadak-exif display_value() 格式

DateTime 类字段的展示格式为 `YYYY-MM-DD HH:MM:SS`（破折号分隔日期），不是原始存储的 `YYYY:MM:DD`。解析时用 `"%Y-%m-%d %H:%M:%S"`。

### 内存 SQLite 测试

```rust
SqlitePoolOptions::new()
    .max_connections(1)   // 必须，否则连接关闭后数据丢失
    .connect("sqlite::memory:")
```

### 图像测试性能

`Cargo.toml` 已配置 `[profile.test] opt-level = 2`，图像处理测试从 40+ 秒降至 ~1.5 秒，不要删除。

### 导入去重逻辑

`decide()` 只检查 sha256（不看 path），因为文件移入库后 path 已变，源路径和库路径不会相同。

### 日期推断三级链

```
EXIF 四字段（DateTimeOriginal→DateTimeDigitized→GPS→DateTime）
  → filename::infer_date(filename)
  → None（→ library/unknown/）
```

## 常用命令

```bash
cargo nextest run            # 跑全部测试
cargo nextest run <模块>     # 跑特定模块，如 metadata::exif
cargo clippy                 # 检查警告
python3 tests/make_fixtures.py  # 重新生成 fixture 文件

picmanager import <dir>      # 导入（移动文件）
picmanager import --copy <dir>  # 导入（保留源文件）
picmanager serve             # 启动 Web（http://127.0.0.1:8080）
picmanager config            # 显示当前配置
```
