# HEIC 方向问题完整手册

## 背景：方向的三个来源

从 iPhone 拍摄到 PicManager 显示，一张 HEIC 照片的"方向"经历三个独立层次：

| 层次 | 存储位置 | 由谁读取 |
|------|----------|----------|
| **传感器原始像素布局** | HEIC 文件像素数据 | image crate / sips |
| **EXIF Orientation tag** (0x0112) | HEIC 文件 EXIF metadata | kamadak-exif、exiftool |
| **HEIF IROT box** | HEIC 容器级旋转 | sips（转成 EXIF 写到输出 JPEG）、浏览器（部分） |
| **Photos 内部旋转修正** | Photos.app 私有 SQLite | Photos.app、PhotoKit 专用 |

---

## 各工具的实际行为（已验证）

### sips（macOS 系统工具）

```bash
sips -s format jpeg input.heic --out output.jpg
```

- **不旋转像素**。输出 JPEG 的像素布局与 HEIC 传感器数据完全相同。
- 输出 JPEG 的 EXIF Orientation 因 HEIC 内容不同而有两种情况：
  - **普通 iPhone HEIC（无 HEIF IROT box）**：将 HEIC 的 EXIF Orientation 原样复制到输出 JPEG。`sips -g orientation file.heic` 返回 `<nil>`，因为 sips 是通过读 IROT box 来获取方向的，iPhone HEIC 没有 IROT box。
  - **Photos 导出的 HEIC（有 HEIF IROT box）**：**将 IROT 翻译成 EXIF Orientation** 写入输出 JPEG（例如 IROT=Rotate90CW → 输出 EXIF=6），即使 HEIC 文件自身的 EXIF Orientation 字段是 1。此时 sips 输出的 EXIF 与原始 HEIC EXIF **不同**。
- `sips -s orientation N file.heic` **无效**（报错 "Cannot do --setProperty orientation"）。HEIC 的 EXIF 只能用 exiftool 修改。

### image crate（Rust）

- `image::open()` / `ImageReader::decode()` **不读取、不应用** EXIF Orientation tag。
- 返回的 `DynamicImage` 是传感器原始像素（与 sips 输出 JPEG 像素完全一致）。

### kamadak-exif

- `Reader::new().read_from_container(file)` 可以直接读取 HEIC 文件中的 EXIF Orientation（通过 ISOBMFF box 解析，不经过 sips）。
- 返回的是 HEIC 文件自身的 EXIF Orientation，**不受 HEIF IROT box 影响**。
- 这是 PicManager 读取 HEIC 方向的唯一正确入口。

### exiftool

```bash
exiftool -Orientation=1 -n -overwrite_original file.heic  # 修改 EXIF tag，不重新编码
exiftool -Orientation# file.heic                           # 读取原始数字值
```

- 可以**无损修改** HEIC 的 EXIF Orientation（只改 metadata，不重新编码像素）。
- sips 不行，只有 exiftool 能做这件事。
- exiftool 读/写的也是 EXIF Orientation，不是 HEIF IROT box。

### 浏览器（Chrome / Safari）

- 读取 JPEG 的 EXIF Orientation tag，**自动旋转展示**。
- 因此，PicManager 的 `GET /api/photos/{id}/file` 对 HEIC 不能直接返回 sips 输出——sips 输出可能携带从 IROT 翻译来的错误 EXIF（如 EXIF=6），浏览器会据此误旋转。

### Apple Photos.app

- **除 EXIF 外**，还维护一个内部旋转修正数据库（私有 SQLite）。
- 用户在 Photos 里手动旋转照片时，Photos **不修改 HEIC 文件**，只写内部数据库。
- PhotoKit API `PHImageManager.requestImageDataAndOrientation(version:.current)` 返回的 `CGImagePropertyOrientation` 是 Photos 内部修正叠加后的"真实显示方向"。
- 导出时，PhotoBridge 读取这个真实方向，用 exiftool 写入导出文件的 EXIF。

---

## 情况分类

### 情况 A：正常 iPhone HEIC（EXIF 与实际像素一致，无 IROT，无 Photos 内部修正）

**示例**：自拍人像，iPhone 竖拍，EXIF=6

```
传感器像素: 横版（人侧着）
EXIF tag:   6 = 旋转 90° CW → 竖版
HEIF IROT:  无
Photos 修正: 无
正确显示:   竖版人像 ✓
```

PicManager 处理：`read_exif_orientation(heic_path)` → 6，`apply_exif_orientation(img, 6)` → 竖版 ✓

---

### 情况 B：Photos 内部旋转修正（HEIC 文件 EXIF 与 Photos 显示不一致）

**示例**：横版照片，EXIF=6（iPhone 误判），用户在 Photos 里手动旋转修正

```
传感器像素: 横版
EXIF tag:   6（错误，iPhone 陀螺仪误判）
HEIF IROT:  无（Photos 内部修正不写入文件）
Photos 修正: 内部数据库记录 -90° CCW
Photos 显示: 横版 ✓
```

PhotoBridge 处理（`writeAssetResourceOrientationFixed`）：
1. 导出原始 HEIC（EXIF=6）
2. 向 Photos 查询真实显示方向（Photos 说：横版，orientation=1）
3. 用 exiftool 将 EXIF 改为 1
4. 导出后文件 EXIF=1，PicManager 处理正确 ✓

---

### 情况 C：EXIF=1（传感器像素与显示方向一致，无需旋转）

```
传感器像素: 与显示方向一致（横版）
EXIF tag:   1（无需旋转）
HEIF IROT:  无
正确显示:   直接展示像素 ✓
```

最简单情况，PicManager 无需任何变换。

---

### 情况 D：Photos 导出 HEIC 带 HEIF IROT box（sips 输出 EXIF ≠ HEIC 文件 EXIF）

**示例**：photo 9316，横版照，HEIC EXIF=1，HEIF IROT=Rotate90CW

```
传感器像素: 横版
HEIC EXIF:  1（PhotoBridge exiftool 修正后的正确值）
HEIF IROT:  Rotate90CW（Photos 内部记录，exiftool 不修改 IROT）
sips 输出:  EXIF=6（sips 将 IROT 翻译成 EXIF），像素仍是横版
浏览器直接展示 sips 输出: 将横版误旋转成竖版 ✗
```

**关键区别**：情况 B 的 Photos 修正不写 IROT box（Photos 内部 SQLite），PhotoBridge 用 exiftool 修复了 HEIC EXIF；情况 D 中 HEIF IROT box 已写入文件，exiftool 只修改了 EXIF，IROT 仍然存在，导致 sips 输出 EXIF ≠ HEIC EXIF。

PicManager 正确处理：用 `read_exif_orientation(heic_path)` 从 HEIC 文件读到 EXIF=1 → 无旋转 → 横版 ✓。**绝对不能从 sips 输出读 EXIF**（会读到 IROT 翻译出的 6，然后错误地旋转成竖版）。

---

## PicManager 中的处理逻辑

### 核心规则

**对 HEIC，始终从原始 HEIC 文件读取 EXIF Orientation（`read_exif_orientation(heic_path)`），不从 sips 输出读。**

原因：sips 会将 HEIF IROT box 翻译成 EXIF 写入输出（情况 D），导致输出 EXIF 与文件 EXIF 不一致。读 sips 输出会在 IROT 已存在时多旋转一次。

### 缩略图生成（`generate_thumb` in `photos.rs`）

```rust
// 1. open_image → sips 获取原始传感器像素（sips 不旋转）
let img = crate::image_open::open_image(p)?;

// 2. HEIC 从原始文件读 EXIF（不从 sips 输出读），非 HEIC 用 DB 值
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

### 原图端点（`get_photo_file` in `photos.rs`）

**HEIC 必须始终走 `apply_transforms_full`**，不能直接返回 sips 输出，原因：

1. sips 输出可能携带 IROT 翻译出的 EXIF（如 EXIF=6），浏览器据此误旋转
2. `apply_transforms_full` 读 HEIC 文件 EXIF → 烘焙进像素 → 输出不带 EXIF 的纯 JPEG
3. 浏览器收到无 EXIF 的 JPEG，按像素原样显示，方向正确

```rust
// HEIC（或有 DB rotation/flip）→ 方向烘焙进像素，返回无 EXIF 的 JPEG
if is_heic || rotation != 0 || flip_h || flip_v {
    apply_transforms_full(&path, exif_orient_u8, rotation, flip_h, flip_v)
}
```

`apply_transforms_full` 内部同样用 `read_exif_orientation(heic_path)` 读方向，与 `generate_thumb` 一致。

### DB `exif_orientation` 列

- 导入时从文件读取 EXIF tag 并写入 DB（migration 0012 后）
- migration 0012 前导入的照片默认值为 1
- HEIC 运行时方向读取直接从文件读，DB 值仅作 fallback（`unwrap_or(exif_orient)`）
- `open_image_with_orient(path)` 也从原始文件读（不从 sips 输出），保证与上述逻辑一致

---

## 修复方案

### 方案一：在 PhotoBridge 导出时修复（推荐，一劳永逸）

`writeAssetResourceOrientationFixed` 函数（`AssetExporter.swift`）：

1. 导出原始 HEIC 资源（`writeAssetResource`）
2. 向 Photos 查询该照片的真实显示方向（`requestImageDataAndOrientation(version:.current)`）
3. 用 `CGImageSource` 读取导出文件的 EXIF 方向
4. 若两者不一致，用 **exiftool** 无损修改文件 EXIF

```bash
exiftool -Orientation=1 -n -overwrite_original /path/to/file.heic
```

> exiftool 只修改 EXIF Orientation，不会删除 HEIF IROT box。但 PicManager 读的是 EXIF，所以 IROT 是否存在不影响正确性。

> exiftool 是 Homebrew 软件包，需要 `brew install exiftool`。未安装时函数静默跳过（不报错）。

---

### 方案二：修复 staging 目录中已导出但未导入的 HEIC

直接用 PhotoBridge 重新导出这批文件。更新后的 PhotoBridge 会自动修复方向：

```bash
cd photobridge
swift build -c release
codesign --force --sign - --entitlements Sources/PhotoBridge/PhotoBridge.entitlements .build/release/photobridge
.build/release/photobridge export --output /tmp/staging-fixed
```

---

### 方案三：修复已导入 PicManager 的 HEIC（逐张手动或批量）

#### 3a. 少量：用 PicManager Web UI 手动旋转

进入 http://localhost:8080，找到该照片，在详情模态框用旋转按钮调整。
这会在 DB 写入 `rotation` 值，并自动清除缩略图缓存。

#### 3b. 批量：exiftool 修复磁盘文件 + 更新 DB

```bash
# 1. 找出候选问题文件（exif_orientation != 1 的 HEIC）
sqlite3 /path/to/picmanager.db \
  "SELECT id, path, exif_orientation FROM photos
   WHERE format IN ('heic','heif')
     AND exif_orientation != 1
     AND import_status = 'imported';"

# 2. 人工确认方向错误后，用 exiftool 修改文件 EXIF
exiftool -Orientation=1 -n -overwrite_original /path/to/photo.heic

# 3. 更新 DB
sqlite3 /path/to/picmanager.db \
  "UPDATE photos SET exif_orientation = 1 WHERE id = <id>;"

# 4. 清除缩略图缓存
rm /path/to/.thumbs/<id>.jpg
```

---

## 已发现问题文件记录

| 文件 | Photo ID | HEIC EXIF | HEIF IROT | sips 输出 EXIF | 修复方式 | 修复日期 | 备注 |
|------|----------|-----------|-----------|--------------|----------|----------|------|
| 294BA16E-…_L0_001.heic | 9316 | 1（PhotoBridge 已修正）| Rotate90CW（仍存在）| 6（sips 翻译 IROT）| PicManager 读 HEIC EXIF=1，返回烘焙 JPEG | 2026-05-07 | 横版照，IROT 遗留但 PicManager 正确忽略 |

---

## 关键限制

1. **exiftool 不修改 HEIF IROT box**，只修改 EXIF Orientation。情况 D 中 IROT 仍会存在，但 PicManager 不依赖 sips 输出，所以不受影响。
2. **无法自动批量识别**哪些已导入照片有此问题，因为判断需要访问 Photos 内部数据库（只有 Photos.app 和 PhotoKit 能读）
3. **PhotoBridge 新代码需要 exiftool**（`brew install exiftool`）。未安装时静默跳过，但方向仍然错误
4. **HEIC 的 EXIF 只能用 exiftool 修改**，sips 不支持（报错 "Cannot do --setProperty orientation"）
5. 已导入的旧文件需要手动修复；新导出的文件由 PhotoBridge 自动处理

## 陷阱速查

| 陷阱 | 后果 | 正确做法 |
|------|------|----------|
| 从 sips 输出 JPEG 读取 EXIF Orientation | 情况 D：IROT 被翻译成错误 EXIF，多旋转一次 | 用 `read_exif_orientation(原始 HEIC 路径)` |
| 直接把 sips 输出 JPEG 返回给浏览器（`heic_to_jpeg` 直接返回）| sips 输出携带 IROT 翻译的 EXIF，浏览器误旋转 | 走 `apply_transforms_full`，烘焙方向，输出无 EXIF 的 JPEG |
| 用 `sips -s orientation` 修改 HEIC | 报错，无效 | 用 exiftool |
| 用 exiftool 修改 EXIF 后认为 IROT 也消失 | IROT 仍存在，sips 输出仍有错误 EXIF | PicManager 不用 sips 输出，无影响；但其他工具仍受影响 |
