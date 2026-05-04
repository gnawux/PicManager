# PhotoKit 调研笔记

> 调研日期：2026-05-04  
> 用途：iCloud 照片导入辅助程序（PhotoBridge）开发参考

## 1. PhotoKit 框架概述

PhotoKit 是 Apple 提供的官方相册框架，可在 macOS / iOS / iPadOS 上访问系统照片库。
关键类：

| 类 | 作用 |
|----|------|
| `PHPhotoLibrary` | 库访问入口，权限申请，变更订阅 |
| `PHAsset` | 代表一张照片/视频的元数据（不含图像数据本身） |
| `PHAssetCollection` | 相册/智能相册/时刻 |
| `PHFetchOptions` | 查询过滤与排序 |
| `PHImageManager` | 请求缩略图或图像数据 |
| `PHAssetResource` | 代表资产的底层文件（JPEG、RAW、MOV 等） |
| `PHAssetResourceManager` | 下载/写出资产文件数据 |

## 2. 关键能力边界

### 2.1 只能访问系统照片库

PhotoKit **只能访问 System Photo Library**（Photos 应用设置 → 通用 → 用作系统照片库的那个）。
用户若有多个 Photos Library，只有被设为系统库的那个可以通过 PhotoKit 访问。

### 2.2 iCloud Drive 与 iCloud Photos 不同

- **iCloud Photos**：Photos.app 专属同步服务，通过 PhotoKit 可访问
- **iCloud Drive 中的图片文件**：普通文件同步，**不可通过 PhotoKit 访问**，需要文件系统 API

### 2.3 云端照片下载与系统卷磁盘空间（重要！）

这是最关键的工程限制，经多方资料确认：

**结论：PhotoKit 无法在不影响系统照片库的情况下导出云端照片。**

- 通过 `PHAssetResourceManager.writeData(for:toFile:options:)` 或 `requestData()` 请求云端资产时，Photos 框架会先将原始文件下载到系统卷：
  - 临时缓存路径：`~/Library/Containers/com.apple.cloudphotosd/`
  - 可能写入主库：`~/Pictures/Photos Library.photoslibrary/`
- "优化 Mac 存储"（Optimize Mac Storage）开启时，Photos 会在磁盘空间紧张时自动驱逐本地副本，但**不保证及时驱逐**
- 已有用户报告：用 PhotoKit 导出 1.2TB iCloud 库时，系统卷被占满

**根本原因**：Photos 框架的下载行为由系统守护进程（`cloudphotosd`）控制，应用层无法绕过。

### 2.4 规避系统卷空间问题的方案

| 方案 | 说明 | 适用性 |
|------|------|--------|
| **将 Photos Library 迁移到外置卷** | Photos 偏好设置 → 移动库，再设为系统库。之后所有下载直接到外置卷，PhotoKit 操作无系统卷压力 | **推荐**（对 PicManager 场景最合适） |
| 小批量处理 + 依赖 Optimize Storage 驱逐 | 每批处理若干张，等 Photos 自动驱逐后再下一批 | 不可靠，速度慢 |
| 用 osxphotos `--ramdb` | 把库数据库放内存，减少系统卷 I/O，但仍无法阻止图像缓存 | 仅供参考 |

**对 PhotoBridge 的设计结论**：

应在文档 / 首次运行引导中明确告知用户：
> 建议先将 Photos Library 迁移至外置硬盘（与 PicManager 库同一卷），再运行 PhotoBridge 导入。
> 若库仍在系统卷，导入过程会临时占用系统卷额外空间（等于云端未下载照片的原始大小）。

## 3. Live Photo 格式说明

**HEIC 与 Live Photo 是独立概念，不要混淆：**

- **HEIC**（High Efficiency Image Container）：静态图像格式，使用 HEVC 编码，是 JPEG 的继任者。现代 iPhone 默认拍摄格式。
- **Live Photo**：一张静态图（HEIC 或 JPEG）+ 一段 ~3 秒 MOV 视频，**两个独立文件**，通过元数据关联。HEIC 文件里不包含 MOV 内容。

通过 PhotoKit 访问 Live Photo：

```swift
let resources = PHAssetResource.assetResources(for: asset)
// resources 包含：
//   .photo 类型       → 静态图（.heic 或 .jpg）
//   .pairedVideo 类型 → 配对视频（.mov）
```

判断是否为 Live Photo：

```swift
asset.mediaSubtypes.contains(.photoLive)
```

## 4. RAW + JPEG 双格式

一个 PHAsset 可同时包含 RAW 和 JPEG：

```swift
let resources = PHAssetResource.assetResources(for: asset)
// .photo              → JPEG（主版本）
// .alternatePhoto     → RAW（如 .dng / .arw / .nef）
```

导出时需分别对两个 resource 调用 `PHAssetResourceManager.writeData()`。

## 5. 连拍照片（Burst）

同一次连拍内所有照片共享 `PHAsset.burstIdentifier`。  
用户精选张通过 `PHAsset.burstSelectionTypes` 标识（`.userPick` = 用户手动选择的精选）。

查询精选张：

```swift
let options = PHFetchOptions()
options.predicate = NSPredicate(format: "burstIdentifier == %@", burstId)
// 筛选 burstSelectionTypes 包含 .userPick
```

## 6. 增量同步：变更历史（macOS 13+）

`PHPersistentChangeFetchRequest`（macOS 13 Ventura）允许查询上次运行以来的增量变更，不需要程序常驻。

```swift
// 保存令牌到磁盘
let token = PHPhotoLibrary.shared().currentChangeToken
// 下次运行时查询变更
let request = PHPersistentChangeFetchRequest(changeToken: savedToken)
let result = try PHPhotoLibrary.shared().fetchPersistentChanges(with: request)
```

低于 macOS 13 时，只能用 `PHPhotoLibraryChangeObserver`（需程序常驻）或全量扫描。

## 7. 权限模型

- `Info.plist` 需声明 `NSPhotoLibraryUsageDescription`（读取权限）
- 首次访问时系统弹出授权对话框，用户选择"完整访问"或"受限访问"
- macOS 14+ 新增"受限访问"模式（只允许访问用户选择的相册），建议 PhotoBridge 引导用户授予完整访问

## 8. 元数据可访问性

| 字段 | 访问方式 | 备注 |
|------|----------|------|
| 拍摄时间 | `PHAsset.creationDate` | |
| 修改时间 | `PHAsset.modificationDate` | |
| GPS 坐标 | `PHAsset.location` | |
| 媒体类型 | `PHAsset.mediaType` / `mediaSubtypes` | 含 HDR/全景/Live |
| 相机型号 | 通过 CoreImage 读取 EXIF | PhotoKit 不直接暴露 |
| 关键字 | **不可访问** | PhotoKit 公开 API 不提供 |
| 人物标签 | **不可访问** | Photos.app 私有 |

## 9. 参考资源

- [PhotoKit 官方文档](https://developer.apple.com/documentation/photokit)
- [PHAssetResourceManager](https://developer.apple.com/documentation/photos/phassetresourcemanager)
- [PHPersistentChangeFetchRequest](https://developer.apple.com/documentation/photos/phpersistentchangefetchrequest)
- [osxphotos（Python PhotoKit 封装，生产级参考实现）](https://github.com/RhetTbull/osxphotos)
- [WWDC22: Discover PhotoKit change history](https://developer.apple.com/videos/play/wwdc2022/10132/)
- [WWDC21: Improve access to Photos in your app](https://developer.apple.com/videos/play/wwdc2021/10046/)
