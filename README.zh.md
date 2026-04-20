# PicManager

用 Rust 编写的家庭照片管理工具。自动整理照片、检测重复，按时间、相机和地点分组归集为相册，本地离线检测人脸，同时提供 Web 界面和命令行工具。

English documentation: [README.md](README.md)

## 功能列表

| 功能 | 状态 |
|------|------|
| 从目录导入照片 | ✓ |
| EXIF 元数据提取（时间、相机、GPS） | ✓ |
| 精确重复检测（SHA-256） | ✓ |
| 感知重复检测（dHash） | ✓ |
| 重复确认工作流（保留 / 软删除） | ✓ |
| 导入状态追踪（跳过已导入文件） | ✓ |
| 格式检测（JPEG / PNG / GIF / WebP / HEIC / ARW） | ✓ |
| 按月份、相机、GPS 地点自动分组为相册 | ✓ |
| 手动合并相册 | ✓ |
| Web 界面（照片网格、相册导航、导入面板、重复处理弹窗） | ✓ |
| REST API | ✓ |
| 配置文件（`~/Library/Application Support/picmanager/config.toml`） | ✓ |
| 导入时本地离线人脸检测（ultraface-slim-320 ONNX） | ✓ |
| 人脸特征提取（ArcFace MobileNetV1，512 维 L2 归一化） | ✓ |
| 批量人脸重分析（全库或指定照片） | ✓ |
| CLI 下载 ONNX 模型文件（`models fetch`） | ✓ |
| 人物视图：DBSCAN 自动聚类（ArcFace 嵌入向量） | ✓ |
| 树状人物层级（parent_id，无限深度） | ✓ |
| 导入时动物检测（YOLOv8-nano ONNX，10 种 COCO 动物） | ✓ |
| 动物种类浏览（卡片网格 + bounding-box overlay） | ✓ |
| 地点层级视图（国家 → 省/州 → 城市三列钻取） | ✓ |
| 地图打点视图（Leaflet.js + markercluster） | ✓ |
| 照片时间/时区编辑（仅写数据库，不回写 EXIF） | ✓ |

## 环境要求

- Rust 1.95+
- macOS（主要平台；其他平台计划支持）
- [libheif](https://github.com/strukturag/libheif) — 用于 HEIC / Apple Live Photo 支持
- [ONNX Runtime](https://github.com/microsoft/onnxruntime) — 用于人脸检测和特征提取（可选；不安装时人脸功能静默跳过）

```bash
brew install libheif
# 可选 — 启用人脸检测：
brew install onnxruntime
picmanager models fetch   # 下载 face_detector.onnx + arcface_mobilenetv1.onnx + yolov8n.onnx
```

## 编译

```bash
cargo build --release
```

编译产物位于 `target/release/picmanager`。

## 使用方法

### 导入照片

扫描目录并将所有支持格式的照片导入到照片库：

```bash
picmanager import ~/Pictures/exported-from-phone/
```

```
从 /Users/alice/Pictures/exported-from-phone/ 导入照片...
完成：共 128 张，导入 120，跳过 8，失败 0
```

- 源文件**不会被修改或删除** — 确认导入无误后可手动清理。
- SHA-256 相同的文件在重新导入时会被跳过。
- 导入完成后，照片自动按月份和相机型号分组为相册。

支持格式：JPEG、PNG、GIF、WebP、HEIC（含 Apple Live Photo）、ARW（Sony RAW）

### 查找并确认重复照片

```bash
picmanager dedup
```

扫描所有已导入照片的视觉相似性（感知哈希，汉明距离 ≤ 10），然后逐组交互式展示重复结果。输入要保留的照片 ID，其余照片将被软删除（在数据库中标记为 `deleted` — 磁盘文件不会被删除）。

### 启动 Web 服务

```bash
picmanager serve
```

启动后访问 `http://127.0.0.1:8080` — 提供照片网格、相册侧边栏、导入面板和重复处理弹窗。

### 查看当前配置

```bash
picmanager config
```

输出所有配置项及配置文件路径。

### 人脸检测与特征提取

首次使用前下载模型文件：

```bash
picmanager models fetch
```

将 `face_detector.onnx`（约 1 MB）、`arcface_mobilenetv1.onnx`（约 10 MB）和 `yolov8n.onnx`（约 6 MB）下载到
`~/Library/Application Support/picmanager/models/`。此后每次导入照片时自动检测人脸和动物。

对全库照片重新分析（例如首次下载模型后）：

```bash
picmanager faces analyze
```

对指定照片重新分析：

```bash
picmanager faces analyze --photo-ids 1,2,3
```

人脸数据仅存储在本地 SQLite 数据库，不调用任何云服务。

## 配置

创建 `~/Library/Application Support/picmanager/config.toml` 来覆盖默认值：

```toml
library_path = "/Volumes/NAS/Photos/PicManager"
host         = "0.0.0.0"
port         = 9090
thumb_size   = 400
```

优先级：命令行参数 > 配置文件 > 内置默认值。

## REST API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/photos` | 分页照片列表 |
| GET | `/api/photos/gps-points` | 所有带 GPS 照片的坐标列表 |
| POST | `/api/photos/batch-update` | 批量修改多张照片的时间/时区 |
| GET | `/api/photos/:id` | 单张照片详情 |
| PATCH | `/api/photos/:id` | 修改拍摄时间/时区（仅写数据库） |
| GET | `/api/photos/:id/thumb` | 300px JPEG 缩略图 |
| POST | `/api/import` | 触发后台导入任务 |
| GET | `/api/import/status` | 轮询导入进度 |
| GET | `/api/dedup` | 列出待确认的重复组 |
| POST | `/api/dedup/:group_id/resolve` | 确认保留哪些照片 |
| GET | `/api/albums` | 列出所有相册及照片数量 |
| GET | `/api/albums/:id/photos` | 分页获取相册内的照片 |
| POST | `/api/albums/merge` | 合并两个相册 |
| GET | `/api/photos/:id/faces` | 获取照片中检测到的人脸区域 |
| POST | `/api/faces/analyze` | 触发人脸重分析任务（全库或指定照片） |
| GET | `/api/faces/jobs/:id` | 轮询人脸分析任务进度 |
| GET | `/api/faces/:id/thumb` | 人脸裁剪缩略图 |
| GET | `/api/geo/hierarchy` | 地理层级（国家→省→城市，含照片数） |
| GET | `/api/people` | 人物列表 |
| GET | `/api/people/tree` | 嵌套人物树 |
| POST | `/api/people/cluster` | 触发 DBSCAN 重聚类 |
| POST | `/api/people/merge` | 合并两个人物记录 |
| GET | `/api/people/:id` | 人物下的照片列表 |
| POST | `/api/people/:id/reparent` | 变更人物的父节点 |
| GET | `/api/animals/species` | 动物种类列表（含照片数） |
| GET | `/api/animals/:species/photos` | 指定种类的照片列表 |
| GET | `/api/photos/:id/animals` | 照片中的动物检测结果 |

**示例 — 触发导入：**

```bash
curl -X POST http://localhost:8080/api/import \
  -H 'Content-Type: application/json' \
  -d '{"dir": "/path/to/photos"}'
```

**示例 — 确认重复组 3，保留照片 7：**

```bash
curl -X POST http://localhost:8080/api/dedup/3/resolve \
  -H 'Content-Type: application/json' \
  -d '{"keep": [7]}'
```

**示例 — 将相册 2 合并到相册 1：**

```bash
curl -X POST http://localhost:8080/api/albums/merge \
  -H 'Content-Type: application/json' \
  -d '{"source": 2, "target": 1}'
```

## 数据存储

元数据存储在以下路径的 SQLite 数据库中：

```
~/Pictures/PicManager/picmanager.db
```

原始照片文件**永远不会被修改**。数据库只存储元数据和状态信息。

## 开发

```bash
cargo nextest run            # 运行全部 182 个测试（另有 5 个 #[ignore] 需要 ONNX 模型文件）
cargo clippy -- -D warnings  # 代码检查
cargo watch -x build         # 文件变更时自动重新编译
```

## 项目结构

```
src/
  main.rs        CLI 入口（import / dedup / faces / models / serve / config）
  config.rs      Config 结构体及 TOML 配置文件加载
  error.rs       统一的 AppError 类型（含 ModelNotFound）
  importer/      目录扫描、SHA-256、导入流水线
  metadata/      格式检测（魔数字节）、EXIF/GPS 提取
  dedup/         感知哈希、候选扫描、重复确认工作流
  album/         按月份、相机、GPS 地点自动分组；手动合并
  face/          本地人脸检测（ultraface）、特征提取（ArcFace）、DBSCAN 聚类、批量作业
  animal/        导入时动物检测（YOLOv8-nano ONNX，10 种 COCO 动物）
  storage/       SQLite 连接池、数据库迁移
  web/           Axum 服务器、REST 处理器、静态文件服务
frontend/        静态 HTML + CSS + JS（无需构建步骤）
migrations/      SQLx 数据库迁移文件（0001–0009）
tests/           集成测试 + 真实相机样本照片
docs/            架构设计与开发计划
```
