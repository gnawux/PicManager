# 前端架构指南

开发新功能时的快速参考。补充 CLAUDE.md 中的 pitfall 列表和 DESIGN.md 中的 API 参考。

---

## 文件概览

| 文件 | 行数 | 说明 |
|------|------|------|
| `frontend/index.html` | ~337 | 所有 DOM 结构与 ID 在此定义；所有模态框骨架在此 |
| `frontend/style.css` | ~350 | 全部样式；无预处理器，直接 CSS |
| `frontend/app.js` | ~1900 | 全部 JavaScript；无构建步骤，无依赖（除 Leaflet） |

前端编译进二进制（rust-embed），**每次修改后必须 `cargo build --release` + 重启服务器**。

---

## app.js 分区地图

| 行范围 | 分区 | 核心函数 |
|--------|------|---------|
| 1–30 | `state` 对象 + 常量 | — |
| 31–155 | `init()` 事件绑定 | 所有 `addEventListener` 集中在此 |
| 157–394 | 照片标签页 | `loadPhotos` · `renderGrid` · `openDetail` · `navigateDetail` · 批量选择 |
| 395–465 | 相册 + 导入 | `loadAlbums` · `startImport` · `pollImport` |
| 466–522 | 去重 | `openDedupModal` |
| 523–538 | 视图切换 | `switchView` |
| 539–737 | 地点标签页 | `loadGeoHierarchy` · `renderGeoCountries/States/Cities` · `loadGeoPhotos` · `renderGeoPhotos` · `initMap` |
| 738–1646 | 人物标签页 | 见下方人物子分区 |
| 1649–1780 | 人物工具 | `fillMissingMeta` · `triggerIntegrate` · `triggerRecluster` · `openMergeDialog` · `confirmMerge` |
| 1781–1896 | 动物标签页 | `loadAnimalSpecies` · `showAnimalSpeciesPhotos` · `loadAnimalPhotos` |
| 1888–1897 | 工具函数 | `fetchJSON` |

### 人物标签页子分区（738–1646）

| 行范围 | 子分区 |
|--------|--------|
| 738–813 | 人物列表加载与渲染（`loadPeopleList` · `renderPeopleGrid`） |
| 815–875 | 多选与批量操作（`togglePersonSelect` · `batchUpdatePeopleStatus`） |
| 879–935 | 批量命名合并（`openPeopleNameMergeDialog` · `confirmPeopleNameMerge`） |
| 937–990 | 人物详情页（`showPersonDetail` · `renderPersonDetailPhotos`） |
| 1000–1047 | 子人物面板（`loadSubPersons`） |
| 1048–1090 | 详情页照片选择模式 |
| 1092–1330 | 创建子人物 / 父节点 / 转移至兄弟（三个操作 + 撤销） |
| 1331–1443 | 面包屑 · 更改父节点面板（`updatePersonBreadcrumb` · `openReparentPanel`） |
| 1444–1502 | 保存姓名 · 重复姓名检测（`savePersonName` · `checkDuplicateName`） |
| 1504–1526 | 撤销栈（`pushUndo` · `undoPeopleOp`） |
| 1527–1646 | 内联改名 · 右键菜单（`startInlineNameEdit` · `showPersonMenu`） |

---

## state 对象字段说明

```js
const state = {
  // ── 照片标签页 ──────────────────────────────────────
  page,          // 当前页码（1-based）
  perPage,       // 每页张数，固定 50
  total,         // 当前筛选下的总照片数
  albumId,       // 选中的相册 ID；null = 全部照片
  photos,        // Photo[] — 当前页照片（Photos tab 专用）

  // ── 详情弹窗（所有标签页共用） ────────────────────
  detailPhotos,  // Photo[] — 当前弹窗的 ← → 导航范围
  detailIdx,     // detailPhotos 中当前照片的下标
  currentDetail, // 当前照片的完整元数据（GET /api/photos/{id} 的响应）

  // ── 照片批量选择 ─────────────────────────────────
  selectMode,    // bool — 勾选模式是否激活
  selected,      // Set<photoId> — 已勾选的照片 ID

  // ── 全局视图状态 ──────────────────────────────────
  currentView,   // 'photos' | 'people' | 'locations' | 'animals'

  // ── 人物：列表 ────────────────────────────────────
  allPeople,      // People[] — 缓存的人物列表（用于合并对话框搜索）
  selectedPeople, // Set<personId> — 人物多选
  mergeTargetId,  // 「合并到」对话框中选定的目标人物 ID

  // ── 人物：详情页 ──────────────────────────────────
  currentPersonId,         // 右侧正在展示的人物 ID
  currentPersonParentId,   // 该人物的 parent_id（null = 顶级）
  personDetailSelectMode,  // bool — 照片选择模式是否激活
  personDetailSelection,   // Set<photoId> — 详情页已勾选照片
  personDetailPhotos,      // Photo[] — 详情页当前照片列表（undo 用）

  // ── 动物标签页 ────────────────────────────────────
  animalSpecies, // 当前浏览的动物种类字符串
  animalPage,    // 当前种类的页码
  animalTotal,   // 当前种类的总照片数
  animalPhotos,  // Photo[] — 当前种类当前页照片

  // ── 导入 ─────────────────────────────────────────
  importPollId,  // setInterval 句柄，导入轮询用
};
```

### 地点标签页的模块级变量（不在 state 中）

```js
let geoData = null;            // 缓存的层级数据（GET /api/geo/hierarchy 响应）
let geoCurrentGeoFilter = null;// {country?, state?, city?}；null 表示未选中
let geoCurrentPage = 1;        // 当前页码
const GEO_PER_PAGE = 50;       // 每页张数
```

---

## HTML 结构（关键 ID）

### 顶层布局
- `#main-content` — 外层 flex 容器（侧边栏 + 内容区）
- 左侧边栏：导入、去重、补全元数据、相册列表
- 右侧内容区：标签栏 + 四个视图

### 标签视图（均为 `.view-section`，同时只有一个可见）
- `#view-photos` — 照片标签页
- `#view-locations` — 地点标签页（左列表 + 右照片区 + 地图）
- `#view-animals` — 动物标签页
- `#view-people` — 人物标签页（左人物列表 + 右详情区）

### 共用弹窗（所有标签页可用）
- `#detail-modal` — 照片大图详情弹窗（四个标签页共享同一个弹窗）
- `#batch-time-modal` — 批量时间编辑
- `#dedup-modal` — 去重组

### 人物相关弹窗
- `#merge-modal` — 「合并到」人物选择器
- `#dup-name-modal` — 重复姓名检测对话框
- `#people-name-merge-modal` — 批量命名合并输入框
- `#sibling-picker-modal` — 转移至兄弟节点选择器
- `#person-detail-name-modal` — 通用姓名输入（创建子人物 / 父节点 / 兄弟）
- `#recluster-confirm-dialog` — 全量重建确认（`<dialog>` 元素，不是 `.modal`）

---

## 核心模式

### 照片卡片（标准写法）

```js
photos.forEach((p, idx) => {
  const card = document.createElement('div');
  card.className = 'photo-card';
  const label = p.taken_at ? p.taken_at.slice(0, 10) : '';
  card.innerHTML = `<img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
    <div class="meta">${label}</div>`;
  card.addEventListener('click', () => openDetail(idx, photos));
  grid.appendChild(card);
});
```

网格容器必须有 `class="photo-grid"`。若需要勾选框，在 `card.innerHTML` 开头加 `<div class="card-check"></div>`，并在容器上添加 `select-mode` class。

### 详情弹窗

```js
// 从任意标签页打开，context 作为 ← → 的导航范围
openDetail(idx, photosArray);

// 不传 context 则默认用 state.photos（照片标签页场景）
openDetail(idx);
```

`openDetail` 自动：获取大图 URL、拉取元数据（GET /api/photos/{id}）、渲染人脸框和动物框、更新 ← → 按钮状态。

### fetchJSON

```js
const data = await fetchJSON('/api/endpoint');
if (!data) return; // null 表示非 2xx 或网络错误

// POST 示例
const data = await fetchJSON('/api/endpoint', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(payload),
});
```

### 人物列表刷新

```js
loadPeopleList();
```

任何增删改 `people` 表的操作后都需要调用，否则左侧列表不会同步。同时更新 `state.allPeople`（供合并对话框搜索用）。

### 撤销栈

```js
pushUndo('操作描述（显示在按钮 tooltip）', async () => {
  // 逆操作：调用对应的 undo API 或 patch
  await fetch('/api/people/some-undo-endpoint', { method: 'POST', ... });
  loadPeopleList();
});
```

### switchView 钩子

```js
function switchView(view) {
  // ...隐藏/显示 view-section...
  if (view === 'photos')    loadPhotos();
  if (view === 'people')    loadPeopleList();
  if (view === 'locations') loadGeoHierarchy();
  if (view === 'animals')   loadAnimalSpecies();
}
```

---

## CSS 关键类

### 布局
- `.view-section` + `.view-section.hidden` — 标签视图的显隐
- `#view-people:not(.hidden)` — 人物标签用 row flex（特殊：覆盖 column 默认）
- `.toolbar` — flex 横向工具栏，`justify-content: space-between`
- `.modal` / `.modal-inner` — 弹窗遮罩 + 内容卡片

### 照片网格
- `.photo-grid` — CSS grid，`repeat(auto-fill, minmax(160px, 1fr))`
  - **必须**：`flex:1; min-height:0; overflow-y:auto`（否则高度被压扁）
  - **必须**：`grid-auto-rows: max-content; align-items: start`（否则 overflow:hidden 的卡片行高为 0）
- `.photo-card` — 含 `overflow:hidden`，注意上述 grid 配置缺一不可
- `.photo-card img` — `aspect-ratio:1; object-fit:cover; width:100%`

### 人物网格
- `.people-grid` — 与 photo-grid 同规则，列宽 `minmax(140px, 1fr)`

### 通用
- `.hidden` — `display:none`
- `.batch-bar` — 底部浮出的批量操作栏
- `.btn-ghost` — 次要按钮样式
- `.btn-danger` — 危险操作按钮（红色）

---

## 新功能开发清单

### 后端（每步完成后跑 `cargo nextest run`）
1. 在 `src/web/handlers/<domain>.rs` 添加 handler 函数
2. 在 `src/web/mod.rs` 引入 handler 并添加路由
3. 在 `tests/web_api.rs` 添加集成测试

### 前端
1. 在 `index.html` 添加必要的 DOM 结构（ID 要全局唯一）
2. 在 `init()` 里绑定事件监听（集中在 31–155 行）
3. 在 `app.js` 对应区域实现逻辑函数
4. 变更 `people` 表后调用 `loadPeopleList()`；变更地点数据后调用 `loadGeoHierarchy()`
5. 若新增照片网格，用 `openDetail(idx, photosArray)` 接入大图弹窗

### 文档（每次功能性变更必须同步）
- `docs/REQUIREMENTS.md` — 行为变更
- `docs/DESIGN.md` — 新增 API endpoint、DB schema 变化
- `CLAUDE.md` — 项目结构变化、新 pitfall
- 本文件（`docs/FRONTEND.md`）— 新增 state 字段、新 HTML ID、新模式

### 构建 + 验证
```bash
cargo nextest run        # 所有测试
cargo build --release    # 重新编译嵌入前端
# 重启 picmanager serve
```
