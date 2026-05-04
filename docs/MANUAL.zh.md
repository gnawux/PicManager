# PicManager — 用户手册

`picmanager` CLI、`photobridge` CLI、REST API 和配置项的完整参考文档。

---

## 目录

1. [picmanager CLI](#picmanager-cli)
2. [配置](#配置)
3. [REST API](#rest-api)
4. [PhotoBridge](#photobridge)
5. [数据存储](#数据存储)
6. [开发](#开发)

---

## picmanager CLI

### `import` — 将照片导入照片库

```bash
picmanager import <dir>
picmanager import --copy <dir>          # 复制而非移动（保留源文件）
picmanager import --batch-size 200 <dir>
picmanager import --log import.ndjson <dir>
picmanager import --dry-run <dir>       # 统计文件数量，不实际导入
```

**日期推断链（按优先级排列）：**

| 优先级 | 来源 | 说明 |
|--------|------|------|
| 1 | EXIF（DateTimeOriginal → DateTimeDigitized → GPS DateStamp → DateTime） | 最可靠 |
| 2 | 文件 mtime | 由 PhotoBridge 从 `PHAsset.creationDate` 设置 |
| 3 | 文件名模式 | Unix 时间戳（10/13 位）、YYYYMMDD_HHMMSS、YYYY-MM-DD |
| 4 | 无法推断 | 放入 `library/unknown/` |

**参数说明：**

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--copy` | — | 复制文件，不移动 |
| `--batch-size <n>` | （全部）| 导入 N 个文件后停止 |
| `--log <path>` | — | 写入 NDJSON 导入日志（每文件一行）|
| `--dry-run` | — | 扫描并报告数量，不修改任何内容 |

**NDJSON 日志格式**（`--log`）：

```json
{"path":"/staging/file.heic","status":"imported","sha256":"abc...","error":null,"ts":"2026-01-01T00:00:00Z"}
{"path":"/staging/dup.jpg","status":"skipped","sha256":"abc...","error":null,"ts":"..."}
{"path":"/staging/bad.jpg","status":"failed","sha256":null,"error":"unsupported format","ts":"..."}
```

---

### `dedup` — 查找视觉重复照片

```bash
picmanager dedup            # 增量扫描（仅扫描新照片）
picmanager dedup --full     # 全量重扫整个照片库
```

使用两层算法：

1. **第一层** — Gradient pHash，汉明距离 ≤ 10（拍摄时间差 > 60 秒时 ≤ 8）
2. **第二层** — DCT pHash 验证，汉明距离 ≤ 8 — 消除误报

重复组以 *pending* 状态存储在数据库中。在 Web UI（🔍 按钮）或通过 REST API 确认处理。不会自动删除文件——仅软删除。

---

### `serve` — 启动 Web 服务

```bash
picmanager serve
picmanager serve --host 0.0.0.0 --port 9090
```

默认访问地址 `http://127.0.0.1:8080`。包含照片网格、相册侧边栏、人物管理、地图视图、去重确认和精选集。

---

### `faces` — 人脸分析

```bash
picmanager faces analyze                      # 重新分析整个照片库
picmanager faces analyze --photo-ids 1,2,3    # 指定照片
picmanager faces analyze --rotated-only       # 仅处理有非零旋转/翻转的照片
```

---

### `fill-missing` — 补全缺失元数据

```bash
picmanager fill-missing            # 同时补充人脸和地理编码
picmanager fill-missing --faces    # 仅补充未做人脸分析的照片
picmanager fill-missing --geo      # 仅对有 GPS 但无城市名缓存的照片触发反地理编码
```

每分钟打印一次进度：

```
开始补全缺失元数据…
  待补充人脸分析：75 张
  待补充地理编码：23 张

[00:01:00] 人脸：12/75 (16%) ｜ 地理：3/23 (13%)
[00:03:45] 人脸：75/75 (100%) ｜ 地理：20/23 (87%)

补全完成（耗时 3 分 45 秒）
```

---

### `models` — 管理 ONNX 模型文件

```bash
picmanager models fetch     # 下载 face_detector.onnx（约 1 MB）、
                            # arcface_mobilenetv1.onnx（约 10 MB）、
                            # yolov8n.onnx（约 6 MB）
picmanager models bundle    # 复制模型到 ./models/ 并嵌入下次编译的二进制
```

模型存储在 `~/Library/Application Support/picmanager/models/`。模型存在时，每次导入照片自动运行人脸和动物检测。

---

### `config` — 显示当前配置

```bash
picmanager config
```

---

## 配置

创建 `~/Library/Application Support/picmanager/config.toml`：

```toml
library_path = "/Volumes/NAS/Photos/PicManager"
host         = "0.0.0.0"
port         = 9090
thumb_size   = 400
```

优先级：命令行参数 > 配置文件 > 内置默认值。

---

## REST API

### 照片

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/photos` | 分页照片列表（`?page=`、`?per_page=`、`?album_id=`、`?in_collection=`） |
| GET | `/api/photos/gps-points` | 所有带 GPS 照片的坐标 |
| POST | `/api/photos/batch-update` | 批量修改时间 / 时区 / 旋转 |
| GET | `/api/photos/:id` | 单张照片详情（EXIF、GPS、人脸、动物） |
| PATCH | `/api/photos/:id` | 修改 taken_at / timezone_offset / rotation / flip |
| GET | `/api/photos/:id/thumb` | 300px JPEG 缩略图 |
| GET | `/api/photos/:id/file` | 原始文件字节（Content-Type 根据格式推断） |
| GET | `/api/photos/:id/faces` | 照片中检测到的人脸区域 |
| GET | `/api/photos/:id/animals` | 照片中的动物检测结果 |

### 导入

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/import` | 触发后台导入（`{"dir": "..."}` ） |
| GET | `/api/import/status` | 轮询导入进度 |

### 去重

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/dedup` | 列出待确认的重复组（含文件名、宽高、日期） |
| POST | `/api/dedup/:group_id/resolve` | 确认保留哪些照片（`{"keep": [id, ...]}` ） |

### 相册

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/albums` | 所有相册及照片数量和 `latest_photo_at` |
| GET | `/api/albums/:id/photos` | 相册内分页照片 |
| POST | `/api/albums/merge` | 合并相册（`{"source": 2, "target": 1}` ） |

### 精选集

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/collections` | 列出所有精选集 |
| POST | `/api/collections` | 创建精选集（`{"name": "..."}` ） |
| PATCH | `/api/collections/:id` | 重命名精选集 |
| DELETE | `/api/collections/:id` | 删除精选集 |
| GET | `/api/collections/:id/photos` | 精选集内的照片 |
| POST | `/api/collections/:id/photos` | 添加照片（`{"photo_ids": [...]}` ） |
| DELETE | `/api/collections/:id/photos` | 移除照片（`{"photo_ids": [...]}` ） |

### 人脸与人物

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/faces/analyze` | 触发人脸重分析（`{}` 全库、`{"photo_ids": [...]}` 指定、`{"missing_only": true}` 仅补缺） |
| GET | `/api/faces/jobs/:id` | 轮询人脸任务进度 |
| GET | `/api/faces/:id/thumb` | 人脸裁剪缩略图 |
| GET | `/api/people` | 人物列表（`?status=active\|ignored\|not_a_person\|all`，`?name_exact=`） |
| GET | `/api/people/tree` | 带 `cover_face_id` 的嵌套人物树 |
| POST | `/api/people/cluster` | 触发完整 DBSCAN 重聚类 |
| POST | `/api/people/cluster/incremental` | 非破坏性增量聚类 |
| POST | `/api/people/merge` | 合并两个人物（`{"source_id": x, "target_id": y}` ） |
| PATCH | `/api/people/:id` | 修改人物姓名和/或状态 |
| POST | `/api/people/batch-update` | 批量修改多个人物的状态 |
| GET | `/api/people/:id` | 人物下的照片（递归子树） |
| POST | `/api/people/:id/reparent` | 修改人物在树中的父节点 |
| GET | `/api/people/:id/merge-suggestions` | 按质心余弦距离排列的合并候选 |
| GET | `/api/people/:id/outlier-faces` | 距质心最远的人脸（可能误入） |
| POST | `/api/people/:id/eject-face` | 将人脸从该人物中移出 |
| GET | `/api/people/:id/centroid-faces` | 用于计算质心的人脸及距离分布统计 |

### 地理

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/geo/hierarchy` | 嵌套的国家 → 省/州 → 城市（含照片数） |
| POST | `/api/geo/regeocode` | 为未地理编码的照片触发后台反地理编码 |
| GET | `/api/geo/regeocode/status` | 轮询地理编码任务状态 |

### 动物

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/animals/species` | 动物种类列表（含照片数） |
| GET | `/api/animals/:species/photos` | 指定种类的照片列表 |
| GET | `/api/photos/:id/animals` | 照片中的动物检测结果 |

---

## PhotoBridge

PhotoBridge 是一个配套 Swift CLI，将 iCloud / Photos.app 照片库中的照片导出到暂存目录。如果 `picmanager` 在 `PATH` 中，每次导出后会自动调用 `picmanager import`。

### 前置条件

- macOS 13+（增量同步通过 PHPersistentChangeToken 需要 macOS 16+）
- Xcode 命令行工具（`xcode-select --install`）

### 编译

```bash
cd photobridge
swift build -c release
codesign --force --sign - \
  --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \
  .build/release/photobridge
```

> **照片权限说明：** macOS TCC 把授权弹框归属到**启动 photobridge 的终端 App**（iTerm2、Terminal.app 等），不是 photobridge 二进制本身。弹框显示"iTerm2 想要访问您的照片"，属于正常行为。授权后无需重启，直接再运行命令即可。
>
> 每次重新编译后都需要执行 `codesign` 步骤；未签名时 `requestAuthorization` 会直接返回 `denied`，不弹框。
>
> 如果弹框不出现（之前点了拒绝），重置对应终端 App 的照片权限后重试：
> ```bash
> tccutil reset Photos com.googlecode.iterm2   # iTerm2
> tccutil reset Photos com.apple.Terminal      # Terminal.app
> tccutil reset Photos com.microsoft.VSCode    # VS Code
> ```

### 首次配置

```bash
photobridge setup                          # 打印分步配置向导
photobridge setup --install-launchd        # 生成 launchd plist 实现自动同步
photobridge setup --install-launchd --interval-hours 3
# 激活：
launchctl load ~/Library/LaunchAgents/com.picmanager.photobridge-sync.plist
```

### 命令

**`export`** — 全量导出整个照片库：

```bash
photobridge export --dry-run                    # 仅统计数量
photobridge export                              # 导出 + 自动导入（若 picmanager 在 PATH 中）
photobridge export --output /Volumes/NAS/staging \
                   --picmanager /usr/local/bin/picmanager
```

**`sync`** — 增量导出（自上次同步后的新资产）：

```bash
photobridge sync
photobridge sync --dry-run
photobridge status                              # 上次同步时间和总数量
```

**`fix-timestamps`** — 修复已导出文件的 mtime/ctime：

```bash
photobridge fix-timestamps /path/to/staging/
photobridge fix-timestamps --dry-run /path/to/staging/
```

在自动时间戳功能加入之前导出的文件，使用此命令一次性修复。

**`export` 和 `sync` 共享选项：**

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--output <dir>` | `~/Library/Application Support/PhotoBridge/staging` | 暂存目录 |
| `--picmanager <path>` | （从 PATH 自动检测）| picmanager 可执行文件路径 |
| `--batch-size <n>` | 200 | 每批次 picmanager 导入照片数 |
| `--max-concurrent <n>` | 4 | 最大并发 iCloud 下载数 |
| `--dry-run` | — | 仅统计数量，不实际导出 |

### 时间戳机制

PhotoBridge 写入文件时，调用 `FileManager.setAttributes([.modificationDate:, .creationDate:])` 并使用 `PHAsset.creationDate`。picmanager 随后在 EXIF 无日期时以文件 mtime 作为回退——这样截图和无 EXIF 的 WhatsApp 图片也能落入正确的日期目录，而非 `library/unknown/`。

---

## 数据存储

```
~/Pictures/PicManager/
  picmanager.db            SQLite 数据库
  YYYY-MM-DD/              按日期整理的照片
  unknown/                 无法推断日期的照片
  .thumbs/                 缩略图缓存（自动生成，可安全删除）
```

原始照片文件**永远不会被修改**。数据库只存储元数据和状态。软删除的照片（来自去重）在数据库中标记为 `import_status='deleted'`；磁盘文件不会被删除。

---

## 开发

```bash
cargo nextest run                   # 运行全部测试（约 312 个通过，另有 1 个 #[ignore] 需要 yolov8n.onnx）
cargo clippy                        # 代码检查
python3 tests/make_fixtures.py      # 重新生成测试 fixture

# PhotoBridge 测试
cd photobridge
.build/debug/PhotoBridgeTestRunner  # 53 个测试
```
