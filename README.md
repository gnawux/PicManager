# PicManager

家庭照片与图片管理工具。自动整理照片、识别重复、按时间/地点/相机分相册，提供 Web 界面和命令行界面。

*A family photo management tool. Automatically organizes photos, detects duplicates, groups them by time/location/camera, and provides a Web UI and CLI.*

---

## 功能现状 / Status

| 功能 | 状态 |
|------|------|
| 从目录导入照片 | ✓ |
| EXIF 元数据提取（时间、相机、GPS） | ✓ |
| 重复照片识别（SHA-256 精确 + 导入去重） | ✓ |
| 导入状态跟踪（防止重复导入） | ✓ |
| 格式识别（JPEG/PNG/GIF/WebP/HEIC/ARW） | ✓ |
| Web API（照片列表、缩略图、导入触发） | ✓ |
| 感知哈希去重（pHash） | 待实现 |
| 相册自动分组 | 待实现 |
| Web 前端界面 | 待实现 |

## 环境要求

- Rust 1.95+
- macOS（当前主要支持平台）
- [libheif](https://github.com/strukturag/libheif)（HEIC 格式支持，通过 Homebrew 安装）

```bash
brew install libheif
```

## 构建

```bash
cargo build --release
```

## 使用

### 导入照片

从指定目录扫描并导入照片到本地库：

```bash
picmanager import ~/Pictures/从手机导出的照片/
```

输出示例：

```
从 /Users/gnawux/Pictures/从手机导出的照片/ 导入照片...
完成：共 128 张，导入 120，跳过 8，失败 0
```

- **导入**：新照片写入数据库，提取 EXIF 元数据
- **跳过**：SHA-256 相同的照片（已导入或重复文件）不重复写入
- 源目录文件**不会被修改或删除**，用户可在确认导入成功后手工清理

支持格式：JPEG、PNG、GIF、WebP、HEIC（含苹果 Live Photo）、ARW（索尼 RAW）

### 启动 Web 服务

```bash
picmanager serve
```

默认监听 `http://127.0.0.1:8080`。

## Web API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/photos` | 照片列表（支持分页） |
| GET | `/api/photos/:id/thumb` | 照片缩略图（300px JPEG） |
| POST | `/api/import` | 触发后台导入任务 |
| GET | `/api/import/status` | 查询导入进度 |

**分页参数：**

```
GET /api/photos?page=1&per_page=50
```

**响应示例：**

```json
{
  "photos": [
    {
      "id": 1,
      "path": "/Users/gnawux/Pictures/PicManager/...",
      "format": "jpeg",
      "taken_at": "2024-06-15 10:30:00",
      "camera": "Apple iPhone 15 Pro",
      "import_status": "imported"
    }
  ],
  "total": 128,
  "page": 1,
  "per_page": 50
}
```

**触发导入：**

```bash
curl -X POST http://localhost:8080/api/import \
  -H 'Content-Type: application/json' \
  -d '{"dir": "/path/to/photos"}'
```

## 数据存储

照片元数据存储在 SQLite 数据库中，默认路径：

```
~/Pictures/PicManager/picmanager.db
```

原始照片文件**不做任何修改**，数据库只存储元数据和导入状态。

## 开发

```bash
# 运行测试
cargo nextest run

# 代码检查
cargo clippy -- -D warnings

# 监听文件变化自动重编译
cargo watch -x build
```

测试覆盖 41 个用例，涵盖格式识别、EXIF 提取、导入去重、数据库操作、HTTP API 等核心路径。

## 项目结构

```
src/
  main.rs          # CLI 入口（import / serve）
  lib.rs           # 库根
  config.rs        # 配置（库路径、端口等）
  error.rs         # 统一错误类型
  importer/        # 导入器（扫描、SHA-256、写库）
  metadata/        # 元数据提取（格式识别、EXIF、GPS）
  storage/         # 数据库连接与迁移
  web/             # Axum Web 服务器与 API handlers
migrations/        # SQLite 迁移文件
tests/             # 集成测试
docs/              # 架构设计与开发计划
```

---

## English

### Status

| Feature | Status |
|---------|--------|
| Import photos from directory | ✓ |
| EXIF metadata extraction (time, camera, GPS) | ✓ |
| Duplicate detection (SHA-256 exact match) | ✓ |
| Import state tracking (skip already-imported) | ✓ |
| Format detection (JPEG/PNG/GIF/WebP/HEIC/ARW) | ✓ |
| Web API (photo list, thumbnails, import trigger) | ✓ |
| Perceptual hash dedup (pHash) | planned |
| Auto album grouping | planned |
| Web frontend | planned |

### Requirements

- Rust 1.95+
- macOS (primary platform for now)
- [libheif](https://github.com/strukturag/libheif) for HEIC support

```bash
brew install libheif
```

### Build

```bash
cargo build --release
```

### Usage

**Import photos** from a directory into the library:

```bash
picmanager import ~/Pictures/exported-from-phone/
```

```
Importing from /Users/gnawux/Pictures/exported-from-phone/ ...
Done: 128 total, 120 imported, 8 skipped, 0 errors
```

- Source files are **never modified or deleted** — clean up manually after confirming a successful import.
- Duplicate files (same SHA-256) are skipped without writing to the database again.

Supported formats: JPEG, PNG, GIF, WebP, HEIC (incl. Apple Live Photo), ARW (Sony RAW)

**Start the Web server** (default: `http://127.0.0.1:8080`):

```bash
picmanager serve
```

### Web API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/photos` | Paginated photo list |
| GET | `/api/photos/:id/thumb` | 300px JPEG thumbnail |
| POST | `/api/import` | Trigger background import |
| GET | `/api/import/status` | Poll import progress |

**Trigger import:**

```bash
curl -X POST http://localhost:8080/api/import \
  -H 'Content-Type: application/json' \
  -d '{"dir": "/path/to/photos"}'
```

### Data Storage

Metadata is stored in SQLite at `~/Pictures/PicManager/picmanager.db`. Original photo files are never touched.

### Development

```bash
cargo nextest run          # run tests (41 cases)
cargo clippy -- -D warnings
cargo watch -x build       # auto-rebuild on changes
```
