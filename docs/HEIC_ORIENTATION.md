# HEIC 方向问题完整手册

## 背景：方向的三个来源

从 iPhone 拍摄到 PicManager 显示，一张 HEIC 照片的"方向"经历三个独立层次：

| 层次 | 存储位置 | 由谁读取 |
|------|----------|----------|
| **传感器原始像素布局** | HEIC 文件像素数据 | image crate / sips |
| **EXIF Orientation tag** (0x0112) | HEIC 文件 EXIF metadata | kamadak-exif、exiftool、浏览器 |
| **Photos 内部旋转修正** | Photos.app 私有 SQLite | Photos.app 专用 |

三层叠加才是用户眼中"正确"的显示方向。

---

## 各工具的实际行为（已验证）

### sips（macOS 系统工具）

```bash
sips -s format jpeg input.heic --out output.jpg
```

- **不旋转像素**。输出 JPEG 的像素布局与 HEIC 传感器数据完全相同。
- **复制 EXIF Orientation tag** 原样到输出 JPEG。
- `sips -g orientation file.heic` 返回 `<nil>`：sips 读的是 HEIF 容器里的 IROT box，但 iPhone HEIC 方向存在 EXIF tag，不写 IROT box，所以读不到。
- `sips -s orientation N file.heic` **无效**（报错 "Cannot do --setProperty orientation"）。HEIC 的 EXIF 只能用 exiftool 修改。

### image crate（Rust）

- `image::open()` / `ImageReader::decode()` **不读取、不应用** EXIF Orientation tag。
- 返回的 `DynamicImage` 是传感器原始像素（与 sips 输出 JPEG 像素完全一致）。

### exiftool

```bash
exiftool -Orientation=1 -n -overwrite_original file.heic  # 修改 EXIF tag，不重新编码
exiftool -Orientation# file.heic                           # 读取原始数字值
```

- 可以**无损修改** HEIC 的 EXIF Orientation（只改 metadata，不重新编码像素）。
- sips 不行，只有 exiftool 能做这件事。

### 浏览器（Chrome / Safari）

- 读 JPEG/HEIC 的 EXIF Orientation，**自动旋转展示**。
- 所以 `GET /api/photos/{id}/file` 返回带 EXIF 的原始字节，浏览器方向正确。

### Apple Photos.app

- **除 EXIF 外**，还维护一个内部旋转修正数据库（私有 SQLite）。
- 用户在 Photos 里手动旋转照片时，Photos **不修改 HEIC 文件**，只写内部数据库。
- PhotoKit API `PHImageManager.requestImageDataAndOrientation(version:.current)` 返回的 `CGImagePropertyOrientation` 是 Photos 内部修正叠加后的"真实显示方向"。

---

## 情况分类

### 情况 A：正常（EXIF 与实际像素一致，无 Photos 内部修正）

**示例**：自拍人像，iPhone 竖拍，EXIF=6（需 CW 旋转显示竖版）

```
传感器像素: 横版（人侧着）
EXIF tag:   6 = 旋转 90° CW → 竖版
Photos 修正: 无
正确显示:   竖版人像 ✓
```

PicManager 处理：`apply_exif_orientation(img, 6)` = 旋转 CW → 竖版 ✓

---

### 情况 B：Photos 内部旋转修正（EXIF 与 Photos 显示不一致）

**示例**：294BA16E，狗的横版照，EXIF=6 但 Photos 内部有修正

```
传感器像素: 横版（狗站着）
EXIF tag:   6 = 旋转 90° CW → 竖版（但这是"错的"）
Photos 修正: -90° CCW（抵消 EXIF，让照片显示为横版）
Photos 显示: 横版 ✓
预览/QuickLook: 竖版 ✗（只用 EXIF，不知道 Photos 修正）
```

产生原因：
- iPhone 拍摄时陀螺仪误判方向，写入了错误 EXIF
- 用户之后在 Photos 里手动旋转修正
- Photos 不修改 HEIC 文件，只写内部数据库

---

### 情况 C：无 EXIF tag（EXIF=1 或 tag 缺失）

```
传感器像素: 与显示方向一致
EXIF tag:   1（无需旋转）或缺失
Photos 修正: 无
正确显示:   直接展示像素 ✓
```

最简单情况，无需任何处理。

---

## PicManager 中的处理逻辑

### 缩略图生成（`generate_thumb` in `photos.rs`）

```rust
// 1. open_image 返回原始传感器像素（sips 不旋转）
let img = crate::image_open::open_image(p)?;

// 2. 读取有效方向：HEIC 从文件直接读，确保不用 migration 0012 前的默认值
let effective_orient = if crate::image_open::is_heic(p) {
    crate::image_open::read_exif_orientation(p).unwrap_or(exif_orient)
} else {
    exif_orient
};

// 3. EXIF 方向先于 resize 应用，保证裁剪在显示空间进行
let img = apply_exif_orientation(img, effective_orient);
let thumb = img.resize_to_fill(size, size, ...);

// 4. 用户手动旋转/翻转叠加
let thumb = apply_transform(thumb, rotation, flip_h, flip_v);
```

**注意**：HEIC 文件的 EXIF tag 直接从磁盘读取（不完全信任 DB），因此修复朝向问题必须修改磁盘上的文件。

### DB `exif_orientation` 列

- 导入时从文件读取 EXIF tag 并写入 DB（migration 0012 后）
- migration 0012 前导入的照片默认值为 1
- HEIC 缩略图生成时直接从文件读取，DB 值仅作 fallback

---

## 修复方案

### 方案一：在 PhotoBridge 导出时修复（推荐，一劳永逸）

`writeAssetResourceOrientationFixed` 函数（`AssetExporter.swift`）：

1. 导出原始 HEIC 资源（`writeAssetResource`）
2. 向 Photos 查询该照片的真实显示方向（`requestImageDataAndOrientation(version:.current)`）
3. 用 `CGImageSource` 读取导出文件的 EXIF 方向
4. 若两者不一致，用 **exiftool** 无损修改文件 EXIF

exiftool 使用场景：
```bash
exiftool -Orientation=1 -n -overwrite_original /path/to/file.heic
```

> exiftool 是 Homebrew 软件包，需要 `brew install exiftool`。未安装时函数静默跳过（不报错）。

---

### 方案二：修复 staging 目录中已导出但未导入的 HEIC（批量脚本）

对于 PhotoBridge 已导出到 staging 但尚未 `picmanager import` 的文件，运行：

```bash
#!/bin/bash
# fix-staging-orientation.sh
# 需要 exiftool: brew install exiftool

STAGING_DIR="$1"
PHOTOS_LIBRARY="${2:-$HOME/Pictures/Photos Library.photoslibrary}"

if [[ -z "$STAGING_DIR" ]]; then
    echo "Usage: $0 <staging_dir>"
    exit 1
fi

# 遍历 staging 目录的所有 HEIC 文件
find "$STAGING_DIR" -name "*.heic" -o -name "*.HEIC" | while read -r file; do
    # 文件名即 localIdentifier（斜杠替换为下划线），去掉扩展名
    # 格式: B5A8F3C2-1234-5678-ABCD-000000000001_L0_001.heic
    # 对应: B5A8F3C2-1234-5678-ABCD-000000000001/L0/001
    local_id=$(basename "$file" .heic | tr '_' '/')
    local_id=$(basename "$local_id" .HEIC)

    file_orient=$(exiftool -Orientation# -s3 "$file" 2>/dev/null)
    if [[ -z "$file_orient" || "$file_orient" == "1" ]]; then
        continue  # 已经是 1 或无 EXIF，跳过
    fi

    # 用 osascript 查询 Photos 中该 asset 的真实方向
    # （只适用于本机 Photos Library 可访问的情况）
    echo "Checking $file (EXIF=$file_orient)..."
done
```

**实际操作**（更简单）：直接用 PhotoBridge 重新导出这批文件。更新后的 PhotoBridge 会自动修复方向：

```bash
cd photobridge
swift build -c release
codesign --force --sign - --entitlements Sources/PhotoBridge/PhotoBridge.entitlements .build/release/photobridge
# 删除 staging 中的旧文件（或换一个空 staging 目录）
.build/release/photobridge export --output /tmp/staging-fixed
```

---

### 方案三：修复已导入 PicManager 的 HEIC（逐张手动或批量）

#### 3a. 针对单张（少量）：用 PicManager Web UI 手动旋转

进入 http://localhost:8080，找到该照片，在详情模态框用旋转按钮调整：

- 若显示为顺时针旋转 90°（应横但显示竖）：点一次 "逆时针 90°"
- 若显示为逆时针旋转 90°：点一次 "顺时针 90°"

这会在 DB 写入 `rotation` 值，并自动清除缩略图缓存。

#### 3b. 针对批量：直接用 exiftool 修复磁盘文件 + 更新 DB

```bash
# 1. 找出所有 exif_orientation != 1 的 HEIC 照片（候选问题文件）
sqlite3 /path/to/picmanager.db \
  "SELECT id, path, exif_orientation FROM photos
   WHERE format IN ('heic','heif')
     AND exif_orientation != 1
     AND import_status = 'imported';"
```

对于每张确认方向错误的照片（需人工逐一确认，因为大多数 EXIF!=1 的 HEIC 是正确的）：

```bash
# 2. 用 exiftool 修改文件 EXIF
exiftool -Orientation=1 -n -overwrite_original /path/to/photo.heic

# 3. 更新 DB（id 替换为实际值）
sqlite3 /path/to/picmanager.db \
  "UPDATE photos SET exif_orientation = 1 WHERE id = <id>;"

# 4. 清除缩略图缓存
rm /path/to/.thumbs/<id>.jpg
```

**注意**：不能用 `sips -s orientation` 修改 HEIC，只有 exiftool 支持。

---

## 已发现问题文件记录

| 文件 | Photo ID | 原 EXIF | 正确 EXIF | 修复日期 | 备注 |
|------|----------|---------|-----------|----------|------|
| 294BA16E-…_L0_001.heic | 9316 | 6 | 1 | 2026-05-05 | 狗的横版照，用户在 Photos 里修正过方向 |

---

## 关键限制

1. **无法自动批量识别**哪些已导入照片有此问题，因为判断需要访问 Photos 内部数据库（只有 Photos.app 和 PhotoKit 能读）
2. **PhotoBridge 新代码需要 exiftool**（`brew install exiftool`）。未安装时静默跳过，不会报错，但方向仍然错误
3. **HEIC 的 EXIF 只能用 exiftool 修改**，sips 不支持（会报错 "Cannot do --setProperty orientation"）
4. 已导入的旧文件需要手动修复；新导出的文件由 PhotoBridge 自动处理
