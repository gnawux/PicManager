# PicManager

家庭照片与图片管理工具。支持自动整理、去重、按时间/地点/相机分相册，提供 Web 界面和命令行界面。

## 技术栈

**PicManager（Rust）**
- 语言：Rust（edition 2024，当前工具链 1.95+）
- HTTP 框架：Axum 0.8
- 数据库：SQLite via sqlx 0.8（编译期查询检查）
- 异步运行时：tokio
- EXIF 解析：kamadak-exif 0.5
- 图像处理：image 0.25 + image_hasher 3
- 静态文件：rust-embed 8（前端 + 可选 ONNX 模型编译进二进制）
- CLI：clap 4
- ONNX 推理：ort 2.0.0-rc.12（download-binaries + coreml + ndarray features；macOS 上自动使用 CoreML / ANE）
- 数值计算：ndarray 0.17

**PhotoBridge（Swift）**
- 语言：Swift 6，Swift Package Manager
- Photos 框架：PhotoKit（PHPhotoLibrary、PHAsset、PHAssetResource、PHPersistentChangeToken）
- 最低平台：macOS 13（incrementalEnumerate 需要 macOS 16+）
- CLI：ArgumentParser 1.5
- 测试：自定义 test runner（executableTarget，无 XCTest）

## 实际项目结构

```
src/
  main.rs              CLI 入口（import [--copy] / dedup / serve / config / models fetch|bundle）
  lib.rs               库根；EmbeddedModels（rust-embed，models/ 目录）；get_embedded_model(name)
  config.rs            Config 结构体（含 thumb_cache_dir）；配置文件 ~/Library/Application Support/picmanager/config.toml
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
    mod.rs             scan(), scan_full(), list_groups(), resolve()
    hash.rs            compute_phash(), hamming_distance()
    candidate.rs       scan()增量扫描，scan_full()多索引分桶扫描，写入 dedup_groups
  album/
    mod.rs
    organize.rs        group_by_month(), group_by_camera()
    location.rs        group_by_location()，Nominatim 反地理编码 + geocache
    merge.rs           merge(source_id, target_id)
  storage/
    mod.rs
    db.rs              connect()，运行迁移
  face/
    mod.rs             analyze_one(pool, photo_id, img)：检测+嵌入，导入时调用
    detector.rs        detect(img) -> Vec<FaceRegion>；OnceLock<Mutex<Session>> 懒加载
    embedder.rs        Embedder::load(path)/extract(img, region) -> Vec<f32> 512D L2归一化
    job.rs             run_job(pool, scope) -> job_id；execute_job() pub(crate) 供测试调用
    cluster.rs         DBSCAN 聚类（cosine 距离），run_clustering(pool)
  animal/
    mod.rs             detect_and_save(pool, photo_id, img)，模型不存在时静默跳过
    detector.rs        detect(img) -> Vec<AnimalDetection>；YOLOv8-nano，OnceLock<Mutex<Session>>
  web/
    mod.rs             AppState, router(), serve()
    embed.rs           rust-embed 静态文件服务
    handlers/
      import.rs        POST /api/import（body: {dir, copy?}），GET /api/import/status
      photos.rs        GET /api/photos, GET /api/photos/{id}/thumb, GET /api/photos/{id}/file, GET /api/photos/{id}, PATCH /api/photos/{id}, POST /api/photos/batch-update, GET /api/photos/gps-points
      dedup.rs         GET /api/dedup, POST /api/dedup/{group_id}/resolve
      albums.rs        GET /api/albums（含 latest_photo_at 字段）, GET /api/albums/{id}/photos, POST /api/albums/merge
      collections.rs   GET /api/collections, POST /api/collections, PATCH/DELETE /api/collections/{id}, GET/POST/DELETE /api/collections/{id}/photos
      faces.rs         POST /api/faces/analyze, GET /api/faces/jobs/{id}, GET /api/photos/{id}/faces
      people.rs        GET /api/people（含 status/name_exact 过滤）, PATCH /api/people/{id}, POST /api/people/batch-update, GET /api/people/tree, POST /api/people/cluster, POST /api/people/merge, GET /api/people/{id}, POST /api/people/{id}/reparent, GET /api/faces/{id}/thumb
      geo.rs           GET /api/geo/hierarchy
      animals.rs       GET /api/animals/species, GET /api/animals/{species}/photos, GET /api/photos/{id}/animals
frontend/              HTML + CSS + JS（编译进二进制，不依赖运行时工作目录；架构详见 docs/FRONTEND.md）
migrations/
  0001_initial.sql     photos, albums, photo_albums, dedup_groups, dedup_members, import_sessions
  0002_geocache.sql    geocache 表（GPS 坐标 → 城市名缓存）
  0003_faces.sql       faces 表（人脸区域 + embedding BLOB）、face_jobs 表
  0004_photo_stats.sql photo_stats 单行计数器表（active_count，避免全表 COUNT(*)）
  0005_dedup_incremental.sql photos.dedup_scanned_at 列（增量 dedup 扫描标记）
  0006_timezone_offset.sql photos.timezone_offset 列（UTC 偏移分钟数）
  0007_people.sql      people 表（人物记录）、person_faces 表（人物-人脸多对多）
  0008_geocache_hierarchy.sql geocache 新增 country/state/county 列
  0009_animals.sql     animals 表（动物检测结果，photo_id/species/confidence/bbox）
  0010_people_status.sql  people.status 列（active / ignored / not_a_person）
tests/
  web_api.rs           Web API 集成测试（tower::ServiceExt::oneshot）
  fixtures/            测试 fixture JPEG 文件（由 make_fixtures.py 生成）
  samples/             真实照片样本（详见下方"测试样本照片"一节）
  make_fixtures.py     生成所有 fixture（Pillow + 原始 EXIF 二进制写入）
docs/
  REQUIREMENTS.md      需求文档
  PLAN.md              开发计划（Steps 1–22 已全部完成）
  ARCHITECTURE.md      架构设计
  DESIGN.md            详细设计（模块接口、DB schema、API 参考、测试策略）
photobridge/           iCloud Photos 导出伴侣工具（Swift Package）
  Sources/PhotoBridge/
    PhotoBridgeCommand.swift  CLI 入口（export / sync / status / fix-timestamps / setup）
    Commands/
      ExportCommand.swift     全量导出 + 时间戳 + 磁盘预检 + picmanager 自动导入
      SyncCommand.swift       增量同步（PHPersistentChangeToken）+ 同上
      StatusCommand.swift     显示上次同步时间与数量
      FixTimestampsCommand.swift  修复已导出文件的 mtime/ctime
      SetupCommand.swift      首次配置向导 + launchd plist 生成
  Sources/PhotoBridgeLib/
    LibraryEnumerator.swift   全量枚举：PHAsset + selectExportResource
    IncrementalEnumerator.swift  增量枚举：PHPersistentChangeFetchResult
    AssetExporter.swift       exportDestinationURL() + writeAssetResource()
    AssetTimestamp.swift      applyTimestamp(to:date:)
    AssetEnumerator.swift     AssetResourceInfo 协议
    DiskSpaceCheck.swift      estimatedBytes / freeBytes / checkDiskSpace
    PicManagerRunner.swift    parseImportLog() + importBatch() 子进程调用
    PhotoLibraryAuth.swift    requestPhotoLibraryAccess()
    SyncState.swift           SyncState（Codable，持久化到 Application Support）
  Tests/PhotoBridgeTestRunner/  自定义测试 runner（executableTarget，53 个测试）
```

## 开发原则

- 照片安全优先：去重删除需人工确认（软删除 `import_status='deleted'`），不自动操作文件
- 导入默认 **移动** 文件到库目录；`--copy` 保留源文件
- 数据库 `photos.path` 存储库内最终路径，不是源路径
- 单文件导入失败只记录 `tracing::warn`，不中断整批次
- **文档同步**：每次功能性调整（行为变更、新增、删除）完成后，必须确认 `docs/REQUIREMENTS.md`、`docs/DESIGN.md`、`docs/ARCHITECTURE.md`、`README.md`、`CLAUDE.md` 是否需要更新，需要的必须同步修改后再提交

## 开发状态

**已完成：Steps 1–22（docs/PLAN.md 全部步骤）**

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
| 14a | DB Schema 扩展：faces + face_jobs 表 |
| 14b | 人脸检测模块（ultraface-slim-320 / ort 2.x） |
| 14c | 人脸特征提取模块（ArcFace MobileNetV1 / ort 2.x） |
| 14d | 导入集成 + 批量重分析 API + CLI |
| 15a | 缩略图磁盘缓存 + spawn_blocking（`{library}/.thumbs/{id}.jpg`） |
| 15b | COUNT(*) 替换为 photo_stats 计数器表 |
| 16a | dedup 增量扫描（photos.dedup_scanned_at 标记，新照片才比较） |
| 16b | dedup 全量重扫多索引分桶（4×16bit 段，pigeonhole 保证零漏报） |
| 17a | DB: photos.timezone_offset 列 |
| 17b | 照片时间/时区编辑 API（PATCH /api/photos/{id} + 批量更新） |
| 17c | 前端：照片详情编辑 + 批量时间调整 |
| 18a | DB: people + person_faces 表 |
| 18b | 人脸 DBSCAN 聚类（face/cluster.rs，cosine 距离） |
| 18c | 人物 API（list/tree/cluster/merge/reparent/face-thumb） |
| 18d | 前端：人物标签页（聚类、详情、子人物树、合并） |
| 19a | DB: geocache 层级字段 + GET /api/geo/hierarchy |
| 19b | 前端：地点层级列表（三列钻取） |
| 19c | 前端：Leaflet 地图打点 + GET /api/photos/gps-points |
| 20a | DB: animals 表 + YOLOv8-nano 导入集成（src/animal/） |
| 20b | 动物 API + 前端动物标签页 |
| 22a | DB: people.status 列（0010 迁移）+ PATCH /api/people/{id} + POST /api/people/batch-update + GET /api/people 支持 status/name_exact 过滤 |
| 22b | 前端：内联姓名编辑 + "…" 右键菜单（忽略 / 标记非人物） |
| 22c | 前端：多选人物 + 批量操作栏（命名合并 / 批量忽略 / 批量标记非人物） |
| 22d | 前端：客户端撤销栈（↩ 撤销按钮，patch 逆操作） |
| 22e | 前端：人物详情页树状编辑（更改父节点 / 子人物面板） |
| 22f | 前端：重复姓名检测弹窗（面孔缩略图对比，选择合并或保留重名） |
| 23a | importer: SharedImportProgress + import_dir_with_progress（原子计数器，per-file 更新） |
| 23b | CLI import 进度循环：每 5 秒检查、每 60 秒打印进度 + 耗时汇总 |
| 24a/b | import_one → Option<(photo_id, face_count)>；faces_found/geo_total/geo_done 进度追踪；group_by_location_scoped 限定新导入照片范围 |
| 24c | CLI 三段式进度格式：导入 / 人脸 / 地理分开显示 |
| 25a | face/cluster: run_incremental_clustering（非破坏性增量聚类，TDD） |
| 25b | web: POST /api/people/cluster/incremental；import 完成后自动触发增量聚类 |
| 25c | 前端：工具栏改为「整合新面孔」主按钮 + 「⚠️ 全量重建」次要按钮（带确认对话框） |
| 26a | 后端：GET /api/albums 新增 latest_photo_at 字段（MAX(p.taken_at)，TDD） |
| 26b | 前端：相册侧边栏分三类（设备/时间/地点）折叠展示，按最新照片时间排序，默认显示 4 个 + "更多/收起" |
| 27a | 后端：GET /api/people/{id} 改用 WITH RECURSIVE CTE 包含子树所有后代照片（TDD） |
| 27b | 后端：PersonNode 增加 cover_face_id 字段，GET /api/people/tree 返回（TDD） |
| 27c | 前端：人物列表只显示顶级节点；详情页照片分页（50/页，上/下页按钮）；子人物行加缩略图可点击 |
| 28  | 照片详情模态框展示人物：GET /api/photos/{id}/faces 增加 person_id/person_name（TDD）；前端渲染人物缩略图区 |
| 29a | 前端：人物列表按有名/无名排序，有子节点的显示子节点数量；无人脸人物使用 default-person.svg 占位图 |
| 29b | 后端：GET /api/photos/{id}/file 返回原始文件字节（Content-Type 依 format 列推断）；前端详情模态框增加"查看原图/切换缩略图"切换按钮 |
| 30a | 文档：邻近地理编码缓存（proximity geocache）设计写入 REQUIREMENTS/ARCHITECTURE/DESIGN |
| 30b | 后端：`cached_or_fetch` 精确 key 未命中时先查 ±0.01°（约 1km）邻近缓存，命中则写回精确 key 并返回（TDD，6 个新测试） |
| 31a | importer/log.rs：MigrationLog（NDJSON，TDD，5 个测试） |
| 31b | import_dir_batch()：batch_size/log/dry_run 支持（TDD，6 个测试） |
| 31c | CLI: --batch-size / --log / --dry-run 参数 |
| 31d | 文档更新：REQUIREMENTS.md 新增大批量分批迁移节 |
| 32a | 前端：人物列表两组（命名/未命名）均按 photo_count 降序排列 |
| 32b | 后端：GET /api/people/{id}/merge-suggestions（质心余弦距离，TDD） |
| 32c | 前端：已命名人物详情页「建议合并」面板 |
| 32d | 后端：GET /api/people/{id}/outlier-faces + POST /api/people/{id}/eject-face（TDD） |
| 32e | 前端：人物详情页「⚠ 可能误入的照片」面板（移出/保留） |
| 32f | 文档更新：REQUIREMENTS.md 新增人物聚类辅助功能节，DESIGN.md 新增 3 个 API 接口 |
| 32g | 质心算法：置信度优先过滤（≥0.85 → ≥0.70 → 全部，至少10张），排除暗光/畸变照片；GET /api/people/{id}/centroid-faces 返回距离分布统计（min/p25/median/p75/max）；离群阈值 0.20→0.50；合并建议上限 5→10 |
| 32h | 前端 UI 优化：建议合并/可能误入面板默认折叠；质心照片 meta 浅黄底色；centroid-stats 移入 outlier panel；离群脸诊断模态窗（min_dist=0 全量排序，最多40张）；调试模态窗多选+创建子人物；面包屑父节点可点击导航；#people-detail-empty CSS 特异性修复 |
| 32i | 文档更新：REQUIREMENTS.md 更新聚类辅助功能节（新增质心算法说明、诊断功能描述），DESIGN.md 更新 4 个 API 接口（含 centroid-faces） |
| 33a | DB migration 0011（rotation/flip_h/flip_v 列）；PATCH /api/photos/{id} + batch-update 支持 rotation_delta/flip_h_toggle/flip_v_toggle；apply_transform + generate_thumb 应用变换；删除旧缩略图缓存（TDD，5 个新测试 + 2 个单元测试） |
| 33b | 前端：详情模态框 + 批量操作栏各增 4 个旋转/翻转按钮；applyPhotoTransform / applyBatchTransform，成功后立即刷新缩略图 |
| 33c | 文档更新：REQUIREMENTS.md 新增旋转/翻转需求，DESIGN.md 更新 PATCH / batch-update API 字段 |
| 34a | DB migration 0012（exif_orientation 列）；metadata::exif 读取 EXIF Orientation tag；PhotoMeta 新增 exif_orientation 字段；导入时写入 DB（TDD，3 个新测试） |
| 34b | face/mod.rs：apply_exif_orientation（EXIF 1-8 → rotation/flip_h）+ apply_transform（从 photos.rs 迁入）；analyze_one 检测前读 DB 方向字段并变换到显示空间；photos.rs generate_thumb 同样应用 EXIF + DB 两层变换；people.rs crop_face 同样应用两层变换后裁剪（TDD，5 个单元测试 + 1 个集成测试） |
| 34c | 文档更新：REQUIREMENTS.md 新增方向感知检测需求，DESIGN.md/ARCHITECTURE.md 同步更新，CLAUDE.md 记录 image crate 不自动应用 EXIF Orientation |
| 35a | fix: 人物合并父→子节点时子节点升级到父节点位置（递归 CTE 祖先检测 + 事务）；migration 0013 修复已有自引用孤儿节点 |
| 35b | fix: 照片旋转/翻转后自动后台触发人脸重分析；提取 reanalyze_one_photo（修复 cover_face_id FK 静默失败 bug）；`picmanager faces analyze --rotated-only` 修复历史数据 |
| 35c | feat: 人物合并操作（建议合并面板 + 合并到…对话框）加通用确认弹窗，显示源→目标名称及不可撤销提示 |
| 36a | feat: 精选集后端 CRUD（GET/POST /api/collections，PATCH/DELETE /api/collections/{id}）；albums 表复用 kind='curated'；REQUIREMENTS.md 新增精选集节；4 个 TDD 测试 |
| 36b | feat: 精选集照片成员管理（POST/DELETE /api/collections/{id}/photos，GET /api/collections/{id}/photos）；5 个 TDD 测试 |
| 36c | feat: 前端精选集侧边栏（创建/改名/删除）；loadCollections/selectCollection；loadPhotos 支持 inCollection 状态 |
| 36d | feat: 前端批量加入/移除精选集；add-to-collection picker 弹窗；工具栏"精选"按钮（整相册加入） |
| 36e | docs: DESIGN.md 新增 7 个 API 端点，ARCHITECTURE.md 新增 collections.rs，CLAUDE.md 更新 |
| 37a | fix(dedup): 两层去重架构 + 时间感知阈值；degenerate hash 过滤（对称双侧）；DCT pHash Layer 2 验证 |
| 37b | fix(dedup): Union-Find 聚类 — 连拍 n 张合并为 1 组而非 C(n,2) 对组；scan_full 扫前清除旧 pending 组 |
| 37c | fix(dedup): SIMILARITY_THRESHOLD_FAR 8→3；连拍/远距离独立 Union-Find 防止连拍组被结构相似链污染 |
| 38a | feat(dedup): CLI 只扫描报告组数，移除终端交互提示，重定向到 Web 界面确认 |
| 38b | feat(dedup): GET /api/dedup 返回 filename/width/height 字段；migration 0014；import_one 存储尺寸；3 个 TDD 测试 |
| 38c | feat(dedup): Web UI dedup 列表展示完整文件名 + 尺寸徽章 + 拍摄日期/相机信息；移除文件名截断 |
| 38d | feat(dedup): Web UI 全屏比较模态框，加载原图并排展示，点击选择保留项并与主模态双向同步 |

当前测试数：**312 个**（`cargo nextest run` 全部通过，另有 1 个 `#[ignore]` 需 yolov8n.onnx）

## 关键实现细节（避免踩坑）

### 两层去重架构（dedup）

**Layer 1**（`dedup/hash.rs` + `candidate.rs`）：`image_hasher::HashAlg::Gradient` 64-bit pHash，O(n log n) 多索引分桶粗筛。

**Layer 2**（`dedup/hash.rs::compute_dcthash`）：自实现 DCT pHash — 图像缩至 32×32 灰度，Row-wise + Column-wise DCT-II，取左上 8×8 低频系数，各值与均值比较生成 64-bit hash。仅对 Layer 1 候选对调用，按照片 ID 缓存（`HashMap<i64, Option<u64>>`）避免重复计算。

**Union-Find 聚类**（`candidate.rs`）：两层都通过的候选对收集完后，用带路径压缩的 Union-Find 将传递相似照片合并为连通分量，每个分量写一个 DB 组。`scan_full` 调用 `write_clusters`，`scan` 调用 `write_clusters_incremental`（合并已有 pending 组）。

**关键行为**：
- Layer 2 图像读取失败时返回 `None` → 跳过第二层（不过滤），防止误判；测试中合成路径正是依赖此行为
- `different.jpg` vs `with_exif.jpg` 的 DCT 距离为 6，但此对的 Gradient 距离 > 10，根本不进入 Layer 2，与 `DCT_THRESHOLD=8` 无冲突
- 时间感知阈值：`taken_at` 差值 ≤ 60 s 用宽松阈值（10），否则用严格阈值（8）；NULL 当作远距离处理
- `is_degenerate` 双侧对称：set bits < 10（过稀疏）或 > 54（过密集）均视为退化，两者都会产生假阳性
- `scan_full` 扫描前先 `DELETE dedup_groups WHERE status='pending'`，清除上次扫描的旧结果再重建
- `SIMILARITY_THRESHOLD_FAR = 3`（远距离对阈值）：只匹配 dist ≤ 3 的远距离对（原图 + App 处理副本），防止结构相似的户外照片被链式合并成大组
- 连拍对与远距离对独立 Union-Find：连拍照片所在的簇不会被远距离结构相似对污染（`burst_components` + `far_components`）

### image crate 与 sips 均不自动应用 EXIF Orientation

`image::open()` / `ImageReader::decode()` **不会**自动应用 EXIF Orientation tag（0x0112）。

**sips 行为**（macOS）：`sips -s format jpeg input.heic --out output.jpg` 同样**不旋转像素**——它只是把 HEIC 文件里的 EXIF Orientation 标签原样复制到输出 JPEG，像素布局与传感器原始方向保持一致。（`sips -g orientation` 显示 `<nil>` 是因为 sips 通过 HEIF 容器属性读取方向，而 iPhone HEIC 只写 EXIF Orientation，不写 HEIF IROT box，所以 sips 的 `-g orientation` 读不到）。

浏览器（Chrome）读 JPEG 的 EXIF Orientation 标签后自动旋转展示——这是为什么"查看原图"方向正确，但用 image crate 加载后处理却不对的原因。

因此，所有代码路径均需手动处理两层变换：
1. 用 kamadak-exif 读取 `Tag::Orientation`（1–8）
2. 调用 `face::apply_exif_orientation(img, orientation)` 旋转/翻转到显示方向
3. 再叠加用户调整的 `rotation/flip_h/flip_v`（通过 `face::apply_transform`）

**重要顺序**：`apply_exif_orientation` 必须在 `resize_to_fill` **之前**调用，否则裁剪会发生在传感器方向（横版图裁横版内容），而非显示方向（竖版图裁竖版内容）。

这两层变换组合后的图像才是"显示空间"。人脸 bbox、缩略图生成、人脸裁图均应在显示空间操作。

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

### ort 2.0.0-rc.12 关键 API（避免踩坑）

Step 14 全程使用 `ort = "=2.0.0-rc.12"`，以下细节与文档和 AI 训练数据常见说法不一致：

**0. Cargo features 配置（M4 Mac 优化）**

```toml
ort = { version = "=2.0.0-rc.12", features = ["download-binaries", "coreml", "ndarray"] }
```

- `download-binaries`：编译时自动从 parcel.pyke.io 下载 ort 预编译二进制（macOS arm64 版本已内置 CoreML 支持）
- `coreml`：启用 CoreML EP 代码路径，推理自动路由至 M4 的 Neural Engine（ANE）
- 不需要手动设置 `ORT_DYLIB_PATH`，也不需要 `brew install onnxruntime`

Session builder 注册 CoreML EP（三个模型加载器均使用此模式）：

```rust
Session::builder()?
    .with_execution_providers([ort::ep::coreml::CoreML::default().build()])?
    .commit_from_memory(&bytes)
```

**1. Session 的 import 路径**

```rust
// ✗ 错误：ort::Session 在 RC.12 未在 crate root 再导出
use ort::Session;

// ✓ 正确
use ort::session::Session;
```

**2. ndarray 版本必须是 0.17**

ort RC.12 内部依赖 ndarray 0.17（不是 0.15 或 0.16）。`Cargo.toml` 中必须写：

```toml
ndarray = "0.17"
```

版本不匹配时 `TensorArrayData` trait bound 不满足，错误信息极不直观。

**3. TensorRef 构造：传 `&Array`，不是 `.view()`**

```rust
// ✗ 错误
TensorRef::from_array_view(input.view())?

// ✓ 正确：传引用，不是 ArrayView
let tensor = TensorRef::from_array_view(&input)?;
```

**4. `try_extract_tensor` 返回的是 `(Shape, &[f32])` 元组，不是 ndarray**

```rust
// ✗ 错误：.view() 对 tuple 无效
let sv = outputs["scores"].try_extract_tensor::<f32>()?;
sv.view()

// ✓ 正确：解构元组，用平坦切片索引
let (_shape, scores) = outputs["scores"].try_extract_tensor::<f32>()?;
let conf = scores[i * 2 + 1];         // ultraface: [1,4420,2]
let x1   = scores_boxes[i * 4];       // ultraface boxes: [1,4420,4]
```

**5. `Session::run()` 需要 `&mut self` → 全局 Session 要用 `Mutex`**

```rust
// ✗ 错误：OnceLock<Option<Session>> 只能给出 &Session，无法 run
static SESSION: OnceLock<Option<Session>> = OnceLock::new();

// ✓ 正确
static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();
// 使用时：
let mut session = mtx.lock().unwrap();
run_inference(&mut session, img)
```

**6. `Session::builder().and_then(...)` 中闭包需要 `mut`**

```rust
// ✗ 错误
Session::builder().and_then(|b| b.commit_from_file(&path))

// ✓ 正确：commit_from_file 消耗 self，闭包参数需 mut
Session::builder().and_then(|mut b| b.commit_from_file(&path))
```

**7. `SessionOutputs` 不支持 `&String` 作为索引键**

```rust
// ✗ 错误
let key: String = outputs.keys().next().unwrap().to_owned();
outputs[&key]   // 编译失败：无 Index<&String> impl

// ✓ 正确：用 &str、String（owned）或 usize
outputs[0usize]
outputs["scores"]
```

**8. `inputs!` 宏不返回 `Result`，不加 `?`**

```rust
// ✗ 错误
session.run(ort::inputs!["input" => tensor]?)?

// ✓ 正确
session.run(ort::inputs!["input" => tensor])?
```

### face 模块测试模式

- 纯函数测试（`iou`、`nms`、`preprocess`、`l2_normalize`、`encode/decode`）无需模型文件，始终在 CI 中运行
- 需要 ONNX 模型的测试**直接**在测试函数内创建 `Session`（见下方模板），不依赖全局 OnceLock
- `execute_job` 标为 `pub(crate)` 供测试同步调用，避免 `tokio::spawn` 导致的竞争条件
- session loader 中 `if cfg!(test) { return None; }` 的作用：纯单元测试不需要推理，跳过加载避免无谓开销，**不是**对运行时配置问题的掩盖
- 需要 DB 持久化的测试：调用 `pub(crate)` 的底层函数（如 `run_inference`、`embed_with_session`、`save_faces`），不经过全局 OnceLock

**模型测试模板：**

```rust
// detector 测试
fn detector_session() -> Session {
    let path = dirs::config_dir().unwrap().join("picmanager/models/face_detector.onnx");
    Session::builder().unwrap()
        .with_execution_providers([ort::ep::coreml::CoreML::default().build()]).unwrap()
        .commit_from_file(&path).unwrap()
}

#[test]
fn my_model_test() {
    let mut session = detector_session();
    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples/IMG_9844.JPG")).unwrap();
    let faces = run_inference(&mut session, &img).unwrap();
    assert!(!faces.is_empty());
}
```

### ort 推理性能问题诊断

当 ort 推理异常缓慢或卡死时，优先排查 features 配置，而非在代码里绕过：

**症状 → 根因 → 正确修法**

| 症状 | 根因 | 修法 |
|------|------|------|
| `Session::builder()` 或 `commit_from_file` 卡住 / 极慢 | `load-dynamic` 找到不兼容的系统 dylib（如 Python venv 中的 1.x API） | 换 `download-binaries`，消除运行时 dylib 查找 |
| 推理成功但 CPU 使用率 100%、速度极慢 | 未注册 CoreML EP，全部走 CPU 路径 | 加 `coreml` feature，builder 注册 `ep::coreml::CoreML` |
| 测试挂起 | ort 初始化触发 dylib 查找，在 tokio async 线程中阻塞 | 推理移到 `spawn_blocking`；单元测试用 `cfg!(test)` 跳过加载 |

**正确的 macOS M4 配置（当前项目使用）：**
```toml
ort = { version = "=2.0.0-rc.12", features = ["download-binaries", "coreml", "ndarray"] }
```
首次 `cargo build` 时自动下载含 CoreML 的预编译二进制（~60 MB，之后从 Cargo 缓存读取）。

**验证 crate API 的可靠方式：**
直接读 tagged release 的源码，不依赖 LLM 训练数据或未固定版本的文档。
例如 ort RC.12 CoreML EP 的实现：`https://github.com/pykeio/ort/blob/v2.0.0-rc.12/src/ep/coreml.rs`

### 精炼质心算法（compute_refined_centroid）

`src/web/handlers/people.rs` 中的 `compute_refined_centroid(faces: &[(i64, Vec<f32>, f32)])` 实现两步算法，所有需要质心的端点均调用此函数。

**步骤1：置信度预过滤**
- 优先取 confidence ≥ `CENTROID_HIGH_CONF (0.85)` 的人脸，若 ≥ `CENTROID_MIN_CONF_FACES (10)` 张则用这组
- 否则降至 ≥ `CENTROID_LOW_CONF (0.70)`，若 ≥ 10 张则用这组
- 否则回退到全部人脸（原始行为）
- **目的**：排除暗光/畸变/帽沿遮挡等低质量检测——这类照片的 embedding 可能与正常人脸差异极大，会系统性拉偏质心

**步骤2：几何精炼**
- 若候选数量 > `REFINE_THRESHOLD (50)`：先算粗质心，取距粗质心最近的 `REFINE_PCT (40%)` 再算精炼质心
- 候选数量 ≤ 50 时直接用全部候选

**注意**：callers 在调用前需 fetch `COALESCE(f.confidence, 0.0)` 并作为第三个元素传入；get_merge_suggestions / get_outlier_faces / get_centroid_faces 三个 handler 均已更新。

### DBSCAN 聚类关键细节

- `region_query` 包含点本身（min_samples 按标准 DBSCAN 定义计数自身，如 min_samples=2 意味着至少 1 个其他邻居）
- 距离 = `1.0 - dot(a, b)`（L2 归一化嵌入，所以等价于余弦距离）
- **当前参数**：`EPS = 0.35`，`min_samples = 2`，`MIN_CONFIDENCE = 0.70`（均为 `face/cluster.rs` 顶部常量）
- 噪点（未归入任何核心点）各自单独成组，便于用户后续手动合并

### DBSCAN 链式合并（chaining）陷阱

**症状**：全量重建后出现一个包含数百张脸的超大聚类，其中 98%+ 的脸对余弦距离远超 eps，脸 ID 均匀分布在整个数据库范围内。

**根因**：若 A→B→C 两两距离均 < eps（即使 A 与 C 差异极大），DBSCAN 会把三者并入同一簇。置信度低的检测（小脸、侧脸、模糊脸）embedding 质量差，落在不同真实人物之间充当"桥接点"，导致不相关人物的脸被链式串联。超大簇形成后，增量聚类也会持续把新脸吸入（越大的簇越容易命中 eps 阈值）。

**修法（已实现）**：`run_clustering` 使用两阶段算法：
1. 仅对 `confidence >= MIN_CONFIDENCE (0.70)` 的脸做 DBSCAN
2. 低置信度脸事后归入最近人物（距离 < eps）或各自单独建人物记录

**禁止**：不要调高 eps（会加剧链式合并）；不要在未修复算法的情况下反复全量重建（重建结果相同）

### YOLOv8-nano 关键细节

- 输出 `[1, 84, 8400]`，布局为 feature-first：`flat[f * 8400 + i]` 访问第 i 个检测框的第 f 个特征
- 前 4 行为 cx/cy/w/h（归一化到 640），后 80 行为 COCO 类别分数（无需 sigmoid，已在模型内处理）
- 动物类索引 14–23（0-based COCO）：bird/cat/dog/horse/sheep/cow/elephant/bear/zebra/giraffe
- 与 face detector 不同，YOLOv8 输入归一化为 `[0,1]` RGB，face detector 是 `(pixel-127)/128` BGR
- 模型路径：`{config_dir}/picmanager/models/yolov8n.onnx`（约 6 MB）

### sqlx 动态 IN 子句

sqlx 宏（`sqlx::query!`）不支持把 `Vec` 直接绑定为 IN 参数。需要动态 IN 时（如 batch-update），必须手工构造占位符字符串并逐一 bind：

```rust
// ✗ 错误：sqlx 宏不支持数组绑定
sqlx::query!("UPDATE people SET status = ? WHERE id IN (?)", status, ids)

// ✓ 正确：拼占位符 + 链式 bind
let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
let sql = format!("UPDATE people SET status = ? WHERE id IN ({placeholders})");
let mut q = sqlx::query(&sql).bind(&status);
for id in &ids { q = q.bind(id); }
let result = q.execute(&pool).await?;
```

注意用 `sqlx::query`（非宏版本），且 `rows_affected()` 是 `u64`，与 `ids.len()` 比较时需转型。

### NMS 结果收集不能用 Vec::remove

`Vec::remove(i)` 会把后续元素前移，对 NMS 返回的原始索引集合逐一调用会导致越界 panic（移除第一个元素后，后续原始索引全部偏移）。

```rust
// ✗ 错误：remove(0) 后 candidates 长度缩减，remove(1) 可能越界
let kept = nms(&candidates, 0.45);
let result: Vec<_> = kept.into_iter().map(|i| candidates.remove(i).0).collect();

// ✓ 正确：直接索引 + clone，不修改 vec
let result: Vec<_> = kept.into_iter().map(|i| candidates[i].0.clone()).collect();
```

### ONNX 模型输入输出节点名必须验证

不同来源的 ONNX 模型节点名可能与常见示例不同，**不能假设**。已知：

| 模型 | 输入名 | 输出 |
|------|--------|------|
| ultraface-slim-320（face_detector.onnx） | `"input"` | `"scores"` `"boxes"` |
| w600k_mbf（arcface_mobilenetv1.onnx） | `"input.1"` | `outputs[0usize]` |
| yolov8n | `"images"` | `outputs[0usize]` |

验证方法：`python3 -c "import onnx; m=onnx.load('model.onnx'); print([i.name for i in m.graph.input])"`，或在二进制中搜索字符串（见 ONNX protobuf 格式）。

### 前端 flex 布局：照片/缩略图被压扁

此问题已出现两次（人物卡片、地点相册照片网格），根因相同：**flex 容器未给图片网格分配明确高度，图片在剩余空间中被压缩**。

**规则：**

1. `photo-grid` / `photo-card` 放在 flex 列容器里时，容器必须有明确的可用高度，否则网格会被压到零或挤成一条。
2. 需要独立滚动的照片区域，必须同时设置三个属性：
   ```css
   flex: 1;          /* 占据剩余空间 */
   min-height: 0;    /* 允许 flex 子元素收缩到小于内容高度，否则会撑破父容器 */
   overflow-y: auto; /* 内容超出时滚动 */
   ```
   缺少任何一个都会复现压扁问题。
3. 同级的固定高度兄弟节点（如筛选列、工具栏）要加 `flex-shrink: 0`，防止它们被照片区域挤压。
4. 照片区域如果可能有大量图片（如地点相册），必须加分页或虚拟滚动，不能一次性 `per_page=100` 硬拉所有数据。

**检查清单（出现图片压扁时）：**
- 找到 `photo-grid` 的所有祖先 flex 容器，逐级确认是否有未约束的高度
- 检查照片容器是否缺 `min-height: 0`
- 检查固定高度的兄弟元素是否有 `flex-shrink: 0`

### CSS grid + aspect-ratio：卡片高度为 0

**症状：** 卡片宽度正确（由 grid column 决定），但高度为 0，`img` 的 `aspect-ratio: 1` 没有生效。

**根因：** CSS grid 默认 `align-items: stretch`。当 grid item 同时设置了 `aspect-ratio` 时，浏览器用"stretch 高度"推导 aspect-ratio 高度，又用 aspect-ratio 高度推导 stretch 高度，循环依赖导致结果为 0。

**修法：** 在 grid 容器或 grid item 上加 `align-items: start`（或 `align-self: start`），让卡片高度由内容（aspect-ratio + meta 文字）决定，打破循环：

```css
.people-grid { align-items: start; }
```

### flex 侧边栏内滚动：让 section 本身滚动而非嵌套 grid 滚动

**症状：** 侧边栏中 grid 设置了 `flex: 1; overflow-y: auto`，但所有行被压缩进容器高度，内容不滚动。

**根因：** flex column 容器上的 `overflow: hidden` 会压制子 grid 的 `overflow-y: auto` 滚动语义，导致 grid 行高被等分压缩。

**修法：** 不要让 grid 自身滚动，改为让**外层 section 滚动**，grid 自然展开到内容高度：

```css
/* 外层 section 负责滚动 */
#people-list-section { overflow-y: auto; overflow-x: hidden; display: flex; flex-direction: column; }
/* grid 不参与滚动，自然高度 */
#people-list-section .people-grid { flex: none; overflow-y: visible; }
```

### 视图切换：布局覆盖规则不能用 display !important

**症状：** 切换到其他标签页后，人物标签页的内容仍然显示在下方。

**根因：** 用 `#view-people { display: flex !important }` 覆盖 flex-direction 时，`!important` 同时覆盖了 `.view-section.hidden { display: none }`，导致隐藏状态失效。

**修法：** 用 `:not(.hidden)` 选择器限定布局覆盖的作用范围，避免与隐藏状态冲突：

```css
/* ✗ 错误：!important 覆盖 display:none */
#view-people { display: flex !important; flex-direction: row !important; }

/* ✓ 正确：只在非隐藏状态下应用布局 */
#view-people:not(.hidden) { display: flex; flex-direction: row; }
```

### CSS grid + overflow:hidden 行高为 0：照片全堆在顶部互相重叠

**症状：** 照片缩略图比例正确（不再压扁），但全部叠在一起，y 坐标相同，看起来只有一张。

**根因：** `photo-card` 设有 `overflow: hidden`，CSS grid 计算行高时以 **min-content** 为基准（`grid-auto-rows: auto` 的默认行为）。`overflow: hidden` 的元素 min-content 高度为 0（内容不撑开行高），导致每行高度为 0，所有卡片都被放置在 y=0 处，相互重叠。

**修法：** 改用 `grid-auto-rows: max-content`，让行高由元素的 **max-content**（实际渲染高度）决定，而非 min-content：

```css
.photo-grid {
  grid-auto-rows: max-content;  /* ← 必须显式指定；auto 对 overflow:hidden 卡片无效 */
  align-items: start;           /* 同时需要，避免 stretch 循环依赖 */
}
```

**检查清单：** 每当新增带 `overflow: hidden` 的 grid item 时，确认其 grid 容器已设置 `grid-auto-rows: max-content`。

### people.parent_id 自引用孤儿节点（合并父节点到子节点）

**症状**：将人物 P 合并到它的子节点 C 后，C 从列表中消失——它既不是顶级节点（parent_id ≠ NULL），也无法从根追溯到它（P 已删除）。

**根因**：旧版 `merge_people` 先执行 `UPDATE people SET parent_id = C WHERE parent_id = P`，再删除 P。由于 C 本身的 parent_id 也是 P，这条 UPDATE 会把 C 的 parent_id 改为 C 自身（自引用循环）。P 删除后 C 就彻底悬挂。

**修法（已实现）**：合并前用递归 CTE 检查 source 是否为 target 的祖先：
```sql
WITH RECURSIVE ancestors(id) AS (
    SELECT parent_id FROM people WHERE id = $target AND parent_id IS NOT NULL
    UNION ALL
    SELECT p.parent_id FROM people p
    JOIN ancestors a ON p.id = a.id
    WHERE p.parent_id IS NOT NULL
)
SELECT COUNT(*) FROM ancestors WHERE id = $source
```
若 source 是祖先，先把 target.parent_id 改为 source.parent_id（提升），再把 source 的其余子节点改到 target 下，最后删 source。整个操作在事务内完成。

**修复已有孤儿**：migration 0013 在服务启动时自动修复：
```sql
UPDATE people SET parent_id = NULL WHERE id = parent_id;
```

**禁止**：不要在 merge 里直接 `UPDATE people SET parent_id = target WHERE parent_id = source` 而不排除 target 自身。

### cover_face_id FK 约束导致人脸删除静默失败

**症状**：`execute_job` 或 `reanalyze_one_photo` 调用后，faces 表里的旧记录没有被删除，新旧人脸数据同时存在，embedding 未更新。

**根因**：`people.cover_face_id INTEGER REFERENCES faces(id)` 没有 `ON DELETE` 动作（等价于 RESTRICT）。SQLite 开启 `PRAGMA foreign_keys = ON` 后，直接 `DELETE FROM faces WHERE photo_id = ?` 会因 FK 约束失败。代码使用了 `.ok()` 吞掉了错误，人脸未删、analyze_one 重新插入，导致重复行。

**修法（已实现）**：删除 faces 之前，先把引用了这些人脸的 cover_face_id 清零：
```rust
sqlx::query(
    "UPDATE people SET cover_face_id = NULL \
     WHERE cover_face_id IN (SELECT id FROM faces WHERE photo_id = ?)",
)
.bind(photo_id).execute(pool).await.ok();
// 之后再 DELETE FROM faces WHERE photo_id = ?
```

**禁止**：不要直接删除 faces 而不先处理 cover_face_id 引用。

### 旋转/翻转照片后 face embedding 失效

**症状**：用户对照片做旋转/翻转后，人脸聚类出现同一人分属多个人物，或人脸缩略图方向不对。

**根因**：`analyze_one` 在调用时读取当时的 `rotation/flip_h/flip_v` 值，在显示空间检测并存储 bbox + embedding。用户之后旋转照片时，只更新了 DB 字段和缩略图缓存，已存储的 embedding 仍是旧方向下的结果，与新显示方向不一致。

**修法（已实现）**：`PATCH /api/photos/{id}` 和 `batch-update` 检测到 transform 变更后，后台 `tokio::spawn` 调用 `face::job::reanalyze_one_photo(pool, photo_id)`，该函数：
1. 清除 cover_face_id 引用
2. 删除旧人脸记录
3. 以新方向重新检测 + 提取 embedding

**历史数据修复**：对在此修复前已旋转的照片，运行：
```bash
picmanager faces analyze --rotated-only
```
该命令仅处理 `rotation != 0 OR flip_h != 0 OR flip_v != 0` 且有人脸记录的照片。

### 测试样本照片（tests/samples/）

**选用原则：根据内容选正确的文件，不要混用。**

| 文件 | 相机 | 内容 | 有人脸 | GPS | 用途 |
|------|------|------|--------|-----|------|
| `IMG_20250204_135549.jpg` | 华为 ABR-AL60 | 场景照片（无人物）| ✗ | ✗ | EXIF 日期解析；ONNX 推理 smoke test（只验证不 panic）|
| `IMG_9886.HEIC` | iPhone（HEIC 格式）| 含 GPS 信息的照片 | ✗ | ✓ 39.8406°N 116.2180°E（北京）| GPS 提取测试；HEIC 格式解析 |
| `IMG_9844.JPG` | 尼康 Z 8 | 人像照片 | ✓ | ✗ | 人脸检测/嵌入测试；需要断言检测到人脸时必须用此文件 |

**注意**：`IMG_20250204_135549.jpg` 没有可检测的人脸，不能用于 `assert!(!faces.is_empty())` 类测试。

### CSS writing-mode + transform:rotate 叠加陷阱

**症状：** 竖排文字标签中每个字符都是上下颠倒的。

**根因：** `writing-mode:vertical-lr` 让文字在列内从上到下排列，字符本身方向正常。再叠加 `transform:rotate(180deg)` 时，整个元素旋转 180°，字符本身也跟着翻转，最终每个字都是倒置的。

**规则：**
- 只需要竖排（从上到下）：`writing-mode:vertical-lr` 即可，不加任何 transform
- 需要从下到上排列：对**外层块级容器**做 `transform:rotate(180deg)`，而不是对设了 writing-mode 的内联元素旋转

```css
/* ✗ 错误：字符会倒置 */
span { writing-mode: vertical-lr; transform: rotate(180deg); }

/* ✓ 正确：从上到下竖排 */
span { writing-mode: vertical-lr; }

/* ✓ 正确：从下到上竖排（整块旋转，字符不倒） */
.label-wrapper { transform: rotate(180deg); }
.label-wrapper span { writing-mode: vertical-lr; }
```

### 前端 cursor-anchored 弹出确认框：outside-click 必须延迟注册

**场景：** 用 `e.clientX/Y` 定位一个 `position:fixed` 的确认气泡，点击气泡外部时自动关闭。

**陷阱：** 在点击事件处理函数内**同步**注册 `document.addEventListener('click', dismiss)` 后，该监听器会立即被当前冒泡到 document 的点击事件触发，气泡刚创建就被关闭。

**修法：** 用 `setTimeout(..., 0)` 将外部点击监听器的注册推迟到当前事件循环结束后：

```js
function showInlineConfirm(anchorX, anchorY, message, onConfirm) {
  const el = document.createElement('div');
  el.className = 'inline-confirm';
  // ... 设置内容和位置（viewport clamping）...
  document.body.appendChild(el);

  const dismiss = () => el.remove();
  el.querySelector('.confirm-ok').addEventListener('click', () => { dismiss(); onConfirm(); });
  el.querySelector('.confirm-cancel').addEventListener('click', dismiss);

  // ✓ 必须延迟：否则当前 click 事件冒泡到 document 时会立即触发 dismiss
  setTimeout(() => {
    document.addEventListener('click', dismiss, { once: true, capture: true });
  }, 0);
}
```

**Viewport clamping 模式：**
```js
const W = window.innerWidth, H = window.innerHeight;
const elW = el.offsetWidth || 200, elH = el.offsetHeight || 80;
let left = anchorX + 10, top = anchorY + 10;
if (left + elW > W - 8) left = anchorX - elW - 10;  // 翻到左侧
if (top  + elH > H - 8) top  = anchorY - elH - 10;  // 翻到上方
el.style.left = `${Math.max(8, left)}px`;
el.style.top  = `${Math.max(8, top)}px`;
```
注意 `el.offsetWidth` 在 `appendChild` 之后、设置 `left/top` 之前读取才有正确值（布局已发生）。

### PhotoBridge：macOS TCC 照片权限归属终端 App

`PHPhotoLibrary.requestAuthorization` 在命令行工具中调用时，macOS TCC 把权限请求归属到**启动该工具的终端 App**（如 iTerm2、Terminal.app），而不是 photobridge 二进制本身。

**关键行为：**
- 授权弹框显示的是"iTerm2 想要访问您的照片"，不是"photobridge"
- System Settings → Privacy & Security → Photos 里出现的是终端 App，不是 photobridge
- `tccutil reset Photos com.picmanager.photobridge` 会报错（bundle ID 从未注册过 TCC）
- 授权后无需重启终端，直接再跑一次命令即可

**首次使用流程：**
1. `swift build -c release` 后执行 `codesign --force --sign - --entitlements Sources/PhotoBridge/PhotoBridge.entitlements .build/release/photobridge`
2. 运行 `photobridge export --dry-run`，终端 App 会弹出照片授权框
3. 点击"允许"，直接再跑命令，权限立刻生效

**弹框不出现（之前拒绝过）：** reset 终端 App 的权限后重试：
```bash
tccutil reset Photos com.googlecode.iterm2  # iTerm2
tccutil reset Photos com.apple.Terminal     # Terminal.app
tccutil reset Photos com.microsoft.VSCode   # VS Code
```

### PhotoBridge：localIdentifier 文件命名约定

导出文件名 = `localIdentifier` 中 `/` 替换为 `_`，拼接资源后缀：

```
PHAsset.localIdentifier = "B5A8F3C2-1234-5678-ABCD-000000000001/L0/001"
→ 导出文件名 = "B5A8F3C2-1234-5678-ABCD-000000000001_L0_001.heic"
```

`exportDestinationURL(stagingDir:localIdentifier:uti:)` 是唯一的命名来源，`FixTimestampsCommand` 和 `PicManagerRunner` 都依赖此函数的一致性。不要在其他地方硬编码命名规则。

### PhotoBridge：NDJSON 日志对接（picmanager → photobridge）

`picmanager import --log <file>` 将每条结果写为一行 JSON，字段：

```json
{"path":"/staging/B5A8_...heic","status":"imported","sha256":"...","error":null,"ts":"..."}
```

`status` 值（来自 Rust `LogStatus` 枚举，`serde(rename_all = "lowercase")`）：
- `"imported"` → 成功入库，暂存文件可删除
- `"skipped"`  → SHA-256 重复（已在库中），暂存文件可删除
- `"failed"`   → 失败，保留暂存文件等下次重试

Swift 侧 `parseImportLog()` 解析此格式；未知 status 归入 failedPaths（保守处理）。

## 常用命令

```bash
cargo nextest run            # 跑全部测试
cargo nextest run <模块>     # 跑特定模块，如 metadata::exif
cargo clippy                 # 检查警告
python3 tests/make_fixtures.py  # 重新生成 fixture 文件

picmanager import <dir>                     # 导入（移动文件）
picmanager import --copy <dir>              # 导入（保留源文件）
picmanager dedup                            # 增量 dedup 扫描（只比较新照片）
picmanager dedup --full                     # 全量 dedup 重扫（多索引分桶）
picmanager faces analyze                    # 全库人脸重分析
picmanager faces analyze --photo-ids 1,2,3 # 指定照片重分析
picmanager models fetch                     # 下载 ONNX 模型文件到配置目录
picmanager models bundle                    # 复制模型到 ./models/，再 cargo build 可内置于二进制
picmanager serve                            # 启动 Web（http://127.0.0.1:8080）
picmanager config                           # 显示当前配置

# PhotoBridge（在 photobridge/ 目录下构建后使用）
cd photobridge && swift build -c release
codesign --force --sign - --entitlements Sources/PhotoBridge/PhotoBridge.entitlements .build/release/photobridge

photobridge setup                           # 首次配置向导
photobridge setup --install-launchd         # 安装 launchd 定时同步任务（默认每 6 小时）
photobridge export                          # 全量导出 + 自动 picmanager import
photobridge export --dry-run                # 只统计数量
photobridge sync                            # 增量同步（自上次 token 之后的新照片）
photobridge status                          # 上次同步时间与数量
photobridge fix-timestamps /path/to/staging # 修复已导出文件的 mtime/ctime
.build/debug/PhotoBridgeTestRunner          # 跑 PhotoBridge 全部测试（53 个）
```
