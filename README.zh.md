# PicManager

家庭照片管理工具。导入、去重、按时间 / 地点 / 相机整理照片，检测人脸和动物，通过 Web 界面浏览所有内容——完全本地运行，无需云服务。

English documentation: [README.md](README.md)

---

## 可执行文件

| 二进制 | 语言 | 用途 |
|--------|------|------|
| `picmanager` | Rust | 主程序 — 导入、去重、启动 Web UI、CLI |
| `photobridge` | Swift | iCloud 照片伴侣 — 从 Photos.app 导出并送入 picmanager |

---

## 核心功能

- **导入** — 扫描目录，将照片移入带日期的照片库（`library/YYYY-MM-DD/`），按 SHA-256 跳过重复
- **智能日期推断** — EXIF → 文件 mtime → 文件名模式 → `unknown/`
- **两层去重** — Gradient pHash（快速粗筛）+ DCT pHash（精确验证）+ Union-Find 聚类
- **自动相册** — 按月份、相机型号、GPS 城市（Nominatim 反地理编码）
- **人脸检测与聚类** — ultraface-slim-320 + ArcFace 512 维嵌入 + DBSCAN 人物聚类；全本地，无需 API Key
- **动物检测** — YOLOv8-nano，10 种 COCO 动物，导入时自动运行
- **Web UI** — 照片网格、相册侧边栏、人物管理、地图视图、去重确认、精选集
- **PhotoBridge** — 通过 `PHPersistentChangeToken` 增量同步 iCloud；自动从 `PHAsset.creationDate` 设置文件时间戳；自动修正 HEIC EXIF 方向使其与 Photos.app 显示一致（需要 `exiftool`）

---

## 环境要求

- **picmanager**：Rust 1.95+，macOS（主要平台）；HEIC 支持需 `brew install libheif`
- **photobridge**：macOS 13+，Xcode 命令行工具；HEIC 方向修正需 `brew install exiftool`

---

## 编译

```bash
# picmanager
cargo build --release          # → target/release/picmanager

# photobridge（可选，iCloud 同步伴侣）
cd photobridge
swift build -c release
codesign --force --sign - \
  --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \
  .build/release/photobridge
```

---

## 快速上手

```bash
# 1. 导入一个照片目录（移动文件到照片库）
picmanager import ~/Downloads/photos/

# 2. 启动 Web UI
picmanager serve               # → http://127.0.0.1:8080

# 3. 下载 AI 模型（人脸 + 动物检测）
picmanager models fetch

# 4. 从 iCloud 导入（PhotoBridge）
photobridge setup              # 首次配置向导
photobridge export             # 全量导出 + 若 picmanager 在 PATH 中则自动导入
photobridge sync               # 后续增量同步
# 修复已导出 HEIC 文件的方向
photobridge fix-orientations --dir ~/staging/ --dry-run  # 检查方向不一致
photobridge fix-orientations --dir ~/staging/            # 应用修复
```

完整 CLI 参考、REST API、配置项和 PhotoBridge 选项，参见 **[docs/MANUAL.zh.md](docs/MANUAL.zh.md)**。
