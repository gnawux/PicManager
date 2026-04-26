# PicManager

家庭照片与图片管理工具。支持自动整理、去重、按时间/地点/相机分相册，提供 Web 界面和命令行界面。

## 技术栈

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

当前测试数：**228 个**（`cargo nextest run` 全部通过，另有 1 个 `#[ignore]` 需 yolov8n.onnx）

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

### 测试样本照片（tests/samples/）

**选用原则：根据内容选正确的文件，不要混用。**

| 文件 | 相机 | 内容 | 有人脸 | GPS | 用途 |
|------|------|------|--------|-----|------|
| `IMG_20250204_135549.jpg` | 华为 ABR-AL60 | 场景照片（无人物）| ✗ | ✗ | EXIF 日期解析；ONNX 推理 smoke test（只验证不 panic）|
| `IMG_9886.HEIC` | iPhone（HEIC 格式）| 含 GPS 信息的照片 | ✗ | ✓ 39.8406°N 116.2180°E（北京）| GPS 提取测试；HEIC 格式解析 |
| `IMG_9844.JPG` | 尼康 Z 8 | 人像照片 | ✓ | ✗ | 人脸检测/嵌入测试；需要断言检测到人脸时必须用此文件 |

**注意**：`IMG_20250204_135549.jpg` 没有可检测的人脸，不能用于 `assert!(!faces.is_empty())` 类测试。

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
```
