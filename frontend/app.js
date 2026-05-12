'use strict';

const state = {
  page: 1,
  perPage: 50,
  order: 'desc',
  total: 0,
  albumId: null,
  collectionId: null,   // currently viewed collection
  inCollection: false,  // true when showing a curated collection
  importPollId: null,
  photos: [],       // current page photos for the Photos tab
  detailPhotos: [], // photos used for prev/next navigation in the open detail modal
  detailIdx: -1,    // index into detailPhotos of the currently open detail
  selectMode: false,
  selected: new Set(), // selected photo IDs
  currentDetail: null, // full detail object of the open photo
  currentView: 'photos', // 'photos' | 'people' | 'locations'
  currentPersonId: null, // person being viewed in detail
  currentPersonParentId: null, // parent_id of person in detail
  allPeople: [],    // cached people list for merge dialog
  mergeTargetId: null,
  selectedPeople: new Set(), // selected person IDs for batch ops
  personDetailSelectMode: false,
  personDetailSelection: new Set(), // photo IDs selected in person detail
  personDetailPhotos: [], // photos shown in person detail (for undo)
  personDetailPage: 1,
  personDetailTotal: 0,
  centroidPhotoIds: new Set(), // photo IDs used for refined centroid of current person
};

// Album category UI state — lives outside `state` so it survives loadAlbums() re-calls
const albumCategoryCollapsed = { camera: false, time: false, location: false };
const albumCategoryExpanded  = { camera: false, time: false, location: false };

const PERSON_DETAIL_PER_PAGE = 50;

// ── Init ──────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // Tab navigation
  document.querySelectorAll('.tab-btn[data-view]').forEach(btn => {
    btn.addEventListener('click', () => switchView(btn.dataset.view));
  });

  loadAlbums();
  loadCollections();
  loadPhotos();

  // Collections sidebar
  document.getElementById('create-collection-btn').addEventListener('click', () => {
    document.getElementById('create-collection-name').value = '';
    document.getElementById('create-collection-modal').classList.remove('hidden');
    document.getElementById('create-collection-name').focus();
  });
  document.getElementById('create-collection-ok-btn').addEventListener('click', async () => {
    const name = document.getElementById('create-collection-name').value.trim();
    if (!name) return;
    await createCollection(name);
    document.getElementById('create-collection-modal').classList.add('hidden');
  });
  document.getElementById('create-collection-cancel-btn').addEventListener('click', () => {
    document.getElementById('create-collection-modal').classList.add('hidden');
  });
  document.getElementById('create-collection-name').addEventListener('keydown', e => {
    if (e.key === 'Enter') document.getElementById('create-collection-ok-btn').click();
  });
  document.getElementById('rename-collection-ok-btn').addEventListener('click', async () => {
    const name = document.getElementById('rename-collection-name').value.trim();
    const id = parseInt(document.getElementById('rename-collection-modal').dataset.collectionId);
    if (!name || !id) return;
    await renameCollection(id, name);
    document.getElementById('rename-collection-modal').classList.add('hidden');
  });
  document.getElementById('rename-collection-cancel-btn').addEventListener('click', () => {
    document.getElementById('rename-collection-modal').classList.add('hidden');
  });
  document.getElementById('rename-collection-name').addEventListener('keydown', e => {
    if (e.key === 'Enter') document.getElementById('rename-collection-ok-btn').click();
  });

  document.getElementById('import-btn').addEventListener('click', startImport);
  document.getElementById('prev-btn').addEventListener('click', () => changePage(-1));
  document.getElementById('next-btn').addEventListener('click', () => changePage(1));
  document.getElementById('sort-order-btn').addEventListener('click', toggleSortOrder);
  document.getElementById('dedup-btn').addEventListener('click', openDedupModal);
  document.getElementById('close-dedup').addEventListener('click', () => {
    document.getElementById('dedup-modal').classList.add('hidden');
    closeCompareOverlay();
  });
  document.getElementById('close-compare').addEventListener('click', closeCompareOverlay);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeCompareOverlay();
  });

  // Detail edit
  // Geo view sub-tabs
  document.querySelectorAll('[data-geoview]').forEach(btn => {
    btn.addEventListener('click', () => switchGeoView(btn.dataset.geoview));
  });

  // People view
  document.getElementById('fill-faces-btn').addEventListener('click', fillMissingFaces);
  document.getElementById('fill-geo-btn').addEventListener('click', fillMissingGeo);
  document.getElementById('integrate-faces-btn').addEventListener('click', triggerIntegrate);
  document.getElementById('recluster-btn').addEventListener('click', () => {
    document.getElementById('recluster-confirm-dialog').showModal();
  });
  document.getElementById('recluster-confirm-cancel').addEventListener('click', () => {
    document.getElementById('recluster-confirm-dialog').close();
  });
  document.getElementById('recluster-confirm-ok').addEventListener('click', () => {
    document.getElementById('recluster-confirm-dialog').close();
    triggerRecluster();
  });
  document.getElementById('person-back-btn').addEventListener('click', () => showPeopleList());
  document.getElementById('centroid-debug-btn').addEventListener('click', () => {
    if (state.currentPersonId) openCentroidDebugModal(state.currentPersonId);
  });
  document.getElementById('person-name-input').addEventListener('change', savePersonName);
  document.getElementById('person-merge-btn').addEventListener('click', openMergeDialog);
  document.getElementById('person-reparent-btn').addEventListener('click', openReparentPanel);
  document.getElementById('person-lift-btn').addEventListener('click', startCreateParent);
  document.getElementById('person-reparent-search').addEventListener('input', filterReparentList);
  document.getElementById('person-select-toggle-btn').addEventListener('click', togglePersonDetailSelectMode);
  document.getElementById('person-detail-create-child-btn').addEventListener('click', () => createSubPerson([...state.personDetailSelection]));
  document.getElementById('person-detail-transfer-sibling-btn').addEventListener('click', () => startTransferToSibling([...state.personDetailSelection]));
  document.getElementById('person-detail-cancel-select-btn').addEventListener('click', clearPersonDetailSelection);
  document.getElementById('sibling-picker-cancel').addEventListener('click', () => {
    document.getElementById('sibling-picker-modal').classList.add('hidden');
  });
  document.getElementById('person-detail-name-cancel').addEventListener('click', () => {
    document.getElementById('person-detail-name-modal').classList.add('hidden');
    if (_personDetailNameResolve) { _personDetailNameResolve(null); _personDetailNameResolve = null; }
  });
  document.getElementById('merge-cancel-btn').addEventListener('click', () => {
    document.getElementById('merge-modal').classList.add('hidden');
  });
  document.getElementById('merge-confirm-btn').addEventListener('click', confirmMerge);
  document.getElementById('merge-search').addEventListener('input', filterMergeList);

  // Generic merge confirmation modal
  document.getElementById('merge-confirm-ok-btn').addEventListener('click', () => {
    document.getElementById('merge-confirm-modal').classList.add('hidden');
    if (_mergeConfirmCallback) { const cb = _mergeConfirmCallback; _mergeConfirmCallback = null; cb(); }
  });
  document.getElementById('merge-confirm-cancel-btn').addEventListener('click', () => {
    document.getElementById('merge-confirm-modal').classList.add('hidden');
    _mergeConfirmCallback = null;
  });

  // People batch bar
  document.getElementById('people-batch-merge-btn').addEventListener('click', openPeopleNameMergeDialog);
  document.getElementById('people-batch-ignore-btn').addEventListener('click', () => batchUpdatePeopleStatus('ignored'));
  document.getElementById('people-batch-notperson-btn').addEventListener('click', () => batchUpdatePeopleStatus('not_a_person'));
  document.getElementById('people-batch-cancel-btn').addEventListener('click', clearPeopleSelection);
  document.getElementById('people-merge-name-confirm').addEventListener('click', confirmPeopleNameMerge);
  document.getElementById('people-merge-name-cancel').addEventListener('click', () => {
    document.getElementById('people-name-merge-modal').classList.add('hidden');
  });

  // Undo
  document.getElementById('people-undo-btn').addEventListener('click', undoPeopleOp);

  // Duplicate name dialog
  document.getElementById('dup-name-merge-btn').addEventListener('click', confirmDupNameSame);
  document.getElementById('dup-name-keep-btn').addEventListener('click', () => {
    document.getElementById('dup-name-modal').classList.add('hidden');
    _dupNameContext = null;
    if (_dupNameResolve) { _dupNameResolve({ action: 'different' }); _dupNameResolve = null; }
  });

  // Close floating context menus when clicking outside
  document.addEventListener('click', () => closePersonMenu());

  document.getElementById('detail-original-btn').addEventListener('click', toggleDetailOriginal);
  document.getElementById('detail-edit-btn').addEventListener('click', () => {
    openDetailEdit();
  });
  document.getElementById('edit-save-btn').addEventListener('click', saveDetailEdit);
  document.getElementById('edit-cancel-btn').addEventListener('click', cancelDetailEdit);

  // Detail rotation buttons
  document.getElementById('detail-rotate-left-btn').addEventListener('click', () => {
    const d = state.currentDetail; if (d) applyPhotoTransform(d.id, -90, false, false);
  });
  document.getElementById('detail-rotate-right-btn').addEventListener('click', () => {
    const d = state.currentDetail; if (d) applyPhotoTransform(d.id, 90, false, false);
  });
  document.getElementById('detail-flip-v-btn').addEventListener('click', () => {
    const d = state.currentDetail; if (d) applyPhotoTransform(d.id, 0, false, true);
  });
  document.getElementById('detail-flip-h-btn').addEventListener('click', () => {
    const d = state.currentDetail; if (d) applyPhotoTransform(d.id, 0, true, false);
  });

  // Batch select
  document.getElementById('select-toggle-btn').addEventListener('click', toggleSelectMode);
  document.getElementById('batch-deselect-btn').addEventListener('click', () => {
    clearSelection();
    toggleSelectMode(); // exit select mode
  });
  document.getElementById('batch-rotate-left-btn').addEventListener('click',  () => applyBatchTransform(-90, false, false));
  document.getElementById('batch-rotate-right-btn').addEventListener('click', () => applyBatchTransform(90, false, false));
  document.getElementById('batch-flip-v-btn').addEventListener('click',       () => applyBatchTransform(0, false, true));
  document.getElementById('batch-flip-h-btn').addEventListener('click',       () => applyBatchTransform(0, true, false));
  document.getElementById('batch-time-btn').addEventListener('click', () => {
    document.getElementById('batch-taken-at').value = '';
    document.getElementById('batch-timezone').value = '';
    document.getElementById('batch-time-modal').classList.remove('hidden');
  });
  document.getElementById('batch-time-cancel-btn').addEventListener('click', () => {
    document.getElementById('batch-time-modal').classList.add('hidden');
  });
  document.getElementById('batch-time-save-btn').addEventListener('click', saveBatchTime);

  // Collection batch operations
  document.getElementById('batch-add-collection-btn').addEventListener('click', () => showAddToCollectionModal([...state.selected]));
  document.getElementById('batch-remove-collection-btn').addEventListener('click', async () => {
    if (state.inCollection && state.collectionId) {
      await removePhotosFromCollection(state.collectionId, [...state.selected]);
      clearSelection();
      toggleSelectMode();
      loadPhotos();
    }
  });
  document.getElementById('add-to-collection-cancel-btn').addEventListener('click', () => {
    document.getElementById('add-to-collection-modal').classList.add('hidden');
  });
  document.getElementById('add-to-collection-create-btn').addEventListener('click', async () => {
    const name = document.getElementById('add-to-collection-new-name').value.trim();
    if (!name) return;
    const resp = await fetch('/api/collections', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    });
    if (!resp.ok) return;
    const col = await resp.json();
    const photoIds = JSON.parse(document.getElementById('add-to-collection-modal').dataset.photoIds || '[]');
    await addPhotosToCollection(col.id, photoIds);
    document.getElementById('add-to-collection-modal').classList.add('hidden');
    loadCollections();
  });

  // "添加整个相册到精选集" toolbar button
  document.getElementById('add-album-to-collection-btn').addEventListener('click', async () => {
    if (!state.albumId && !state.inCollection) return;
    const srcAlbumId = state.albumId;
    // Fetch all photos from this album (up to 5000 – reasonable limit)
    const data = await fetchJSON(`/api/albums/${srcAlbumId}/photos?page=1&per_page=5000`);
    if (!data) return;
    const photoIds = data.photos.map(p => p.id);
    showAddToCollectionModal(photoIds);
  });


  document.getElementById('detail-close').addEventListener('click', closeDetail);
  document.getElementById('detail-prev').addEventListener('click', () => navigateDetail(-1));
  document.getElementById('detail-next').addEventListener('click', () => navigateDetail(1));
  document.getElementById('detail-modal').addEventListener('click', (e) => {
    if (e.target === document.getElementById('detail-modal')) closeDetail();
  });
  document.addEventListener('keydown', (e) => {
    const modal = document.getElementById('detail-modal');
    if (modal.classList.contains('hidden')) return;
    if (e.key === 'Escape') closeDetail();
    else if (e.key === 'ArrowLeft') navigateDetail(-1);
    else if (e.key === 'ArrowRight') navigateDetail(1);
  });
});

// ── Photos ────────────────────────────────────────────────────────────────────
async function loadPhotos() {
  let url;
  if (state.inCollection && state.collectionId) {
    url = `/api/collections/${state.collectionId}/photos?page=${state.page}&per_page=${state.perPage}&order=${state.order}`;
  } else if (state.albumId) {
    url = `/api/albums/${state.albumId}/photos?page=${state.page}&per_page=${state.perPage}&order=${state.order}`;
  } else {
    url = `/api/photos?page=${state.page}&per_page=${state.perPage}&order=${state.order}`;
  }

  const data = await fetchJSON(url);
  if (!data) return;

  state.total = data.total;
  renderGrid(data.photos);
  renderPagination();
  document.getElementById('photo-count').textContent = `共 ${data.total} 张`;
}

function renderGrid(photos) {
  state.photos = photos;
  const grid = document.getElementById('photo-grid');
  grid.innerHTML = '';
  photos.forEach((p, idx) => {
    const card = document.createElement('div');
    card.className = 'photo-card' + (state.selectMode ? ' select-mode' : '');
    if (state.selected.has(p.id)) card.classList.add('selected');
    const label = p.taken_at ? p.taken_at.slice(0, 10) : p.path.split('/').pop();
    card.innerHTML = `
      <div class="card-check"></div>
      <img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    card.addEventListener('click', () => {
      if (state.selectMode) {
        toggleCardSelect(p.id, card);
      } else {
        openDetail(idx);
      }
    });
    grid.appendChild(card);
  });
}

function toggleSortOrder() {
  state.order = state.order === 'desc' ? 'asc' : 'desc';
  state.page = 1;
  document.getElementById('sort-order-btn').textContent =
    state.order === 'desc' ? '↓ 最新优先' : '↑ 最早优先';
  loadPhotos();
}

// ── Batch selection ───────────────────────────────────────────────────────────
function toggleSelectMode() {
  state.selectMode = !state.selectMode;
  const btn = document.getElementById('select-toggle-btn');
  btn.textContent = state.selectMode ? '✕ 退出选择' : '☑ 选择';
  if (!state.selectMode) clearSelection();
  renderGrid(state.photos);
}

function toggleCardSelect(photoId, card) {
  if (state.selected.has(photoId)) {
    state.selected.delete(photoId);
    card.classList.remove('selected');
  } else {
    state.selected.add(photoId);
    card.classList.add('selected');
  }
  updateBatchBar();
}

function clearSelection() {
  state.selected.clear();
  updateBatchBar();
}

function updateBatchBar() {
  const bar = document.getElementById('batch-bar');
  const n = state.selected.size;
  if (n > 0) {
    bar.classList.remove('hidden');
    document.getElementById('batch-count').textContent = `已选 ${n} 张`;
    document.getElementById('batch-add-collection-btn').classList.toggle('hidden', state.inCollection);
    document.getElementById('batch-remove-collection-btn').classList.toggle('hidden', !state.inCollection);
  } else {
    bar.classList.add('hidden');
  }
}

async function saveBatchTime() {
  const takenAt = document.getElementById('batch-taken-at').value.replace('T', 'T');
  const tzRaw = document.getElementById('batch-timezone').value;
  const body = { photo_ids: [...state.selected] };
  if (takenAt) body.taken_at = takenAt.replace('T', 'T');
  if (tzRaw !== '') body.timezone_offset = parseInt(tzRaw, 10);
  if (!body.taken_at && body.timezone_offset === undefined) {
    alert('请至少填写一个字段'); return;
  }
  await fetch('/api/photos/batch-update', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  document.getElementById('batch-time-modal').classList.add('hidden');
  loadPhotos();
}

async function applyPhotoTransform(id, rotDelta, flipH, flipV) {
  const body = {};
  if (rotDelta) body.rotation_delta = rotDelta;
  if (flipH) body.flip_h_toggle = true;
  if (flipV) body.flip_v_toggle = true;
  const resp = await fetch(`/api/photos/${id}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!resp.ok) return;
  const t = Date.now();
  // Refresh thumbnail in the photo grid
  const card = document.querySelector(`#photo-grid .photo-card img[src^="/api/photos/${id}/thumb"]`);
  if (card) card.src = `/api/photos/${id}/thumb?t=${t}`;
  // Refresh detail modal image
  const detailImg = document.getElementById('detail-img');
  if (detailImg && detailImg.src.includes(`/api/photos/${id}/`)) {
    detailImg.src = `/api/photos/${id}/thumb?t=${t}`;
  }
}

async function applyBatchTransform(rotDelta, flipH, flipV) {
  if (state.selected.size === 0) return;
  const body = { photo_ids: [...state.selected] };
  if (rotDelta) body.rotation_delta = rotDelta;
  if (flipH) body.flip_h_toggle = true;
  if (flipV) body.flip_v_toggle = true;
  await fetch('/api/photos/batch-update', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  const t = Date.now();
  state.selected.forEach(id => {
    const img = document.querySelector(`#photo-grid .photo-card img[src^="/api/photos/${id}/thumb"]`);
    if (img) img.src = `/api/photos/${id}/thumb?t=${t}`;
  });
}

// ── Photo detail modal ────────────────────────────────────────────────────────
// context: optional photos array for this session's prev/next navigation.
//   If omitted, falls back to state.photos (the Photos tab's current page).
async function openDetail(idx, context) {
  const photos = context || state.photos;
  const photo = photos[idx];
  if (!photo) return;
  state.detailPhotos = photos;
  state.detailIdx = idx;

  const modal = document.getElementById('detail-modal');
  modal.classList.remove('hidden');
  resetDetailOriginalBtn();

  const img = document.getElementById('detail-img');
  img.src = `/api/photos/${photo.id}/thumb`;

  // Fetch full metadata
  const detail = await fetchJSON(`/api/photos/${photo.id}`);
  if (detail) { state.currentDetail = detail; renderDetailMeta(detail); }
  cancelDetailEdit();

  // Fetch and draw face boxes; extract people for the sidebar
  const faces = await fetchJSON(`/api/photos/${photo.id}/faces`);
  if (faces) { renderFaceOverlay(faces); renderDetailPeople(faces); }

  updateDetailNav();
}

// ── Detail edit ───────────────────────────────────────────────────────────────
function openDetailEdit() {
  const d = state.currentDetail;
  if (!d) return;
  // Prefill form
  const ta = d.taken_at ? d.taken_at.replace(' ', 'T') : '';
  document.getElementById('edit-taken-at').value = ta.length > 16 ? ta.slice(0, 16) : ta;
  document.getElementById('edit-timezone').value = d.timezone_offset != null ? d.timezone_offset : '';
  document.getElementById('detail-edit-form').classList.remove('hidden');
  document.getElementById('detail-actions').classList.add('hidden');
}

async function saveDetailEdit() {
  const d = state.currentDetail;
  if (!d) return;
  const body = {};
  const ta = document.getElementById('edit-taken-at').value;
  if (ta) body.taken_at = ta.replace('T', 'T');
  const tz = document.getElementById('edit-timezone').value;
  if (tz !== '') body.timezone_offset = parseInt(tz, 10);
  await fetch(`/api/photos/${d.id}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  cancelDetailEdit();
  const updated = await fetchJSON(`/api/photos/${d.id}`);
  if (updated) { state.currentDetail = updated; renderDetailMeta(updated); }
  // Refresh grid photo label
  const p = state.photos.find(x => x.id === d.id);
  if (p && body.taken_at) { p.taken_at = body.taken_at; renderGrid(state.photos); }
}

function cancelDetailEdit() {
  document.getElementById('detail-edit-form').classList.add('hidden');
  document.getElementById('detail-actions').classList.remove('hidden');
}

function renderDetailMeta(detail) {
  document.getElementById('detail-title').textContent =
    detail.path.split('/').pop();

  const tzLabel = detail.timezone_offset != null
    ? `UTC${detail.timezone_offset >= 0 ? '+' : ''}${detail.timezone_offset / 60}`
    : '未知';

  const rows = [
    ['时间', detail.taken_at ? detail.taken_at.replace('T', ' ') : '—'],
    ['时区', tzLabel],
    ['相机', detail.camera || '—'],
    ['格式', detail.format],
    ['GPS', detail.gps_lat != null
      ? `${detail.gps_lat.toFixed(5)}, ${detail.gps_lon.toFixed(5)}`
      : '—'],
  ];

  const table = document.getElementById('detail-table');
  table.innerHTML = rows.map(([k, v]) =>
    `<tr><td>${k}</td><td>${v}</td></tr>`).join('');
}

function renderDetailPeople(faces) {
  const box = document.getElementById('detail-people');
  box.innerHTML = '';

  // Deduplicate by person_id; only show assigned faces
  const seen = new Set();
  const people = [];
  for (const f of faces) {
    if (f.person_id && !seen.has(f.person_id)) {
      seen.add(f.person_id);
      people.push({ person_id: f.person_id, person_name: f.person_name, face_id: f.id });
    }
  }

  if (people.length === 0) { box.classList.add('hidden'); return; }
  box.classList.remove('hidden');

  const label = document.createElement('div');
  label.className = 'detail-people-label';
  label.textContent = '人物';
  box.appendChild(label);

  const chips = document.createElement('div');
  chips.className = 'detail-people-chips';
  for (const p of people) {
    const chip = document.createElement('div');
    chip.className = 'person-chip';
    chip.title = p.person_name || '未命名';
    chip.innerHTML = `<img src="/api/faces/${p.face_id}/thumb" class="person-chip-thumb" alt="">
      <span class="person-chip-name">${escHtml(p.person_name || '未命名')}</span>`;
    chips.appendChild(chip);
  }
  box.appendChild(chips);
}

function renderFaceOverlay(faces) {
  const svg = document.getElementById('detail-faces');
  svg.innerHTML = '';
  if (!faces.length) return;

  const img = document.getElementById('detail-img');
  const w = img.naturalWidth || img.width || 1;
  const h = img.naturalHeight || img.height || 1;
  svg.setAttribute('viewBox', `0 0 ${w} ${h}`);

  for (const f of faces) {
    const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    rect.setAttribute('x', f.x);
    rect.setAttribute('y', f.y);
    rect.setAttribute('width', f.width);
    rect.setAttribute('height', f.height);
    rect.setAttribute('class', 'face-box');
    svg.appendChild(rect);
  }
}

function closeDetail() {
  document.getElementById('detail-modal').classList.add('hidden');
  document.getElementById('detail-faces').innerHTML = '';
  state.detailIdx = -1;
  resetDetailOriginalBtn();
}

function resetDetailOriginalBtn() {
  const btn = document.getElementById('detail-original-btn');
  btn.textContent = '⤢ 查看原图';
  btn.disabled = false;
  btn.dataset.mode = 'thumb';
}

async function toggleDetailOriginal() {
  const photo = state.detailPhotos[state.detailIdx];
  if (!photo) return;
  const btn = document.getElementById('detail-original-btn');
  const img = document.getElementById('detail-img');
  if (btn.dataset.mode !== 'original') {
    btn.disabled = true;
    btn.textContent = '加载中…';
    img.src = `/api/photos/${photo.id}/file`;
    await new Promise(resolve => {
      img.onload = resolve;
      img.onerror = resolve;
    });
    btn.disabled = false;
    btn.textContent = '⤡ 切换缩略图';
    btn.dataset.mode = 'original';
  } else {
    img.src = `/api/photos/${photo.id}/thumb`;
    btn.textContent = '⤢ 查看原图';
    btn.dataset.mode = 'thumb';
  }
}

function navigateDetail(delta) {
  const next = state.detailIdx + delta;
  if (next >= 0 && next < state.detailPhotos.length) openDetail(next, state.detailPhotos);
}

function updateDetailNav() {
  document.getElementById('detail-prev').disabled = state.detailIdx <= 0;
  document.getElementById('detail-next').disabled =
    state.detailIdx >= state.detailPhotos.length - 1;
}

function renderPagination() {
  const totalPages = Math.max(1, Math.ceil(state.total / state.perPage));
  document.getElementById('page-info').textContent = `${state.page} / ${totalPages}`;
  document.getElementById('prev-btn').disabled = state.page <= 1;
  document.getElementById('next-btn').disabled = state.page >= totalPages;
}

function changePage(delta) {
  state.page = Math.max(1, state.page + delta);
  loadPhotos();
}

// ── Albums ────────────────────────────────────────────────────────────────────
async function loadAlbums() {
  const albums = await fetchJSON('/api/albums');
  if (!albums) return;

  const ul = document.getElementById('album-list');
  ul.innerHTML = '';

  // "All photos" entry
  const allLi = document.createElement('li');
  allLi.className = 'album-entry';
  allLi.innerHTML = `全部照片 <span class="count"></span>`;
  allLi.addEventListener('click', () => selectAlbum(null, allLi));
  allLi.classList.add('active');
  ul.appendChild(allLi);

  const CATEGORIES = [
    { kind: 'camera',   label: '设备' },
    { kind: 'time',     label: '时间' },
    { kind: 'location', label: '地点' },
  ];
  const SHOW_DEFAULT = 4;

  for (const { kind, label } of CATEGORIES) {
    const sorted = albums
      .filter(a => a.kind === kind)
      .sort((a, b) => {
        if (!a.latest_photo_at && !b.latest_photo_at) return 0;
        if (!a.latest_photo_at) return 1;
        if (!b.latest_photo_at) return -1;
        return b.latest_photo_at.localeCompare(a.latest_photo_at);
      });
    if (sorted.length === 0) continue;

    const catLi = document.createElement('li');
    catLi.className = 'album-category';
    catLi.dataset.kind = kind;
    if (albumCategoryCollapsed[kind]) catLi.classList.add('collapsed');
    if (albumCategoryExpanded[kind])  catLi.classList.add('album-category-expanded');

    const headerDiv = document.createElement('div');
    headerDiv.className = 'album-category-header';
    const arrow = albumCategoryCollapsed[kind] ? '▶' : '▼';
    headerDiv.innerHTML = `<span class="album-category-toggle">${arrow}</span> ${label}`;
    headerDiv.addEventListener('click', () => toggleAlbumCategory(kind));

    const innerUl = document.createElement('ul');
    innerUl.className = 'album-category-list';

    sorted.forEach((a, idx) => {
      const li = document.createElement('li');
      li.className = 'album-entry';
      if (idx >= SHOW_DEFAULT) li.classList.add('album-hidden-extra');
      li.innerHTML = `${a.name} <span class="count">${a.photo_count}</span>`;
      li.addEventListener('click', () => selectAlbum(a.id, li));
      innerUl.appendChild(li);
    });

    if (sorted.length > SHOW_DEFAULT) {
      const actionLi = document.createElement('li');
      if (albumCategoryExpanded[kind]) {
        actionLi.className = 'album-more-link album-collapse-link';
        actionLi.textContent = '收起';
        actionLi.addEventListener('click', (e) => {
          e.stopPropagation();
          collapseAlbumCategory(kind);
        });
      } else {
        actionLi.className = 'album-more-link';
        actionLi.textContent = `更多 (${sorted.length - SHOW_DEFAULT})`;
        actionLi.addEventListener('click', (e) => {
          e.stopPropagation();
          expandAlbumCategory(kind);
        });
      }
      innerUl.appendChild(actionLi);
    }

    catLi.appendChild(headerDiv);
    catLi.appendChild(innerUl);
    ul.appendChild(catLi);
  }
}

function toggleAlbumCategory(kind) {
  albumCategoryCollapsed[kind] = !albumCategoryCollapsed[kind];
  const catLi = document.querySelector(`#album-list .album-category[data-kind="${kind}"]`);
  if (!catLi) return;
  catLi.classList.toggle('collapsed', albumCategoryCollapsed[kind]);
  catLi.querySelector('.album-category-toggle').textContent =
    albumCategoryCollapsed[kind] ? '▶' : '▼';
}

function expandAlbumCategory(kind) {
  albumCategoryExpanded[kind] = true;
  const catLi = document.querySelector(`#album-list .album-category[data-kind="${kind}"]`);
  if (!catLi) return;
  catLi.classList.add('album-category-expanded');
  const moreLi = catLi.querySelector('.album-more-link');
  if (moreLi) {
    moreLi.className = 'album-more-link album-collapse-link';
    moreLi.textContent = '收起';
    moreLi.onclick = (e) => { e.stopPropagation(); collapseAlbumCategory(kind); };
  }
}

function collapseAlbumCategory(kind) {
  albumCategoryExpanded[kind] = false;
  const catLi = document.querySelector(`#album-list .album-category[data-kind="${kind}"]`);
  if (!catLi) return;
  catLi.classList.remove('album-category-expanded');
  const collapseLi = catLi.querySelector('.album-collapse-link');
  if (collapseLi) {
    const hidden = catLi.querySelectorAll('.album-hidden-extra');
    collapseLi.className = 'album-more-link';
    collapseLi.textContent = `更多 (${hidden.length})`;
    collapseLi.onclick = (e) => { e.stopPropagation(); expandAlbumCategory(kind); };
  }
}

function selectAlbum(albumId, li) {
  document.querySelectorAll('#album-list .album-entry').forEach(el => el.classList.remove('active'));
  document.querySelectorAll('#collection-list .album-entry').forEach(el => el.classList.remove('active'));
  li.classList.add('active');
  state.albumId = albumId;
  state.collectionId = null;
  state.inCollection = false;
  state.page = 1;
  // Show "添加整个相册" only when a specific album (not "all") is selected
  document.getElementById('add-album-to-collection-btn').classList.toggle('hidden', !albumId);
  loadPhotos();
}

function selectCollection(collectionId, li) {
  document.querySelectorAll('#album-list .album-entry').forEach(el => el.classList.remove('active'));
  document.querySelectorAll('#collection-list .album-entry').forEach(el => el.classList.remove('active'));
  li.classList.add('active');
  state.collectionId = collectionId;
  state.inCollection = true;
  state.albumId = null;
  state.page = 1;
  document.getElementById('add-album-to-collection-btn').classList.add('hidden');
  loadPhotos();
}

// ── Collections ───────────────────────────────────────────────────────────────
async function loadCollections() {
  const collections = await fetchJSON('/api/collections');
  if (!collections) return;
  const ul = document.getElementById('collection-list');
  ul.innerHTML = '';
  collections.forEach(c => {
    const li = document.createElement('li');
    li.className = 'album-entry';
    if (state.inCollection && state.collectionId === c.id) li.classList.add('active');
    li.innerHTML = `
      <span class="collection-name" style="flex:1;overflow:hidden;text-overflow:ellipsis">${c.name}</span>
      <span class="count">${c.photo_count}</span>
      <button class="collection-rename-btn btn-ghost" title="改名"
              style="padding:1px 5px;font-size:11px;background:transparent;color:#6c7086;flex-shrink:0">✎</button>
      <button class="collection-delete-btn btn-ghost" title="删除"
              style="padding:1px 5px;font-size:11px;background:transparent;color:#6c7086;flex-shrink:0">×</button>`;
    li.style.display = 'flex';
    li.style.alignItems = 'center';
    li.style.gap = '2px';
    li.addEventListener('click', e => {
      if (e.target.closest('.collection-rename-btn') || e.target.closest('.collection-delete-btn')) return;
      selectCollection(c.id, li);
    });
    li.querySelector('.collection-rename-btn').addEventListener('click', e => {
      e.stopPropagation();
      const modal = document.getElementById('rename-collection-modal');
      modal.dataset.collectionId = c.id;
      document.getElementById('rename-collection-name').value = c.name;
      modal.classList.remove('hidden');
      document.getElementById('rename-collection-name').focus();
    });
    li.querySelector('.collection-delete-btn').addEventListener('click', e => {
      e.stopPropagation();
      deleteCollection(c.id, c.name);
    });
    ul.appendChild(li);
  });
}

async function createCollection(name) {
  const result = await fetch('/api/collections', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!result.ok) return;
  loadCollections();
}

async function renameCollection(id, name) {
  if (!name) return;
  await fetch(`/api/collections/${id}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  loadCollections();
}

async function deleteCollection(id, name) {
  if (!confirm(`删除精选集"${name}"？照片本身不受影响。`)) return;
  await fetch(`/api/collections/${id}`, { method: 'DELETE' });
  if (state.inCollection && state.collectionId === id) {
    state.inCollection = false;
    state.collectionId = null;
    state.albumId = null;
    state.page = 1;
    loadPhotos();
  }
  loadCollections();
}

async function showAddToCollectionModal(photoIds) {
  const modal = document.getElementById('add-to-collection-modal');
  modal.dataset.photoIds = JSON.stringify(photoIds);
  document.getElementById('add-to-collection-new-name').value = '';
  const collections = await fetchJSON('/api/collections');
  const ul = document.getElementById('add-to-collection-list');
  ul.innerHTML = '';
  if (!collections || collections.length === 0) {
    ul.innerHTML = '<li style="padding:8px;color:#888;font-size:12px">暂无精选集，请在下方新建</li>';
  } else {
    collections.forEach(c => {
      const li = document.createElement('li');
      li.style.cssText = 'padding:7px 10px;cursor:pointer;border-bottom:1px solid #f0f0f0';
      li.textContent = `${c.name} (${c.photo_count} 张)`;
      li.addEventListener('mouseover', () => li.style.background = '#f5f5f5');
      li.addEventListener('mouseout', () => li.style.background = '');
      li.addEventListener('click', async () => {
        await addPhotosToCollection(c.id, photoIds);
        modal.classList.add('hidden');
        loadCollections();
        if (state.inCollection && state.collectionId === c.id) loadPhotos();
      });
      ul.appendChild(li);
    });
  }
  modal.classList.remove('hidden');
}

async function addPhotosToCollection(collectionId, photoIds) {
  if (!photoIds.length) return;
  await fetch(`/api/collections/${collectionId}/photos`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ photo_ids: photoIds }),
  });
}

async function removePhotosFromCollection(collectionId, photoIds) {
  if (!photoIds.length) return;
  await fetch(`/api/collections/${collectionId}/photos`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ photo_ids: photoIds }),
  });
}

// ── Import ────────────────────────────────────────────────────────────────────
async function startImport() {
  const dir = document.getElementById('import-dir').value.trim();
  if (!dir) return;

  const btn = document.getElementById('import-btn');
  btn.disabled = true;
  setStatus('导入中…');

  await fetch('/api/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ dir }),
  });

  if (state.importPollId) clearInterval(state.importPollId);
  state.importPollId = setInterval(pollImport, 1500);
}

async function pollImport() {
  const s = await fetchJSON('/api/import/status');
  if (!s) return;

  if (s.running) {
    setStatus(`导入中：已处理 ${s.imported + s.skipped + s.errors} / ${s.total || '?'}`);
  } else {
    clearInterval(state.importPollId);
    state.importPollId = null;
    document.getElementById('import-btn').disabled = false;
    setStatus(`完成：导入 ${s.imported}，跳过 ${s.skipped}，失败 ${s.errors}`);
    loadAlbums();
    loadPhotos();
  }
}

function setStatus(msg) {
  document.getElementById('import-status').textContent = msg;
}

// ── Dedup ─────────────────────────────────────────────────────────────────────
async function openDedupModal() {
  const groups = await fetchJSON('/api/dedup');
  if (!groups) return;

  const container = document.getElementById('dedup-groups');
  container.innerHTML = '';

  if (groups.length === 0) {
    container.textContent = '没有待确认的重复组';
    document.getElementById('dedup-modal').classList.remove('hidden');
    return;
  }

  for (const g of groups) {
    const div = document.createElement('div');
    div.className = 'dedup-group';
    div.innerHTML = `<div class="dedup-group-header">
        <h3>重复组 #${g.group_id}</h3>
        <button class="compare-btn" title="放大比较原图">⛶ 放大比较</button>
      </div>
      <div class="dedup-members"></div>
      <div class="actions">
        <button class="resolve-btn" data-gid="${g.group_id}">确认保留选中项</button>
        <button class="skip-btn">跳过</button>
      </div>`;

    const membersDiv = div.querySelector('.dedup-members');
    for (const m of g.members) {
      const el = document.createElement('div');
      el.className = 'dedup-member';
      el.dataset.photoId = m.photo_id;
      const dimsHtml = m.width ? `<p class="dedup-dims">${m.width}×${m.height}</p>` : '';
      const dateHtml = m.taken_at ? `<p class="dedup-meta">${m.taken_at.slice(0, 10)}</p>` : '';
      const camHtml  = m.camera  ? `<p class="dedup-meta">${m.camera}</p>` : '';
      el.innerHTML = `
        <img src="/api/photos/${m.photo_id}/thumb" alt="">
        <p class="dedup-filename" title="${m.path}">${m.filename}</p>
        ${dimsHtml}${dateHtml}${camHtml}`;
      el.addEventListener('click', () => el.classList.toggle('selected'));
      membersDiv.appendChild(el);
    }

    div.querySelector('.resolve-btn').addEventListener('click', async (e) => {
      const gid = +e.target.dataset.gid;
      const keepIds = [...div.querySelectorAll('.dedup-member.selected')]
        .map(el => +el.dataset.photoId);
      if (keepIds.length === 0) { alert('请先选择要保留的照片'); return; }
      await fetch(`/api/dedup/${gid}/resolve`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ keep: keepIds }),
      });
      div.remove();
      loadPhotos();
    });

    div.querySelector('.skip-btn').addEventListener('click', () => div.remove());
    div.querySelector('.compare-btn').addEventListener('click', () => openCompareOverlay(g, div));
    container.appendChild(div);
  }

  document.getElementById('dedup-modal').classList.remove('hidden');
}

function openCompareOverlay(g, groupDiv) {
  const overlay = document.getElementById('dedup-compare-overlay');
  const imagesDiv = document.getElementById('dedup-compare-images');
  document.getElementById('dedup-compare-title').textContent = `重复组 #${g.group_id} — 点击照片选择保留项`;
  imagesDiv.innerHTML = '';

  for (const m of g.members) {
    const item = document.createElement('div');
    item.className = 'compare-item';
    item.dataset.photoId = m.photo_id;

    // Check if already selected in the main modal
    const mainEl = groupDiv.querySelector(`.dedup-member[data-photo-id="${m.photo_id}"]`);
    if (mainEl && mainEl.classList.contains('selected')) item.classList.add('selected');

    const dimsText = m.width ? `${m.width}×${m.height}` : '';
    const dateText = m.taken_at ? m.taken_at.slice(0, 10) : '';
    const camText  = m.camera  ? m.camera : '';
    item.innerHTML = `
      <img src="/api/photos/${m.photo_id}/file" alt="${m.filename}" loading="lazy">
      <div class="compare-meta">
        <div class="cm-filename">${m.filename}</div>
        ${dimsText ? `<div class="cm-dims">${dimsText}</div>` : ''}
        ${dateText ? `<div>${dateText}</div>` : ''}
        ${camText  ? `<div>${camText}</div>`  : ''}
      </div>
      <button class="compare-select-btn">${item.classList.contains('selected') ? '✓ 已选择保留' : '选择保留'}</button>`;

    const toggleSelection = () => {
      const selected = item.classList.toggle('selected');
      item.querySelector('.compare-select-btn').textContent = selected ? '✓ 已选择保留' : '选择保留';
      // Sync with main modal
      if (mainEl) mainEl.classList.toggle('selected', selected);
    };

    item.querySelector('img').addEventListener('click', toggleSelection);
    item.querySelector('.compare-select-btn').addEventListener('click', (e) => {
      e.stopPropagation();
      toggleSelection();
    });

    imagesDiv.appendChild(item);
  }

  overlay.classList.remove('hidden');
}

function closeCompareOverlay() {
  document.getElementById('dedup-compare-overlay').classList.add('hidden');
}

// ── View switching ────────────────────────────────────────────────────────────
function switchView(view) {
  state.currentView = view;
  document.querySelectorAll('.tab-btn[data-view]').forEach(b => {
    b.classList.toggle('active', b.dataset.view === view);
  });
  document.querySelectorAll('.view-section').forEach(s => s.classList.add('hidden'));
  document.getElementById(`view-${view}`).classList.remove('hidden');

  const albumsSection = document.getElementById('albums-section');
  albumsSection.style.display = view === 'photos' ? '' : 'none';

  if (view === 'people') loadPeopleList();
  if (view === 'locations') loadGeoHierarchy();
  if (view === 'activities') initActivitiesView();
}

// ── Geo view ──────────────────────────────────────────────────────────────────
let geoData = null; // cached hierarchy
let geoCurrentGeoFilter = null; // {country?, state?, city?}
let geoCurrentPage = 1;
const GEO_PER_PAGE = 50;

function hideGeoPhotos() {
  document.getElementById('geo-photos-section').style.display = 'none';
  geoCurrentGeoFilter = null;
  geoCurrentPage = 1;
}

async function loadGeoHierarchy() {
  geoData = await fetchJSON('/api/geo/hierarchy');
  if (!geoData) return;
  renderGeoCountries(geoData.countries);
  document.getElementById('geo-col-state').classList.add('hidden');
  document.getElementById('geo-col-city').classList.add('hidden');
  hideGeoPhotos();
  setBreadcrumb('地理位置');
}

function renderGeoCountries(countries) {
  const ul = document.getElementById('geo-country-list');
  ul.innerHTML = '';
  for (const c of countries) {
    const li = document.createElement('li');
    li.innerHTML = `<span>${c.name}</span><span class="geo-count">${c.photo_count}</span>`;
    li.addEventListener('click', () => {
      ul.querySelectorAll('li').forEach(x => x.classList.remove('active'));
      li.classList.add('active');
      setBreadcrumb(c.name);
      renderGeoStates(c);
      geoCurrentGeoFilter = { country: c.name === 'Unknown' ? '__null__' : c.name };
      geoCurrentPage = 1;
      loadGeoPhotos();
      refreshMapIfActive();
    });
    ul.appendChild(li);
  }
}

function renderGeoStates(country) {
  const stateCol = document.getElementById('geo-col-state');
  stateCol.classList.remove('hidden');
  document.getElementById('geo-col-city').classList.add('hidden');

  const ul = document.getElementById('geo-state-list');
  ul.innerHTML = '';
  for (const s of country.states) {
    const li = document.createElement('li');
    li.innerHTML = `<span>${s.name}</span><span class="geo-count">${s.photo_count}</span>`;
    li.addEventListener('click', () => {
      ul.querySelectorAll('li').forEach(x => x.classList.remove('active'));
      li.classList.add('active');
      setBreadcrumb(`${country.name} › ${s.name}`);
      renderGeoCities(s, country.name);
      geoCurrentGeoFilter = {
        country: country.name === 'Unknown' ? '__null__' : country.name,
        state: s.name === 'Unknown' ? '__null__' : s.name,
      };
      geoCurrentPage = 1;
      loadGeoPhotos();
      refreshMapIfActive();
    });
    ul.appendChild(li);
  }
}

function renderGeoCities(st, countryName) {
  const cityCol = document.getElementById('geo-col-city');
  cityCol.classList.remove('hidden');

  const ul = document.getElementById('geo-city-list');
  ul.innerHTML = '';
  for (const c of st.cities) {
    const li = document.createElement('li');
    li.innerHTML = `<span>${c.name}</span><span class="geo-count">${c.photo_count}</span>`;
    li.addEventListener('click', () => {
      ul.querySelectorAll('li').forEach(x => x.classList.remove('active'));
      li.classList.add('active');
      setBreadcrumb(`${countryName} › ${st.name} › ${c.name}`);
      const cityParam = c.name === 'Unknown' ? '__null__' : c.name;
      const countryParam = countryName === 'Unknown' ? '__null__' : countryName;
      const stateParam = st.name === 'Unknown' ? '__null__' : st.name;
      geoCurrentGeoFilter = { country: countryParam, state: stateParam, city: cityParam };
      geoCurrentPage = 1;
      loadGeoPhotos();
      refreshMapIfActive();
    });
    ul.appendChild(li);
  }
}

async function loadGeoPhotos() {
  if (!geoCurrentGeoFilter) return;
  const q = new URLSearchParams();
  const f = geoCurrentGeoFilter;
  if (f.country) q.set('country', f.country);
  if (f.state)   q.set('state',   f.state);
  if (f.city)    q.set('city',    f.city);
  q.set('page', geoCurrentPage);
  q.set('per_page', GEO_PER_PAGE);
  const data = await fetchJSON(`/api/geo/photos?${q}`);
  if (!data) return;
  renderGeoPhotos(data.photos || [], data.total || 0);
}

function renderGeoPhotos(photos, total) {
  const section = document.getElementById('geo-photos-section');
  const grid = document.getElementById('geo-photos');
  const pager = document.getElementById('geo-photos-pager');

  section.style.display = '';
  grid.scrollTop = 0;
  grid.innerHTML = '';

  photos.forEach((p, idx) => {
    const card = document.createElement('div');
    card.className = 'photo-card';
    const label = p.taken_at ? p.taken_at.slice(0, 10) : '';
    card.innerHTML = `<img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    card.addEventListener('click', () => openDetail(idx, photos));
    grid.appendChild(card);
  });

  const totalPages = Math.ceil(total / GEO_PER_PAGE);
  if (totalPages > 1) {
    pager.style.display = '';
    pager.innerHTML = `
      <button id="geo-prev-btn" class="btn-ghost" ${geoCurrentPage <= 1 ? 'disabled' : ''}>← 上一页</button>
      <span>${geoCurrentPage} / ${totalPages}（共 ${total} 张）</span>
      <button id="geo-next-btn" class="btn-ghost" ${geoCurrentPage >= totalPages ? 'disabled' : ''}>下一页 →</button>
    `;
    document.getElementById('geo-prev-btn').onclick = () => { geoCurrentPage--; loadGeoPhotos(); };
    document.getElementById('geo-next-btn').onclick = () => { geoCurrentPage++; loadGeoPhotos(); };
  } else {
    pager.style.display = 'none';
  }
}

function setBreadcrumb(text) {
  document.getElementById('geo-breadcrumb').textContent = text;
}

function switchGeoView(view) {
  document.querySelectorAll('[data-geoview]').forEach(b => {
    b.classList.toggle('active', b.dataset.geoview === view);
  });
  document.getElementById('geo-list-view').classList.toggle('hidden', view !== 'list');
  document.getElementById('geo-map-view').classList.toggle('hidden', view !== 'map');
  if (view === 'map') {
    setTimeout(() => refreshMapMarkers(), 50);
  }
}

function refreshMapIfActive() {
  if (!document.getElementById('geo-map-view').classList.contains('hidden')) {
    refreshMapMarkers();
  }
}

let leafletMap = null;
let _mapCluster = null;

async function ensureMapInit() {
  if (typeof L === 'undefined') {
    document.getElementById('leaflet-map').textContent = '地图需要网络连接才能加载（Leaflet CDN）';
    return false;
  }
  const mapEl = document.getElementById('leaflet-map');
  if (leafletMap) {
    if (leafletMap.getContainer().offsetWidth === 0) {
      leafletMap.remove();
      leafletMap = null;
      _mapCluster = null;
      mapEl.innerHTML = '';
    } else {
      leafletMap.invalidateSize();
      return true;
    }
  }
  leafletMap = L.map(mapEl).setView([20, 0], 2);
  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '© OpenStreetMap contributors',
    maxZoom: 19,
  }).addTo(leafletMap);
  return true;
}

async function refreshMapMarkers() {
  if (document.getElementById('geo-map-view').classList.contains('hidden')) return;
  const ok = await ensureMapInit();
  if (!ok) return;

  if (_mapCluster) { leafletMap.removeLayer(_mapCluster); _mapCluster = null; }

  const f = geoCurrentGeoFilter || {};
  const qs = new URLSearchParams();
  if (f.country) qs.set('country', f.country);
  if (f.state)   qs.set('state',   f.state);
  if (f.city)    qs.set('city',    f.city);
  const qstr = qs.toString();
  const pts = await fetchJSON(`/api/photos/gps-points${qstr ? '?' + qstr : ''}`);
  if (!pts || pts.length === 0) return;

  const cluster = L.markerClusterGroup();
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i];
    const marker = L.marker([p.gps_lat, p.gps_lon]);
    const date = p.taken_at ? p.taken_at.slice(0, 10) : '未知日期';
    marker.bindPopup(`
      <div style="text-align:center;cursor:pointer">
        <img src="/api/photos/${p.id}/thumb"
             style="max-width:120px;max-height:120px;display:block;margin:0 auto 4px;cursor:pointer">
        <div style="font-size:12px">${date}</div>
        <div style="font-size:11px;color:#6c7af4;margin-top:2px">点击查看详情</div>
      </div>`);
    marker.on('popupopen', () => {
      const pop = marker.getPopup().getElement();
      if (pop) pop.querySelector('img').addEventListener('click', () => openDetail(i, pts));
    });
    cluster.addLayer(marker);
  }
  leafletMap.addLayer(cluster);
  _mapCluster = cluster;

  const bounds = L.latLngBounds(pts.map(p => [p.gps_lat, p.gps_lon]));
  leafletMap.fitBounds(bounds, { padding: [40, 40] });
}

// ── People list ───────────────────────────────────────────────────────────────
async function loadPeopleList() {
  const people = await fetchJSON('/api/people');
  if (!people) return;
  state.allPeople = people;

  // Only show root-level people (parent_id === null) in the grid
  const rootPeople = people.filter(p => p.parent_id === null);
  rootPeople.sort((a, b) => {
    if (a.name && !b.name) return -1;
    if (!a.name && b.name) return 1;
    return b.photo_count - a.photo_count;
  });
  document.getElementById('people-count').textContent = `共 ${rootPeople.length} 人`;
  const grid = document.getElementById('people-grid');
  grid.innerHTML = '';

  if (rootPeople.length === 0) {
    grid.innerHTML = '<p style="padding:24px;color:#888">尚无人物。导入含人脸的照片后，点击"整合新面孔"。</p>';
    return;
  }

  for (const p of rootPeople) {
    const card = document.createElement('div');
    card.className = 'person-card';
    const thumbSrc = p.cover_face_id
      ? `/api/faces/${p.cover_face_id}/thumb`
      : '/default-person.svg';
    if (state.selectedPeople.has(p.id)) card.classList.add('selected');
    card.innerHTML = `
      <div class="person-card-check"></div>
      <img src="${thumbSrc}" loading="lazy" alt="">
      <div class="person-meta">
        <div class="person-name-cell" data-pid="${p.id}" data-name="${escHtml(p.name || '')}">${escHtml(p.name || '未命名')}</div>
        <div class="person-count">${(() => { const c = state.allPeople.filter(q => q.parent_id === p.id).length; return c > 0 ? `${p.photo_count} 张 · ${c} 个子人物` : `${p.photo_count} 张照片`; })()}</div>
      </div>
      <button class="person-menu-btn" aria-label="更多操作">⋯</button>`;

    // In select mode, clicking the card (not name or menu) toggles selection
    card.querySelector('img').addEventListener('click', () => {
      if (state.selectedPeople.size > 0 || document.getElementById('people-grid').classList.contains('people-select-mode')) {
        togglePersonSelect(p.id, card);
      } else {
        showPersonDetail(p.id);
      }
    });
    card.querySelector('.person-count').addEventListener('click', () => {
      if (state.selectedPeople.size > 0) {
        togglePersonSelect(p.id, card);
      } else {
        showPersonDetail(p.id);
      }
    });
    card.querySelector('.person-card-check').addEventListener('click', (e) => {
      e.stopPropagation();
      togglePersonSelect(p.id, card);
    });

    const nameCell = card.querySelector('.person-name-cell');
    nameCell.addEventListener('click', (e) => {
      e.stopPropagation();
      startInlineNameEdit(nameCell, p.id);
    });

    const menuBtn = card.querySelector('.person-menu-btn');
    menuBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      showPersonMenu(menuBtn, p.id, card);
    });

    grid.appendChild(card);
  }
}

function showPeopleList() {
  state.currentPersonId = null;
  state.currentPersonParentId = null;
  clearPeopleSelection();
  clearPersonDetailSelection();
  document.getElementById('person-detail-section').classList.add('detail-empty');
  document.getElementById('people-detail-empty').classList.remove('hidden');
  loadPeopleList();
}

// ── People multi-select & batch operations ────────────────────────────────────

function togglePersonSelect(personId, card) {
  if (state.selectedPeople.has(personId)) {
    state.selectedPeople.delete(personId);
    card.classList.remove('selected');
  } else {
    state.selectedPeople.add(personId);
    card.classList.add('selected');
  }
  updatePeopleBatchBar();
}

function clearPeopleSelection() {
  state.selectedPeople.clear();
  document.querySelectorAll('#people-grid .person-card.selected')
    .forEach(c => c.classList.remove('selected'));
  updatePeopleBatchBar();
}

function updatePeopleBatchBar() {
  const bar = document.getElementById('people-batch-bar');
  const n = state.selectedPeople.size;
  if (n > 0) {
    bar.classList.remove('hidden');
    document.getElementById('people-batch-count').textContent = `已选 ${n} 人`;
  } else {
    bar.classList.add('hidden');
  }
}

async function batchUpdatePeopleStatus(status) {
  const ids = [...state.selectedPeople];
  if (ids.length === 0) return;
  const label = status === 'ignored' ? '忽略' : '标记为非人物';
  if (!confirm(`确定要${label}选中的 ${ids.length} 位人物？此操作可撤销。`)) return;

  const resp = await fetch('/api/people/batch-update', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids, status }),
  });
  if (!resp.ok) return;

  // Remove affected cards from grid
  for (const pid of ids) {
    const card = [...document.querySelectorAll('#people-grid .person-card.selected')]
      .find(c => +c.querySelector('.person-name-cell')?.dataset.pid === pid);
    if (card) card.remove();
  }
  state.allPeople = state.allPeople.filter(p => !ids.includes(p.id));
  clearPeopleSelection();
  document.getElementById('people-count').textContent = `共 ${state.allPeople.length} 人`;

  const capturedIds = [...ids];
  pushUndo(`批量${label} ${capturedIds.length} 人`, async () => {
    await fetch('/api/people/batch-update', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ids: capturedIds, status: 'active' }),
    });
  });
}

function openPeopleNameMergeDialog() {
  if (state.selectedPeople.size < 1) return;
  document.getElementById('people-merge-name-input').value = '';
  document.getElementById('people-name-merge-modal').classList.remove('hidden');
  document.getElementById('people-merge-name-input').focus();
}

async function confirmPeopleNameMerge() {
  const ids = [...state.selectedPeople];
  if (ids.length === 0) return;
  const name = document.getElementById('people-merge-name-input').value.trim();
  document.getElementById('people-name-merge-modal').classList.add('hidden');

  // Pick primary: the person with the most photos; fall back to first id
  const primary = state.allPeople
    .filter(p => ids.includes(p.id))
    .sort((a, b) => b.photo_count - a.photo_count)[0];
  const primaryId = primary ? primary.id : ids[0];
  const others = ids.filter(id => id !== primaryId);

  // Check for duplicate name before proceeding (use primaryId as own)
  if (name) {
    const decision = await checkDuplicateName(name, primaryId);
    if (decision.action === 'same') {
      // Merge selected people into the existing matched person instead of primaryId
      const realTargetId = decision.targetId;
      const toMerge = ids.filter(id => id !== realTargetId);
      for (const srcId of toMerge) {
        await fetch('/api/people/merge', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ source_id: srcId, target_id: realTargetId }),
        });
      }
      clearPeopleSelection();
      loadPeopleList();
      return;
    }
  }

  // Merge others into primary
  for (const srcId of others) {
    await fetch('/api/people/merge', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source_id: srcId, target_id: primaryId }),
    });
  }

  // Rename primary if a name was provided
  if (name) {
    await patchPerson(primaryId, { name });
  }

  clearPeopleSelection();
  loadPeopleList();
}

async function showPersonDetail(personId) {
  state.currentPersonId = personId;
  clearPersonDetailSelection();
  document.getElementById('person-detail-section').classList.remove('detail-empty');
  document.getElementById('people-detail-empty').classList.add('hidden');

  // Load person info
  const people = state.allPeople;
  const person = people.find(p => p.id === personId);
  state.currentPersonParentId = person ? (person.parent_id ?? null) : null;
  document.getElementById('person-name-input').value = person ? (person.name || '') : '';
  document.getElementById('person-name-input').dataset.personId = personId;

  // Load photos for this person (paginated); eagerly hide bar to avoid stale state
  state.personDetailPage = 1;
  state.personDetailTotal = 0;
  document.getElementById('person-photos-pagination').classList.add('hidden');
  await loadPersonDetailPage(personId);

  // Breadcrumb and reparent panel
  document.getElementById('person-reparent-panel').classList.add('hidden');
  await updatePersonBreadcrumb(personId);

  // Show/hide "转移至兄弟" based on whether person has a parent
  updatePersonDetailSiblingBtn();

  // Load sub-persons
  await loadSubPersons(personId);

  // Load merge suggestions (named people only)
  await loadMergeSuggestions(personId, person);

  // Load outlier faces
  await loadOutlierFaces(personId);

  // Load centroid photo IDs for highlight
  await loadCentroidPhotoIds(personId);
  renderPersonDetailPhotos(state.personDetailPhotos);

  // Lazy-load embedding map (reset collapsed panel; data loads on expand)
  resetEmbeddingMapPanel(personId);
}

async function loadCentroidPhotoIds(personId) {
  const data = await fetchJSON(`/api/people/${personId}/centroid-faces`);
  state.centroidPhotoIds = new Set(data ? data.photo_ids : []);
  renderCentroidStats(data);
}

function renderCentroidStats(data) {
  // centroid-stats-text and centroid-debug-btn now live inside outlier-faces-panel
  const panel = document.getElementById('outlier-faces-panel');
  const textEl = document.getElementById('centroid-stats-text');
  if (!panel || !textEl) return;

  if (!data || data.emb_count === 0) {
    textEl.innerHTML = '';
    return; // panel visibility controlled by loadOutlierFaces
  }

  const { emb_count, centroid_size, min_dist, p25_dist, median_dist, p75_dist, max_dist } = data;
  const bar = (dist) => {
    const pct = Math.round((1 - dist) * 100);
    const color = pct >= 80 ? '#a6e3a1' : pct >= 65 ? '#f9e2af' : pct >= 50 ? '#fab387' : '#f38ba8';
    return `<span style="color:${color};font-weight:600">${pct}%</span>`;
  };
  textEl.innerHTML =
    `质心：用 <b>${centroid_size}</b>/${emb_count} 张人脸 &nbsp;·&nbsp; ` +
    `最近 ${bar(min_dist)} · P25 ${bar(p25_dist)} · 中位 ${bar(median_dist)} · P75 ${bar(p75_dist)} · 最远 ${bar(max_dist)}`;

  // Show panel even if no outlier faces, so centroid stats are always accessible
  panel.classList.remove('hidden');
}

// ── Embedding map ─────────────────────────────────────────────────────────────

const EMB_COLORS = ['#4c72b0','#dd8452','#55a868','#c44e52','#8172b2','#937860','#da8bc3','#8c8c8c'];
const EMB_CANVAS_H = 360;
const EMB_POINT_R  = 6;   // hit radius for hover/click
let   embMapState  = null; // { points, personColorMap, canvas, tooltip context }

function resetEmbeddingMapPanel(personId) {
  const panel = document.getElementById('embedding-map-panel');
  if (!panel) return;
  panel.classList.remove('hidden');
  panel.classList.add('panel-collapsed');
  panel.dataset.personId = personId;
  delete panel.dataset.loaded;
  const arrow = panel.querySelector('.panel-toggle-arrow');
  if (arrow) arrow.textContent = '▶';
  const body = document.getElementById('embedding-map-body');
  if (body) body.style.display = 'none';
  const canvas = document.getElementById('embedding-map-canvas');
  if (canvas) { canvas.width = 0; canvas.height = 0; }
  document.getElementById('embedding-map-empty').style.display = 'none';
  embMapState = null;
}

async function loadEmbeddingMap(personId) {
  const canvas  = document.getElementById('embedding-map-canvas');
  const empty   = document.getElementById('embedding-map-empty');
  const data    = await fetchJSON(`/api/people/${personId}/embedding-map`);
  if (!data || data.total < 2) {
    canvas.style.display = 'none';
    empty.style.display  = '';
    return;
  }

  canvas.style.display = '';
  empty.style.display  = 'none';

  const W = canvas.parentElement.clientWidth || 400;
  const H = EMB_CANVAS_H;
  canvas.width  = W;
  canvas.height = H;

  // Assign a colour per person_id
  const personIds = [...new Set(data.points.map(p => p.person_id))].sort((a,b)=>a-b);
  const colorMap  = {};
  personIds.forEach((pid, i) => { colorMap[pid] = EMB_COLORS[i % EMB_COLORS.length]; });

  // Compute canvas coordinates ([-1,1] → [pad, W-pad])
  const PAD = 24;
  const toCanvasX = x => PAD + (x + 1) / 2 * (W - 2*PAD);
  const toCanvasY = y => PAD + (1 - (y + 1) / 2) * (H - 2*PAD);
  const displayPoints = data.points.map(pt => ({
    ...pt,
    cx: toCanvasX(pt.x),
    cy: toCanvasY(pt.y),
    color: colorMap[pt.person_id],
  }));

  embMapState = { points: displayPoints, colorMap, personIds, canvas, personId };
  drawEmbeddingMap();
}

function drawEmbeddingMap() {
  if (!embMapState) return;
  const { points, canvas } = embMapState;
  const ctx = canvas.getContext('2d');
  const W = canvas.width, H = canvas.height;
  ctx.clearRect(0, 0, W, H);

  // Subtle grid lines
  ctx.strokeStyle = '#e8e8e8';
  ctx.lineWidth = 1;
  const cx = W / 2, cy = H / 2;
  ctx.beginPath(); ctx.moveTo(cx, 0); ctx.lineTo(cx, H); ctx.stroke();
  ctx.beginPath(); ctx.moveTo(0, cy); ctx.lineTo(W, cy); ctx.stroke();

  // Draw points
  for (const pt of points) {
    ctx.beginPath();
    ctx.arc(pt.cx, pt.cy, EMB_POINT_R, 0, Math.PI * 2);
    ctx.fillStyle = pt.color;
    ctx.globalAlpha = 0.35 + 0.65 * Math.min(pt.confidence, 1.0);
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.strokeStyle = '#fff';
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  // Legend (top-right)
  drawEmbMapLegend(ctx, W);
}

function drawEmbMapLegend(ctx, W) {
  const { personIds, colorMap } = embMapState;
  const allPeople = state.allPeople || [];
  let y = 16;
  const x = W - 12;
  ctx.font = '11px system-ui,sans-serif';
  ctx.textAlign = 'right';
  const maxShown = 8;
  const shown = personIds.slice(0, maxShown);
  for (const pid of shown) {
    const p = allPeople.find(pp => pp.id === pid);
    const label = p?.name || `#${pid}`;
    ctx.fillStyle = colorMap[pid];
    ctx.beginPath();
    ctx.arc(x - ctx.measureText(label).width - 14, y - 3, 5, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#555';
    ctx.fillText(label, x, y);
    y += 16;
  }
  if (personIds.length > maxShown) {
    ctx.fillStyle = '#aaa';
    ctx.fillText(`及其他 ${personIds.length - maxShown} 人`, x, y);
  }
  ctx.textAlign = 'left';
}

function embMapHitTest(cx, cy) {
  if (!embMapState) return null;
  let best = null, bestDist = EMB_POINT_R * 2;
  for (const pt of embMapState.points) {
    const d = Math.hypot(pt.cx - cx, pt.cy - cy);
    if (d < bestDist) { bestDist = d; best = pt; }
  }
  return best;
}

(function wireEmbeddingMapEvents() {
  const canvas  = document.getElementById('embedding-map-canvas');
  const tooltip = document.getElementById('emb-tooltip');
  if (!canvas || !tooltip) return;

  canvas.addEventListener('mousemove', e => {
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width  / rect.width;
    const scaleY = canvas.height / rect.height;
    const cx = (e.clientX - rect.left)  * scaleX;
    const cy = (e.clientY - rect.top)   * scaleY;
    const pt = embMapHitTest(cx, cy);
    if (!pt) {
      tooltip.style.display = 'none';
      return;
    }
    // Position tooltip
    const W = window.innerWidth, H = window.innerHeight;
    let left = e.clientX + 12, top = e.clientY + 12;
    tooltip.style.display = '';
    const tw = tooltip.offsetWidth  || 100;
    const th = tooltip.offsetHeight || 90;
    if (left + tw > W - 8) left = e.clientX - tw - 12;
    if (top  + th > H - 8) top  = e.clientY - th - 12;
    tooltip.style.left = `${Math.max(8, left)}px`;
    tooltip.style.top  = `${Math.max(8, top)}px`;

    // Fill tooltip content
    document.getElementById('emb-tooltip-img').src = `/api/faces/${pt.face_id}/thumb`;
    document.getElementById('emb-tooltip-date').textContent = pt.taken_at ? pt.taken_at.slice(0, 10) : '';
    document.getElementById('emb-tooltip-conf').textContent = `置信度 ${Math.round(pt.confidence * 100)}%`;
    const person = (state.allPeople || []).find(p => p.id === pt.person_id);
    document.getElementById('emb-tooltip-person').textContent = person?.name || '';
  });

  canvas.addEventListener('mouseleave', () => {
    tooltip.style.display = 'none';
  });

  canvas.addEventListener('click', e => {
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width  / rect.width;
    const scaleY = canvas.height / rect.height;
    const cx = (e.clientX - rect.left) * scaleX;
    const cy = (e.clientY - rect.top)  * scaleY;
    const pt = embMapHitTest(cx, cy);
    if (pt) openPhotoDetail(pt.photo_id);
  });
})();

function showFaceLightbox(src) {
  const lb = document.getElementById('face-lightbox');
  const img = document.getElementById('face-lightbox-img');
  img.src = src;
  lb.style.display = 'flex';
}

function hideFaceLightbox() {
  document.getElementById('face-lightbox').style.display = 'none';
}

async function openCentroidDebugModal(personId) {
  const modal = document.getElementById('centroid-debug-modal');
  const list = document.getElementById('centroid-debug-list');
  const selCount = document.getElementById('centroid-debug-sel-count');
  const createBtn = document.getElementById('centroid-debug-create-child');
  const selectAllBtn = document.getElementById('centroid-debug-select-all');
  if (!modal || !list) return;

  const selected = new Set();
  const facePhotoMap = new Map();

  const updateSelUI = () => {
    const n = selected.size;
    document.getElementById('centroid-debug-sel-count').textContent = n ? `已选 ${n} 张` : '未选中';
    const cb = document.getElementById('centroid-debug-create-child');
    if (cb) cb.disabled = n === 0;
    const sa = document.getElementById('centroid-debug-select-all');
    if (sa) sa.textContent = (n > 0 && n === facePhotoMap.size) ? '取消全选' : '全选';
  };

  list.innerHTML = '<span style="color:#aaa;font-size:13px">加载中…</span>';
  modal.classList.remove('hidden');

  const faces = await fetchJSON(`/api/people/${personId}/outlier-faces?limit=40&min_dist=0`);
  list.innerHTML = '';
  selected.clear();
  facePhotoMap.clear();
  updateSelUI();

  if (!faces || faces.length === 0) {
    list.innerHTML = '<span style="color:#aaa;font-size:13px">无人脸数据</span>';
    return;
  }

  for (const f of faces) {
    facePhotoMap.set(f.face_id, f.photo_id);
    const pct = Math.round((1 - f.distance) * 100);
    const color = pct >= 80 ? '#a6e3a1' : pct >= 65 ? '#f9e2af' : pct >= 50 ? '#fab387' : '#f38ba8';

    const card = document.createElement('div');
    card.style.cssText = 'width:90px;text-align:center;flex-shrink:0;position:relative';

    const src = `/api/faces/${f.face_id}/thumb`;
    const img = document.createElement('img');
    img.src = src;
    img.style.cssText = 'width:90px;height:90px;object-fit:cover;border-radius:6px;display:block;cursor:pointer;border:2px solid #ddd';
    img.onerror = () => { img.style.border = '2px solid #eee'; };
    img.addEventListener('click', (e) => {
      if (e.shiftKey) { showFaceLightbox(src); return; }
      if (selected.has(f.face_id)) {
        selected.delete(f.face_id);
        img.style.border = '2px solid #ddd';
        img.style.opacity = '1';
      } else {
        selected.add(f.face_id);
        img.style.border = `3px solid ${color}`;
        img.style.opacity = '0.85';
      }
      updateSelUI();
    });

    const simEl = document.createElement('div');
    simEl.style.cssText = `font-size:12px;font-weight:600;margin:3px 0 2px;color:${color}`;
    simEl.textContent = `${pct}%`;

    const ejectBtn = document.createElement('button');
    ejectBtn.textContent = '单独移出';
    ejectBtn.className = 'btn-ghost';
    ejectBtn.style.cssText = 'font-size:10px;padding:1px 6px;color:#c0392b;width:100%';
    ejectBtn.addEventListener('click', async () => {
      ejectBtn.disabled = true;
      const resp = await fetch(`/api/people/${personId}/eject-face`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ face_id: f.face_id }),
      });
      if (resp.ok) {
        selected.delete(f.face_id);
        facePhotoMap.delete(f.face_id);
        card.remove();
        updateSelUI();
        await loadCentroidPhotoIds(personId);
        await loadPersonDetailPage(personId);
        loadPeopleList();
      } else { ejectBtn.disabled = false; }
    });

    card.append(img, simEl, ejectBtn);
    list.appendChild(card);
  }

  // Wire up toolbar buttons via onclick (overwrites any previous handler)
  document.getElementById('centroid-debug-select-all').onclick = () => {
    const allSelected = selected.size > 0 && selected.size === facePhotoMap.size;
    if (allSelected) {
      selected.clear();
      list.querySelectorAll('img').forEach(img => { img.style.border = '2px solid #ddd'; img.style.opacity = '1'; });
    } else {
      facePhotoMap.forEach((_, fid) => selected.add(fid));
      list.querySelectorAll('img').forEach(img => { img.style.border = '3px solid #a78bfa'; img.style.opacity = '0.85'; });
    }
    updateSelUI();
  };

  document.getElementById('centroid-debug-create-child').onclick = async () => {
    if (selected.size === 0) return;
    const photoIds = [...selected].map(fid => facePhotoMap.get(fid));
    modal.classList.add('hidden');
    await createSubPerson(photoIds);
    await loadCentroidPhotoIds(personId);
    loadOutlierFaces(personId);
  };
}

document.addEventListener('DOMContentLoaded', () => {
  const lb = document.getElementById('face-lightbox');
  if (lb) lb.addEventListener('click', hideFaceLightbox);

  const closeDebug = document.getElementById('centroid-debug-close');
  if (closeDebug) closeDebug.addEventListener('click', () => {
    document.getElementById('centroid-debug-modal').classList.add('hidden');
  });
  document.getElementById('centroid-debug-modal')?.addEventListener('click', e => {
    if (e.target === e.currentTarget) e.currentTarget.classList.add('hidden');
  });

  document.addEventListener('click', e => {
    const header = e.target.closest('.panel-toggle-header');
    if (!header) return;
    const panel = header.closest('[id$="-panel"]');
    if (!panel) return;
    panel.classList.toggle('panel-collapsed');
    const arrow = header.querySelector('.panel-toggle-arrow');
    if (arrow) arrow.textContent = panel.classList.contains('panel-collapsed') ? '▶' : '▼';
    // Lazy-load embedding map on first expand
    if (panel.id === 'embedding-map-panel' && !panel.classList.contains('panel-collapsed')) {
      const body = document.getElementById('embedding-map-body');
      if (body) body.style.display = '';
      if (panel.dataset.personId && !panel.dataset.loaded) {
        panel.dataset.loaded = '1';
        loadEmbeddingMap(parseInt(panel.dataset.personId));
      }
    }
  });
});

async function loadMergeSuggestions(personId, person) {
  const panel = document.getElementById('merge-suggestions-panel');
  const list = document.getElementById('merge-suggestions-list');
  list.innerHTML = '';
  if (!person || !person.name) {
    panel.classList.add('hidden');
    return;
  }
  const suggestions = await fetchJSON(`/api/people/${personId}/merge-suggestions?limit=20`);
  if (!suggestions || suggestions.length === 0) {
    panel.classList.add('hidden');
    return;
  }
  panel.classList.remove('hidden');
  panel.classList.add('panel-collapsed');

  // Sort: named first (by distance), then unnamed (by distance)
  const named   = suggestions.filter(s =>  s.name);
  const unnamed = suggestions.filter(s => !s.name);

  const addSeparator = (label) => {
    const sep = document.createElement('div');
    sep.style.cssText = 'flex-shrink:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:4px;padding:0 6px';
    sep.innerHTML = `<div style="width:1px;flex:1;background:#ddd"></div><span style="font-size:10px;color:#aaa;white-space:nowrap;writing-mode:vertical-lr">${label}</span><div style="width:1px;flex:1;background:#ddd"></div>`;
    list.appendChild(sep);
  };

  const allGrouped = [];
  if (named.length)   allGrouped.push({ label: '已命名', items: named });
  if (unnamed.length) allGrouped.push({ label: '未命名', items: unnamed });

  let firstGroup = true;
  for (const group of allGrouped) {
    if (!firstGroup) addSeparator(group.label);
    else if (allGrouped.length > 1) {
      // Label for first group: insert header label before cards
      const lbl = document.createElement('div');
      lbl.style.cssText = 'flex-shrink:0;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:0 6px';
      lbl.innerHTML = `<span style="font-size:10px;color:#aaa;white-space:nowrap;writing-mode:vertical-lr">${group.label}</span>`;
      list.appendChild(lbl);
    }
    firstGroup = false;
    for (const s of group.items) {
    const pct = Math.round((1 - s.distance) * 100);
    const card = document.createElement('div');
    card.style.cssText = 'flex-shrink:0;width:80px;text-align:center';

    const thumbSrc = s.cover_face_id ? `/api/faces/${s.cover_face_id}/thumb` : null;
    const img = document.createElement('img');
    if (thumbSrc) {
      img.src = thumbSrc;
      img.style.cssText = 'width:80px;height:80px;object-fit:cover;border-radius:6px;border:2px solid #a78bfa;display:block;cursor:zoom-in';
      img.onerror = () => { img.style.border = '2px solid #ccc'; };
      img.addEventListener('click', () => showFaceLightbox(thumbSrc));
    } else {
      img.style.display = 'none';
    }

    const nameEl = document.createElement('div');
    nameEl.style.cssText = 'font-size:11px;font-weight:600;margin:2px 0 1px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis';
    nameEl.textContent = s.name || '未命名';

    const meta = document.createElement('div');
    meta.style.cssText = 'font-size:10px;color:#888;margin-bottom:2px';
    meta.textContent = `${s.photo_count} 张 · ${pct}%`;

    const mergeBtn = document.createElement('button');
    mergeBtn.textContent = '合并';
    mergeBtn.className = 'btn-ghost';
    mergeBtn.style.cssText = 'font-size:10px;padding:1px 5px;width:100%';
    mergeBtn.addEventListener('click', () => {
      const doMerge = async () => {
        mergeBtn.disabled = true;
        await fetch('/api/people/merge', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ source_id: s.person_id, target_id: personId }),
        });
        card.remove();
        if (list.children.length === 0) panel.classList.add('hidden');
        await loadPersonDetailPage(personId);
        loadPeopleList();
      };
      if (s.name) {
        showMergeConfirm(`将「${s.name}」（${s.photo_count} 张）合并入当前人物？`, doMerge);
      } else {
        doMerge();
      }
    });

    card.append(img, nameEl, meta, mergeBtn);
    list.appendChild(card);
    } // end group.items loop
  } // end allGrouped loop
}

async function loadOutlierFaces(personId) {
  // Panel visibility is controlled by renderCentroidStats; we only populate the list
  const list = document.getElementById('outlier-faces-list');
  list.innerHTML = '';

  const outliers = await fetchJSON(`/api/people/${personId}/outlier-faces?limit=5`);
  if (!outliers || outliers.length === 0) return;

  const panel = document.getElementById('outlier-faces-panel');
  panel.classList.add('panel-collapsed');

  // Track dismissed face IDs in this session
  const dismissed = new Set();

  for (const o of outliers) {
    const card = document.createElement('div');
    card.dataset.faceId = o.face_id;
    card.style.cssText = 'flex-shrink:0;width:80px;text-align:center';

    const faceSrc = `/api/faces/${o.face_id}/thumb`;
    const img = document.createElement('img');
    img.src = faceSrc;
    img.style.cssText = 'width:80px;height:80px;object-fit:cover;border-radius:6px;border:2px solid #e74c3c;display:block;cursor:zoom-in';
    img.onerror = () => { img.style.border = '2px solid #ccc'; };
    img.addEventListener('click', () => showFaceLightbox(faceSrc));

    const pct = Math.round((1 - o.distance) * 100);
    const meta = document.createElement('div');
    meta.style.cssText = 'font-size:10px;color:#888;margin:2px 0';
    meta.textContent = `相似度 ${pct}%`;

    const btnRow = document.createElement('div');
    btnRow.style.cssText = 'display:flex;gap:2px;justify-content:center;margin-top:2px';

    const ejectBtn = document.createElement('button');
    ejectBtn.textContent = '移出';
    ejectBtn.className = 'btn-ghost';
    ejectBtn.style.cssText = 'font-size:10px;padding:1px 5px;color:#c0392b';
    ejectBtn.addEventListener('click', async () => {
      ejectBtn.disabled = true;
      keepBtn.disabled = true;
      const resp = await fetch(`/api/people/${personId}/eject-face`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ face_id: o.face_id }),
      });
      if (resp.ok) {
        card.remove();
        await loadCentroidPhotoIds(personId);
        await loadPersonDetailPage(personId);
        loadPeopleList();
      } else {
        ejectBtn.disabled = false;
        keepBtn.disabled = false;
      }
    });

    const keepBtn = document.createElement('button');
    keepBtn.textContent = '保留';
    keepBtn.className = 'btn-ghost';
    keepBtn.style.cssText = 'font-size:10px;padding:1px 5px';
    keepBtn.addEventListener('click', () => {
      dismissed.add(o.face_id);
      card.remove();
    });

    btnRow.append(ejectBtn, keepBtn);
    card.append(img, meta, btnRow);
    list.appendChild(card);
  }
}

async function loadPersonDetailPage(personId) {
  const page = state.personDetailPage;
  const data = await fetchJSON(
    `/api/people/${personId}?page=${page}&per_page=${PERSON_DETAIL_PER_PAGE}`
  );
  // Discard stale responses that arrived after the user switched persons
  if (!data || personId !== state.currentPersonId) return;
  state.personDetailPhotos = data.photos || [];
  state.personDetailTotal = data.total || 0;
  renderPersonDetailPhotos(state.personDetailPhotos);
  renderPersonDetailPagination();
}

function renderPersonDetailPagination() {
  const bar = document.getElementById('person-photos-pagination');
  const total = state.personDetailTotal;
  const page  = state.personDetailPage;
  const pages = Math.ceil(total / PERSON_DETAIL_PER_PAGE);
  if (pages <= 1) { bar.classList.add('hidden'); return; }
  bar.classList.remove('hidden');
  bar.innerHTML = '';
  const prev = document.createElement('button');
  prev.className = 'btn-ghost'; prev.textContent = '‹ 上页';
  prev.disabled = page <= 1;
  prev.addEventListener('click', () => {
    state.personDetailPage--;
    loadPersonDetailPage(state.currentPersonId);
  });
  const info = document.createElement('span');
  info.textContent = `第 ${page} / ${pages} 页 · 共 ${total} 张`;
  const next = document.createElement('button');
  next.className = 'btn-ghost'; next.textContent = '下页 ›';
  next.disabled = page >= pages;
  next.addEventListener('click', () => {
    state.personDetailPage++;
    loadPersonDetailPage(state.currentPersonId);
  });
  bar.append(prev, info, next);
}

function renderPersonDetailPhotos(photos) {
  const grid = document.getElementById('person-photos-grid');
  grid.innerHTML = '';
  photos.forEach((p, idx) => {
    const card = document.createElement('div');
    card.className = 'photo-card';
    if (state.personDetailSelectMode) card.classList.add('select-mode');
    if (state.personDetailSelection.has(p.id)) card.classList.add('selected');
    if (state.centroidPhotoIds.has(p.id)) card.classList.add('centroid-photo');
    card.dataset.photoId = p.id;
    const label = p.taken_at ? p.taken_at.slice(0, 10) : '';
    card.innerHTML = `<div class="card-check"></div>
      <img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    card.addEventListener('click', () => {
      if (state.personDetailSelectMode) {
        togglePersonDetailPhoto(p.id, card);
      } else {
        openDetail(idx, photos);
      }
    });
    grid.appendChild(card);
  });
}

function updatePersonDetailSiblingBtn() {
  const btn = document.getElementById('person-detail-transfer-sibling-btn');
  if (state.currentPersonParentId !== null) {
    btn.classList.remove('hidden');
  } else {
    btn.classList.add('hidden');
  }
}

async function loadSubPersons(personId) {
  const tree = await fetchJSON('/api/people/tree');
  if (!tree) return;

  const findChildren = (nodes, targetId) => {
    for (const n of nodes) {
      if (n.id === targetId) return n.children || [];
      const found = findChildren(n.children || [], targetId);
      if (found !== null) return found;
    }
    return null;
  };
  const children = findChildren(tree.people || [], personId) || [];

  const list = document.getElementById('subpeople-list');
  list.innerHTML = '';
  if (children.length === 0) {
    list.innerHTML = '<p style="font-size:12px;color:#aaa">无子人物</p>';
    return;
  }
  for (const child of children) {
    const row = document.createElement('div');
    row.className = 'subperson-row';
    const thumbSrc = child.cover_face_id
      ? `/api/faces/${child.cover_face_id}/thumb`
      : '/default-person.svg';
    row.innerHTML = `<img class="subperson-thumb" src="${thumbSrc}" alt="">
      <span class="subperson-name">${escHtml(child.name || '未命名')}</span>
      <div class="subperson-actions">
        <button class="btn-ghost" data-action="top">移至顶级</button>
        <button class="btn-ghost" data-action="pick">移至…</button>
      </div>`;
    row.querySelector('.subperson-thumb').addEventListener('click', () => showPersonDetail(child.id));
    row.querySelector('.subperson-name').addEventListener('click', () => showPersonDetail(child.id));
    row.querySelector('[data-action="top"]').addEventListener('click', async () => {
      const resp = await fetch(`/api/people/${child.id}/reparent`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ new_parent_id: null }),
      });
      if (resp.ok) {
        pushUndo(`移出子人物"${child.name || '未命名'}"`, async () => {
          await fetch(`/api/people/${child.id}/reparent`, {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ new_parent_id: personId }),
          });
        });
      }
      loadSubPersons(personId);
    });
    row.querySelector('[data-action="pick"]').addEventListener('click', () => {
      openReparentPickerForChild(child.id, child.name || '未命名', personId);
    });
    list.appendChild(row);
  }
}

// ── Person detail: photo selection mode ──────────────────────────────────────

function togglePersonDetailSelectMode() {
  state.personDetailSelectMode = !state.personDetailSelectMode;
  document.getElementById('person-select-toggle-btn').textContent =
    state.personDetailSelectMode ? '退出选择' : '选择照片';
  renderPersonDetailPhotos(state.personDetailPhotos);
  updatePersonDetailBatchBar();
}

function togglePersonDetailPhoto(photoId, card) {
  if (state.personDetailSelection.has(photoId)) {
    state.personDetailSelection.delete(photoId);
    card.classList.remove('selected');
  } else {
    state.personDetailSelection.add(photoId);
    card.classList.add('selected');
  }
  updatePersonDetailBatchBar();
}

function clearPersonDetailSelection() {
  state.personDetailSelectMode = false;
  state.personDetailSelection.clear();
  const btn = document.getElementById('person-select-toggle-btn');
  if (btn) btn.textContent = '选择照片';
  updatePersonDetailBatchBar();
}

function updatePersonDetailBatchBar() {
  const bar = document.getElementById('person-detail-batch-bar');
  const n = state.personDetailSelection.size;
  if (n > 0) {
    bar.classList.remove('hidden');
    document.getElementById('person-detail-batch-count').textContent = `已选 ${n} 张`;
  } else {
    bar.classList.add('hidden');
  }
}

// ── Person detail: name input dialog ─────────────────────────────────────────

let _personDetailNameResolve = null;

function showPersonDetailNameDialog(title, desc = '') {
  document.getElementById('person-detail-name-title').textContent = title;
  document.getElementById('person-detail-name-desc').textContent = desc;
  document.getElementById('person-detail-name-input').value = '';
  document.getElementById('person-detail-name-modal').classList.remove('hidden');
  document.getElementById('person-detail-name-input').focus();

  return new Promise(resolve => {
    _personDetailNameResolve = resolve;
    document.getElementById('person-detail-name-confirm').onclick = () => {
      const name = document.getElementById('person-detail-name-input').value.trim();
      document.getElementById('person-detail-name-modal').classList.add('hidden');
      if (_personDetailNameResolve) { _personDetailNameResolve(name); _personDetailNameResolve = null; }
    };
    document.getElementById('person-detail-name-input').onkeydown = (e) => {
      if (e.key === 'Enter') document.getElementById('person-detail-name-confirm').click();
      if (e.key === 'Escape') {
        document.getElementById('person-detail-name-modal').classList.add('hidden');
        if (_personDetailNameResolve) { _personDetailNameResolve(null); _personDetailNameResolve = null; }
      }
    };
  });
}

// ── Person detail: create sub-person ─────────────────────────────────────────

async function createSubPerson(photoIds) {
  if (photoIds.length === 0) return;
  const currentPersonId = state.currentPersonId;

  const name = await showPersonDetailNameDialog('创建子人物', `将 ${photoIds.length} 张照片中的人脸创建为子人物`);
  if (name === null) return; // cancelled

  const decision = name ? await checkDuplicateName(name, null) : { action: 'different' };

  if (decision.action === 'same') {
    const targetId = decision.targetId;
    // Reparent existing person as child of current, then transfer faces
    await fetch(`/api/people/${targetId}/reparent`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ new_parent_id: currentPersonId }),
    });
    const resp = await fetch(`/api/people/${currentPersonId}/transfer`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ target_person_id: targetId, photo_ids: photoIds }),
    });
    if (resp.ok) {
      const { faces_moved } = await resp.json();
      const matchedPerson = state.allPeople.find(p => p.id === targetId);
      const originalParentId = matchedPerson ? (matchedPerson.parent_id ?? null) : null;
      pushUndo(`子人物（已有人物 #${targetId}）`, async () => {
        await fetch(`/api/people/${currentPersonId}/transfer`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ target_person_id: currentPersonId, photo_ids: photoIds }),
        });
        await fetch(`/api/people/${targetId}/reparent`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ new_parent_id: originalParentId }),
        });
      });
    }
  } else {
    // Create new child person
    const createResp = await fetch('/api/people', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name || null, parent_id: currentPersonId }),
    });
    if (!createResp.ok) return;
    const { id: newId } = await createResp.json();

    const transferResp = await fetch(`/api/people/${currentPersonId}/transfer`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ target_person_id: newId, photo_ids: photoIds }),
    });
    if (transferResp.ok) {
      pushUndo(`创建子人物"${name || '未命名'}"`, async () => {
        // Transfer faces back, then delete empty new person
        await fetch(`/api/people/${newId}/transfer`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ target_person_id: currentPersonId, photo_ids: photoIds }),
        });
        await fetch(`/api/people/${newId}`, { method: 'DELETE' });
      });
    }
  }

  clearPersonDetailSelection();
  await refreshPersonDetail();
}

// ── Person detail: create parent node ────────────────────────────────────────

async function startCreateParent() {
  const currentPersonId = state.currentPersonId;
  const originalParentId = state.currentPersonParentId;

  const name = await showPersonDetailNameDialog('创建父节点', '在当前人物上方插入新的父节点');
  if (name === null) return; // cancelled

  const decision = name ? await checkDuplicateName(name, null) : { action: 'different' };

  if (decision.action === 'same') {
    // Use existing person as parent (just reparent)
    const resp = await fetch(`/api/people/${currentPersonId}/reparent`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ new_parent_id: decision.targetId }),
    });
    if (resp.ok) {
      state.currentPersonParentId = decision.targetId;
      pushUndo('创建父节点（已有人物）', async () => {
        await fetch(`/api/people/${currentPersonId}/reparent`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ new_parent_id: originalParentId }),
        });
        state.currentPersonParentId = originalParentId;
        await updatePersonBreadcrumb(currentPersonId);
        updatePersonDetailSiblingBtn();
      });
      await updatePersonBreadcrumb(currentPersonId);
      updatePersonDetailSiblingBtn();
    }
  } else {
    // Create new parent via lift endpoint
    const resp = await fetch(`/api/people/${currentPersonId}/lift`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name || '' }),
    });
    if (!resp.ok) return;
    const { new_person_id: newParentId } = await resp.json();
    state.currentPersonParentId = newParentId;
    pushUndo(`创建父节点"${name || '未命名'}"`, async () => {
      await fetch(`/api/people/${currentPersonId}/reparent`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ new_parent_id: originalParentId }),
      });
      await fetch(`/api/people/${newParentId}`, { method: 'DELETE' });
      state.currentPersonParentId = originalParentId;
      await updatePersonBreadcrumb(currentPersonId);
      updatePersonDetailSiblingBtn();
    });
    await updatePersonBreadcrumb(currentPersonId);
    updatePersonDetailSiblingBtn();
  }
}

// ── Person detail: transfer to sibling ───────────────────────────────────────

async function startTransferToSibling(photoIds) {
  if (photoIds.length === 0 || state.currentPersonParentId === null) return;
  const currentPersonId = state.currentPersonId;
  const parentId = state.currentPersonParentId;

  // Load siblings (same parent, exclude self)
  const allPeople = await fetchJSON('/api/people?status=all') || [];
  const siblings = allPeople.filter(p => p.parent_id === parentId && p.id !== currentPersonId);

  const list = document.getElementById('sibling-picker-list');
  list.innerHTML = '';

  // "New sibling" option
  const newItem = document.createElement('div');
  newItem.className = 'reparent-item';
  newItem.textContent = '＋ 新建兄弟人物';
  newItem.style.fontWeight = '600';
  newItem.addEventListener('click', async () => {
    document.getElementById('sibling-picker-modal').classList.add('hidden');
    await createNewSiblingAndTransfer(currentPersonId, parentId, photoIds);
  });
  list.appendChild(newItem);

  for (const sib of siblings.sort((a, b) => (a.name || '').localeCompare(b.name || ''))) {
    const item = document.createElement('div');
    item.className = 'reparent-item';
    item.textContent = sib.name || '未命名';
    item.addEventListener('click', async () => {
      document.getElementById('sibling-picker-modal').classList.add('hidden');
      await doTransferToSibling(currentPersonId, sib.id, photoIds, false);
    });
    list.appendChild(item);
  }

  document.getElementById('sibling-picker-modal').classList.remove('hidden');
}

async function createNewSiblingAndTransfer(currentPersonId, parentId, photoIds) {
  const name = await showPersonDetailNameDialog('新建兄弟人物', `与当前人物同级，接收 ${photoIds.length} 张照片中的人脸`);
  if (name === null) return;

  const decision = name ? await checkDuplicateName(name, null) : { action: 'different' };

  let targetId;
  let isNew = false;
  if (decision.action === 'same') {
    targetId = decision.targetId;
  } else {
    const resp = await fetch('/api/people', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name || null, parent_id: parentId }),
    });
    if (!resp.ok) return;
    const { id } = await resp.json();
    targetId = id;
    isNew = true;
  }

  await doTransferToSibling(currentPersonId, targetId, photoIds, isNew);
}

async function doTransferToSibling(currentPersonId, targetId, photoIds, isNewPerson) {
  const resp = await fetch(`/api/people/${currentPersonId}/transfer`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ target_person_id: targetId, photo_ids: photoIds }),
  });
  if (resp.ok) {
    const label = isNewPerson ? '新建兄弟并转移' : `转移至兄弟 #${targetId}`;
    pushUndo(label, async () => {
      await fetch(`/api/people/${targetId}/transfer`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target_person_id: currentPersonId, photo_ids: photoIds }),
      });
      if (isNewPerson) {
        await fetch(`/api/people/${targetId}`, { method: 'DELETE' });
      }
    });
  }
  clearPersonDetailSelection();
  await refreshPersonDetail();
}

async function refreshPersonDetail() {
  if (state.currentPersonId) {
    state.personDetailPage = 1;
    await loadPersonDetailPage(state.currentPersonId);
    await loadSubPersons(state.currentPersonId);
  }
}

// ── Person detail: breadcrumb & reparent ──────────────────────────────────────

async function updatePersonBreadcrumb(personId) {
  const el = document.getElementById('person-breadcrumb');
  if (!el) return;
  const tree = await fetchJSON('/api/people/tree');
  el.innerHTML = '';
  if (!tree) { el.textContent = '顶级'; return; }

  const path = []; // [{id, name}]
  function findPath(nodes, targetId) {
    for (const n of nodes) {
      if (n.id === targetId) { path.push({ id: n.id, name: n.name || '未命名' }); return true; }
      if (findPath(n.children || [], targetId)) {
        path.unshift({ id: n.id, name: n.name || '未命名' }); return true;
      }
    }
    return false;
  }
  findPath(tree.people || [], personId);
  path.pop(); // remove self, show ancestors only

  if (path.length === 0) {
    el.textContent = '顶级';
  } else {
    path.forEach((ancestor, i) => {
      const span = document.createElement('span');
      span.textContent = ancestor.name;
      span.style.cssText = 'color:#89b4fa;cursor:pointer';
      span.addEventListener('click', () => showPersonDetail(ancestor.id));
      el.appendChild(span);
      if (i < path.length - 1) el.appendChild(document.createTextNode(' > '));
    });
  }
}

let _reparentTargetPersonId = null;

async function openReparentPanel() {
  _reparentTargetPersonId = state.currentPersonId;
  const panel = document.getElementById('person-reparent-panel');
  panel.classList.toggle('hidden');
  if (!panel.classList.contains('hidden')) {
    document.getElementById('person-reparent-search').value = '';
    await buildReparentList(_reparentTargetPersonId, state.currentPersonId);
    document.getElementById('person-reparent-search').focus();
  }
}

async function filterReparentList() {
  await buildReparentList(_reparentTargetPersonId, state.currentPersonId);
}

async function buildReparentList(targetId, excludeId) {
  const q = document.getElementById('person-reparent-search').value.toLowerCase();
  const container = document.getElementById('person-reparent-list');
  container.innerHTML = '';

  // "Set as top-level" option
  const topItem = document.createElement('div');
  topItem.className = 'reparent-item top-level';
  topItem.textContent = '设为顶级（无父节点）';
  topItem.addEventListener('click', () => doReparent(targetId, null, excludeId));
  container.appendChild(topItem);

  const people = await fetchJSON('/api/people?status=all') || [];
  for (const p of people) {
    if (p.id === excludeId) continue;  // can't be own parent
    const name = (p.name || '未命名').toLowerCase();
    if (q && !name.includes(q)) continue;
    const item = document.createElement('div');
    item.className = 'reparent-item';
    item.textContent = p.name || '未命名';
    item.addEventListener('click', () => doReparent(targetId, p.id, excludeId));
    container.appendChild(item);
  }
}

async function doReparent(targetPersonId, newParentId, currentParentId) {
  document.getElementById('person-reparent-panel').classList.add('hidden');
  const resp = await fetch(`/api/people/${targetPersonId}/reparent`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ new_parent_id: newParentId }),
  });
  if (resp.ok) {
    pushUndo('更改父节点', async () => {
      await fetch(`/api/people/${targetPersonId}/reparent`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ new_parent_id: currentParentId }),
      });
    });
    await updatePersonBreadcrumb(targetPersonId);
    if (targetPersonId !== state.currentPersonId) {
      await loadSubPersons(state.currentPersonId);
    }
  }
}

async function openReparentPickerForChild(childId, childName, currentParentId) {
  // Temporarily redirect the reparent panel to act on this child
  _reparentTargetPersonId = childId;
  const panel = document.getElementById('person-reparent-panel');
  panel.classList.remove('hidden');
  document.getElementById('person-reparent-search').value = '';
  // exclude both the child and its current parent from options
  await buildReparentList(childId, childId);
  document.getElementById('person-reparent-search').focus();
}

async function savePersonName() {
  const input = document.getElementById('person-name-input');
  const personId = +input.dataset.personId;
  if (!personId) return;
  const newName = input.value.trim();
  const oldPerson = state.allPeople.find(x => x.id === personId);
  const oldName = oldPerson ? (oldPerson.name || '') : '';
  if (newName === oldName) return;
  const ok = await patchPerson(personId, { name: newName });
  if (ok) {
    if (oldPerson) oldPerson.name = newName;
    pushUndo(`改名"${newName || '（空）'}"`, async () => {
      await patchPerson(personId, { name: oldName });
      document.getElementById('person-name-input').value = oldName;
    });
  }
}

// ── Generic merge confirmation ────────────────────────────────────────────────

let _mergeConfirmCallback = null;

function showInlineConfirm(anchorX, anchorY, message, onConfirm) {
  document.querySelectorAll('.inline-confirm').forEach(el => el.remove());
  const el = document.createElement('div');
  el.className = 'inline-confirm';
  el.innerHTML = `<p>${message}</p>
    <div class="inline-confirm-btns">
      <button class="btn-danger confirm-ok">确定</button>
      <button class="btn-ghost confirm-cancel">取消</button>
    </div>`;
  document.body.appendChild(el);

  // Clamp position so the popover stays within the viewport
  const W = window.innerWidth, H = window.innerHeight;
  const elW = el.offsetWidth  || 200;
  const elH = el.offsetHeight || 80;
  let left = anchorX + 10;
  let top  = anchorY + 10;
  if (left + elW > W - 8) left = anchorX - elW - 10;
  if (top  + elH > H - 8) top  = anchorY - elH - 10;
  el.style.left = `${Math.max(8, left)}px`;
  el.style.top  = `${Math.max(8, top)}px`;

  const dismiss = () => el.remove();
  el.querySelector('.confirm-ok').addEventListener('click', () => { dismiss(); onConfirm(); });
  el.querySelector('.confirm-cancel').addEventListener('click', dismiss);
  // Dismiss on any outside click (deferred so the originating click doesn't immediately close it)
  setTimeout(() => document.addEventListener('click', dismiss, { once: true, capture: true }), 0);
}

function showMergeConfirm(message, onConfirm) {
  document.getElementById('merge-confirm-message').textContent = message;
  _mergeConfirmCallback = onConfirm;
  document.getElementById('merge-confirm-modal').classList.remove('hidden');
}

// ── Duplicate name detection ─────────────────────────────────────────────────

let _dupNameResolve = null;
let _dupNameContext = null;  // { newPersonId, matchedPeople }

async function checkDuplicateName(name, ownPersonId) {
  if (!name.trim()) return 'keep';
  const matches = await fetchJSON(`/api/people?name_exact=${encodeURIComponent(name.trim())}`);
  if (!matches || matches.length === 0) return 'keep';
  const others = matches.filter(p => p.id !== ownPersonId);
  if (others.length === 0) return 'keep';

  _dupNameContext = { ownPersonId, matchedPeople: others };
  return new Promise(resolve => {
    _dupNameResolve = resolve;
    showDupNameDialog(name, ownPersonId, others);
  });
}

function showDupNameDialog(name, ownPersonId, matches) {
  document.getElementById('dup-name-desc').textContent =
    `姓名"${name}"已有 ${matches.length} 位同名人物，是否为同一人？`;

  const pairs = document.getElementById('dup-name-pairs');
  pairs.innerHTML = '';

  // Show own person thumb on the left
  const ownPerson = state.allPeople.find(p => p.id === ownPersonId);
  if (ownPerson) {
    const d = document.createElement('div');
    d.className = 'dup-name-face';
    const src = ownPerson.cover_face_id ? `/api/faces/${ownPerson.cover_face_id}/thumb` : '';
    d.innerHTML = `<img src="${src}" alt=""><span>当前（待命名）</span>`;
    pairs.appendChild(d);
  }

  for (const m of matches) {
    const d = document.createElement('div');
    d.className = 'dup-name-face';
    const src = m.cover_face_id ? `/api/faces/${m.cover_face_id}/thumb` : '';
    d.innerHTML = `<img src="${src}" alt=""><span>${escHtml(m.name || '未命名')} (#${m.id})</span>`;
    pairs.appendChild(d);
  }

  document.getElementById('dup-name-modal').classList.remove('hidden');
}

function confirmDupNameSame() {
  document.getElementById('dup-name-modal').classList.add('hidden');
  if (!_dupNameContext) return;
  const { matchedPeople } = _dupNameContext;
  const targetId = matchedPeople[0].id;
  _dupNameContext = null;
  if (_dupNameResolve) { _dupNameResolve({ action: 'same', targetId }); _dupNameResolve = null; }
}

// ── People undo stack ─────────────────────────────────────────────────────────

const _peopleUndoStack = [];  // [{label, undo: async fn}]

function pushUndo(label, undoFn) {
  _peopleUndoStack.push({ label, undo: undoFn });
  const btn = document.getElementById('people-undo-btn');
  if (btn) {
    btn.disabled = false;
    btn.title = `撤销：${label}`;
  }
}

async function undoPeopleOp() {
  const op = _peopleUndoStack.pop();
  if (!op) return;
  await op.undo();
  loadPeopleList();
  const btn = document.getElementById('people-undo-btn');
  if (btn) {
    btn.disabled = _peopleUndoStack.length === 0;
    btn.title = _peopleUndoStack.length > 0
      ? `撤销：${_peopleUndoStack[_peopleUndoStack.length - 1].label}`
      : '撤销上一步操作';
  }
}

// ── People inline edit & context menu ────────────────────────────────────────

function escHtml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

async function patchPerson(personId, fields) {
  const resp = await fetch(`/api/people/${personId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(fields),
  });
  return resp.ok;
}

function startInlineNameEdit(cell, personId) {
  if (cell.querySelector('input')) return; // already editing
  const currentName = cell.dataset.name;
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'person-name-inline-input';
  input.value = currentName;
  cell.innerHTML = '';
  cell.appendChild(input);
  input.focus();
  input.select();

  let saved = false;
  const commit = async () => {
    if (saved) return;
    saved = true;
    const newName = input.value.trim();
    if (newName !== currentName) {
      const decision = await checkDuplicateName(newName, personId);
      if (decision.action === 'same') {
        // Merge current person into the matched existing person
        await fetch('/api/people/merge', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ source_id: personId, target_id: decision.targetId }),
        });
        loadPeopleList();
        return;
      }
      const ok = await patchPerson(personId, { name: newName });
      if (ok) {
        cell.dataset.name = newName;
        cell.innerHTML = escHtml(newName || '未命名');
        const p = state.allPeople.find(x => x.id === personId);
        if (p) p.name = newName;
        pushUndo(`改名"${newName || '（空）'}"`, async () => {
          await patchPerson(personId, { name: currentName });
        });
        return;
      }
    }
    cell.innerHTML = escHtml(currentName || '未命名');
  };

  input.addEventListener('blur', commit);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { input.blur(); }
    if (e.key === 'Escape') {
      saved = true;
      cell.innerHTML = escHtml(currentName || '未命名');
    }
  });
}

let _activePersonMenu = null;

function closePersonMenu() {
  if (_activePersonMenu) {
    _activePersonMenu.remove();
    _activePersonMenu = null;
  }
}

function showPersonMenu(btn, personId, card) {
  closePersonMenu();
  const menu = document.createElement('div');
  menu.className = 'person-context-menu';
  menu.innerHTML = `
    <button data-action="ignore">忽略此人</button>
    <button data-action="not-person">标记为非人物</button>`;
  document.body.appendChild(menu);
  _activePersonMenu = menu;

  const rect = btn.getBoundingClientRect();
  menu.style.top = `${rect.bottom + 4}px`;
  menu.style.left = `${rect.left}px`;

  menu.querySelector('[data-action="ignore"]').addEventListener('click', (e) => {
    e.stopPropagation();
    closePersonMenu();
    showInlineConfirm(e.clientX, e.clientY, '确定要忽略此人？此操作可撤销。', async () => {
      const ok = await patchPerson(personId, { status: 'ignored' });
      if (ok) {
        removePersonCard(personId, card);
        pushUndo('忽略此人', async () => { await patchPerson(personId, { status: 'active' }); });
      }
    });
  });
  menu.querySelector('[data-action="not-person"]').addEventListener('click', (e) => {
    e.stopPropagation();
    closePersonMenu();
    showInlineConfirm(e.clientX, e.clientY, '确定要标记为非人物？此操作可撤销。', async () => {
      const ok = await patchPerson(personId, { status: 'not_a_person' });
      if (ok) {
        removePersonCard(personId, card);
        pushUndo('标记为非人物', async () => { await patchPerson(personId, { status: 'active' }); });
      }
    });
  });
}

function removePersonCard(personId, card) {
  state.allPeople = state.allPeople.filter(p => p.id !== personId);
  document.getElementById('people-count').textContent = `共 ${state.allPeople.length} 人`;
  card.style.transition = 'opacity 0.25s';
  card.style.opacity = '0';
  setTimeout(() => card.remove(), 250);
}

async function fillMissingFaces() {
  const btn = document.getElementById('fill-faces-btn');
  const statusEl = document.getElementById('fill-faces-status');
  btn.disabled = true;
  statusEl.textContent = '启动中…';

  const faceRes = await fetchJSON('/api/faces/analyze', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ missing_only: true }),
  });

  if (!faceRes) {
    statusEl.textContent = '启动失败';
    btn.disabled = false;
    return;
  }

  const faceJobId = faceRes.job_id;
  const pollId = setInterval(async () => {
    const faceJob = faceJobId ? await fetchJSON(`/api/faces/jobs/${faceJobId}`) : null;
    const text = faceJob
      ? (faceJob.status === 'running'
          ? `${faceJob.processed}/${faceJob.total ?? '?'}`
          : faceJob.status === 'done' ? '完成' : '失败')
      : '完成';
    statusEl.textContent = text;
    if (!faceJob || faceJob.status !== 'running') {
      clearInterval(pollId);
      btn.disabled = false;
      if (state.currentView === 'people') loadPeopleList();
    }
  }, 2000);
}

async function fillMissingGeo() {
  const btn = document.getElementById('fill-geo-btn');
  const statusEl = document.getElementById('fill-geo-status');
  btn.disabled = true;
  statusEl.textContent = '启动中…';

  const geoRes = await fetchJSON('/api/geo/regeocode', { method: 'POST' });

  if (!geoRes) {
    statusEl.textContent = '启动失败';
    btn.disabled = false;
    return;
  }

  const geoCount = geoRes.count ?? 0;
  const geoStatus = geoRes.status;

  if (geoStatus === 'already_running') {
    statusEl.textContent = '已在运行中';
    btn.disabled = false;
    return;
  }

  statusEl.textContent = `处理中（${geoCount} 张待处理）`;

  const pollId = setInterval(async () => {
    const geoSt = await fetchJSON('/api/geo/regeocode/status');
    const running = geoSt && geoSt.running;
    if (!running) {
      clearInterval(pollId);
      btn.disabled = false;
      statusEl.textContent = `完成（${geoCount} 张）`;
      if (state.currentView === 'locations') loadGeoHierarchy();
    }
  }, 2000);
}

async function triggerIntegrate() {
  const btn = document.getElementById('integrate-faces-btn');
  btn.disabled = true;
  document.getElementById('recluster-status').textContent = '整合中…';
  const result = await fetchJSON('/api/people/cluster/incremental', { method: 'POST' });
  btn.disabled = false;
  if (result) {
    document.getElementById('recluster-status').textContent =
      `整合完成，新增 ${result.people_created} 个人物`;
    loadPeopleList();
  } else {
    document.getElementById('recluster-status').textContent = '整合失败';
  }
}

async function triggerRecluster() {
  const btn = document.getElementById('recluster-btn');
  btn.disabled = true;
  document.getElementById('recluster-status').textContent = '全量重建中…';
  const result = await fetchJSON('/api/people/cluster', { method: 'POST' });
  btn.disabled = false;
  if (result) {
    document.getElementById('recluster-status').textContent =
      `重建完成，生成 ${result.people_created} 个人物`;
    loadPeopleList();
  } else {
    document.getElementById('recluster-status').textContent = '重建失败';
  }
}

async function openMergeDialog() {
  const [people, suggestions] = await Promise.all([
    fetchJSON('/api/people'),
    fetchJSON(`/api/people/${state.currentPersonId}/merge-suggestions?limit=20`),
  ]);
  if (!people) return;
  state.allPeople = people;
  state.mergeSimilarityMap = new Map((suggestions || []).map(s => [s.person_id, s.distance]));
  state.mergeTargetId = null;
  document.getElementById('merge-confirm-btn').disabled = true;
  document.getElementById('merge-search').value = '';
  renderMergeList(people.filter(p => p.id !== state.currentPersonId && p.name));
  document.getElementById('merge-modal').classList.remove('hidden');
}

function renderMergeList(people) {
  const simMap = state.mergeSimilarityMap || new Map();
  const ul = document.getElementById('merge-target-list');
  ul.innerHTML = '';

  // Sort: entries with similarity score first (higher similarity = lower distance),
  // then the rest by photo_count desc
  const sorted = [...people].sort((a, b) => {
    const da = simMap.has(a.id) ? simMap.get(a.id) : Infinity;
    const db = simMap.has(b.id) ? simMap.get(b.id) : Infinity;
    if (da !== db) return da - db;
    return (b.photo_count || 0) - (a.photo_count || 0);
  });

  for (const p of sorted) {
    const li = document.createElement('li');
    li.style.cssText = 'padding:6px 10px;cursor:pointer;display:flex;justify-content:space-between;align-items:center;gap:8px';
    const label = document.createElement('span');
    label.textContent = (p.name || '未命名') + ` (${p.photo_count} 张)`;
    li.appendChild(label);
    if (simMap.has(p.id)) {
      const pct = document.createElement('span');
      pct.style.cssText = 'font-size:11px;color:#888;flex-shrink:0';
      pct.textContent = `${Math.round((1 - simMap.get(p.id)) * 100)}% 相似`;
      li.appendChild(pct);
    }
    li.addEventListener('click', () => {
      ul.querySelectorAll('li').forEach(x => x.style.background = '');
      li.style.background = '#e8e0ff';
      state.mergeTargetId = p.id;
      document.getElementById('merge-confirm-btn').disabled = false;
    });
    ul.appendChild(li);
  }
}

function filterMergeList() {
  const q = document.getElementById('merge-search').value.toLowerCase();
  const filtered = state.allPeople.filter(p =>
    p.id !== state.currentPersonId && p.name &&
    p.name.toLowerCase().includes(q)
  );
  renderMergeList(filtered);
}

function confirmMerge() {
  if (!state.mergeTargetId) return;
  const source = state.allPeople.find(p => p.id === state.currentPersonId);
  const target = state.allPeople.find(p => p.id === state.mergeTargetId);
  document.getElementById('merge-modal').classList.add('hidden');
  const doMerge = async () => {
    await fetch('/api/people/merge', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source_id: state.currentPersonId, target_id: state.mergeTargetId }),
    });
    loadPeopleList();
    showPersonDetail(state.mergeTargetId);
  };
  if (source?.name) {
    const targetName = target ? (target.name || '未命名') : '未知';
    showMergeConfirm(`将「${source.name}」并入「${targetName}」？`, doMerge);
  } else {
    doMerge();
  }
}

// ── Activities view ───────────────────────────────────────────────────────────
const activitiesState = {
  currentType: '',
  page: 1,
  perPage: 50,
  total: 0,
  activityMap: null,
  activityPolyline: null,
  photoMarkers: [],
  multiSelect: false,
  selected: new Map(), // id → ActivityItem
};

const ACTIVITY_ICONS = {
  running: '🏃', hiking: '🥾', cycling: '🚴', walking: '🚶',
  trail_running: '⛰', swimming: '🏊', other: '🏅',
};

function initActivitiesView() {
  const panel = document.getElementById('activities-list-panel');
  const detail = document.getElementById('activities-detail-panel');
  panel.style.display = '';
  detail.classList.add('hidden');

  document.querySelectorAll('.act-type-btn').forEach(btn => {
    btn.onclick = () => {
      document.querySelectorAll('.act-type-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      activitiesState.currentType = btn.dataset.type;
      activitiesState.page = 1;
      loadActivitiesList();
    };
  });

  document.getElementById('act-prev-btn').onclick = () => {
    if (activitiesState.page > 1) { activitiesState.page--; loadActivitiesList(); }
  };
  document.getElementById('act-next-btn').onclick = () => {
    const maxPage = Math.ceil(activitiesState.total / activitiesState.perPage);
    if (activitiesState.page < maxPage) { activitiesState.page++; loadActivitiesList(); }
  };

  document.getElementById('act-back-btn').onclick = () => {
    detail.classList.add('hidden');
    panel.style.display = '';
    if (activitiesState.activityMap) {
      activitiesState.activityMap.remove();
      activitiesState.activityMap = null;
    }
  };

  document.getElementById('act-multiselect-btn').onclick = () => {
    activitiesState.multiSelect = true;
    activitiesState.selected.clear();
    document.getElementById('act-multiselect-btn').classList.add('hidden');
    document.getElementById('act-multiselect-bar').classList.remove('hidden');
    loadActivitiesList();
  };

  document.getElementById('act-sel-cancel-btn').onclick = exitMultiSelect;

  document.getElementById('act-merge-btn').onclick = openMergeModal;

  loadActivitiesList();
}

function exitMultiSelect() {
  activitiesState.multiSelect = false;
  activitiesState.selected.clear();
  document.getElementById('act-multiselect-btn').classList.remove('hidden');
  document.getElementById('act-multiselect-bar').classList.add('hidden');
  loadActivitiesList();
}

function updateMultiSelectBar() {
  const sel = activitiesState.selected;
  const count = sel.size;
  document.getElementById('act-sel-count').textContent = `已选 ${count} 个运动`;

  const btn = document.getElementById('act-merge-btn');
  if (count < 2) {
    btn.disabled = true;
    btn.title = '请至少选择 2 个运动';
    return;
  }
  const types = new Set([...sel.values()].map(a => a.activity_type));
  if (types.size > 1) {
    btn.disabled = true;
    btn.title = '当前仅支持合并同类运动';
  } else {
    btn.disabled = false;
    btn.title = '';
  }
}

async function loadActivitiesList() {
  const { currentType, page, perPage } = activitiesState;
  let url = `/api/activities?page=${page}&per_page=${perPage}`;
  if (currentType) url += `&type=${encodeURIComponent(currentType)}`;

  const data = await fetchJSON(url);
  if (!data) return;

  activitiesState.total = data.total;
  const container = document.getElementById('activities-list');
  container.innerHTML = '';

  const { multiSelect, selected } = activitiesState;
  for (const act of data.activities) {
    const row = document.createElement('div');
    const isSelected = selected.has(act.id);
    row.className = 'activity-row' + (multiSelect ? ' selectable' : '') + (isSelected ? ' selected' : '');
    row.innerHTML = `
      ${multiSelect ? `<input type="checkbox" class="act-row-checkbox" ${isSelected ? 'checked' : ''}>` : ''}
      <span class="act-icon">${ACTIVITY_ICONS[act.activity_type] || '🏅'}</span>
      <div class="act-info">
        <div class="act-title">${act.title || activityDefaultTitle(act)}</div>
        <div class="act-date">${formatActivityDate(act.start_time)}</div>
        <div class="act-stats">
          ${act.distance_meters ? `<span class="act-stat"><span class="act-stat-label">距离</span>${(act.distance_meters / 1000).toFixed(2)} km</span>` : ''}
          ${act.duration_seconds ? `<span class="act-stat"><span class="act-stat-label">时长</span>${formatDuration(act.duration_seconds)}</span>` : ''}
          ${act.elevation_gain_meters ? `<span class="act-stat"><span class="act-stat-label">爬升</span>${Math.round(act.elevation_gain_meters)} m</span>` : ''}
          ${act.avg_heart_rate ? `<span class="act-stat"><span class="act-stat-label">心率</span>${act.avg_heart_rate} bpm</span>` : ''}
        </div>
      </div>
    `;
    if (multiSelect) {
      row.onclick = (e) => {
        e.preventDefault();
        if (selected.has(act.id)) {
          selected.delete(act.id);
        } else {
          selected.set(act.id, act);
        }
        updateMultiSelectBar();
        loadActivitiesList();
      };
    } else {
      row.onclick = () => showActivityDetail(act.id);
    }
    container.appendChild(row);
  }

  if (data.activities.length === 0) {
    container.innerHTML = '<div style="color:#888;padding:24px;text-align:center">暂无运动记录。使用 <code>picmanager activities import &lt;目录&gt;</code> 导入 FIT/GPX 文件。</div>';
  }

  const pag = document.querySelector('.activities-pagination');
  const maxPage = Math.ceil(data.total / perPage);
  if (maxPage > 1) {
    pag.classList.remove('hidden');
    document.getElementById('act-page-info').textContent = `${page} / ${maxPage}`;
  } else {
    pag.classList.add('hidden');
  }
}

async function showActivityDetail(id) {
  const panel = document.getElementById('activities-list-panel');
  const detail = document.getElementById('activities-detail-panel');
  panel.style.display = 'none';
  detail.classList.remove('hidden');

  const [actData, trackData, photosData] = await Promise.all([
    fetchJSON(`/api/activities/${id}`),
    fetchJSON(`/api/activities/${id}/track`),
    fetchJSON(`/api/activities/${id}/photos`),
  ]);

  if (!actData) { detail.classList.add('hidden'); panel.style.display = ''; return; }

  document.getElementById('act-detail-title').textContent = actData.title || activityDefaultTitle(actData);
  renderActivityMeta(actData);
  renderActivityMap(trackData);
  renderActivityPhotos(photosData, trackData);
}

const SENSOR_ICONS = {
  heart_rate: '🫀',
  bike_power: '⚡',
  bike_cadence: '🔄',
  bike_speed: '📡',
  bike_speed_cadence: '📡',
  footpod: '👟',
  stride_speed_distance: '👟',
  muscle_oxygen: '💧',
  running_dynamics: '📊',
};
const SENSOR_NAMES = {
  heart_rate: '心率',
  bike_power: '功率计',
  bike_cadence: '踏频',
  bike_speed: '速度',
  bike_speed_cadence: '速踏频',
  footpod: '足舱',
  stride_speed_distance: '步频',
  muscle_oxygen: '肌氧',
  running_dynamics: '跑步动态',
};
const BATTERY_STATUS_ZH = { good: '充足', ok: '正常', low: '偏低', critical: '严重不足', unknown: '未知' };

function formatSensorBattery(sensor) {
  if (sensor.battery_level != null) return `${sensor.battery_level}%`;
  if (sensor.battery_status) return BATTERY_STATUS_ZH[sensor.battery_status] || sensor.battery_status;
  return null;
}

function formatSensorName(sensor) {
  // product name or fall back to a capitalised manufacturer label
  if (sensor.name) return sensor.name;
  if (sensor.manufacturer) {
    return sensor.manufacturer.charAt(0).toUpperCase() + sensor.manufacturer.slice(1);
  }
  return null;
}

function renderSensors(sensors) {
  if (!sensors || sensors.length === 0) return '';
  const items = sensors.map(s => {
    const icon = SENSOR_ICONS[s.sensor_type] || '📡';
    const label = SENSOR_NAMES[s.sensor_type] || s.sensor_type;
    const name = formatSensorName(s);
    const battery = formatSensorBattery(s);
    let detail = [name, battery ? `${battery}电量` : null].filter(Boolean).join(' · ');
    return `<div class="act-sensor-item">${icon} <span class="act-sensor-label">${label}</span>${detail ? `<span class="act-sensor-detail">${detail}</span>` : ''}</div>`;
  }).join('');
  return `<div class="act-meta-row"><span class="act-meta-label">传感器</span><span class="act-meta-value"><div class="act-sensor-list">${items}</div></span></div>`;
}

function renderActivityMeta(act) {
  const meta = document.getElementById('act-meta');
  const rows = [
    ['类型', activityTypeName(act.activity_type)],
    ['开始时间', formatActivityDate(act.start_time)],
    ['时长', act.duration_seconds ? formatDuration(act.duration_seconds) : null],
    ['距离', act.distance_meters ? `${(act.distance_meters / 1000).toFixed(2)} km` : null],
    ['配速/速度', formatPace(act)],
    ['累计爬升', act.elevation_gain_meters ? `${Math.round(act.elevation_gain_meters)} m` : null],
    ['平均/最大心率', (act.avg_heart_rate && act.max_heart_rate) ? `${act.avg_heart_rate} / ${act.max_heart_rate} bpm` : (act.avg_heart_rate ? `${act.avg_heart_rate} bpm` : null)],
    ['卡路里', act.calories ? `${act.calories} kcal` : null],
    ['设备', act.device || null],
    ['格式', act.file_format.toUpperCase()],
  ].filter(([, v]) => v !== null);

  meta.innerHTML = rows.map(([label, value]) =>
    `<div class="act-meta-row"><span class="act-meta-label">${label}</span><span class="act-meta-value">${value}</span></div>`
  ).join('') +
  renderSensors(act.sensors) +
  `<div style="margin-top:10px">
     <a href="#" style="font-size:12px;color:#3b82f6" onclick="openTrimModal(${act.id});return false">✂ 剪辑运动</a>
   </div>`;
}

function renderActivityMap(trackData) {
  const mapEl = document.getElementById('act-map');
  const noTrack = document.getElementById('act-no-track');

  if (activitiesState.activityMap) {
    activitiesState.activityMap.remove();
    activitiesState.activityMap = null;
    activitiesState.activityPolyline = null;
    activitiesState.photoMarkers = [];
  }

  if (!trackData || !trackData.points || trackData.points.length === 0) {
    mapEl.style.display = 'none';
    noTrack.classList.remove('hidden');
    return;
  }

  mapEl.style.display = '';
  noTrack.classList.add('hidden');

  const map = L.map('act-map');
  activitiesState.activityMap = map;

  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '© OpenStreetMap contributors',
    maxZoom: 19,
  }).addTo(map);

  const latlngs = trackData.points.map(p => [p.lat, p.lon]);
  const polyline = L.polyline(latlngs, { color: '#4a9eff', weight: 3 }).addTo(map);
  activitiesState.activityPolyline = polyline;
  map.fitBounds(polyline.getBounds(), { padding: [20, 20] });
}

function renderActivityPhotos(photosData, trackData) {
  const grid = document.getElementById('act-photos-grid');
  const noPhotos = document.getElementById('act-no-photos');
  const title = document.getElementById('act-photos-title');
  grid.innerHTML = '';

  const photos = photosData ? photosData.photos : [];
  title.textContent = `本次运动中的照片（${photos.length} 张）`;

  if (photos.length === 0) {
    noPhotos.classList.remove('hidden');
    return;
  }
  noPhotos.classList.add('hidden');

  for (let i = 0; i < photos.length; i++) {
    const photo = photos[i];
    const card = document.createElement('div');
    card.className = 'photo-card';
    const img = document.createElement('img');
    img.src = `/api/photos/${photo.id}/thumb`;
    img.alt = '';
    img.loading = 'lazy';
    card.appendChild(img);
    card.onclick = () => openDetail(i, photos);
    grid.appendChild(card);

    // Add map marker for photos with GPS
    if (photo.gps_lat && photo.gps_lon && activitiesState.activityMap) {
      const icon = L.divIcon({ html: '📷', className: 'camera-marker', iconSize: [20, 20] });
      const marker = L.marker([photo.gps_lat, photo.gps_lon], { icon })
        .addTo(activitiesState.activityMap)
        .bindPopup(`<img src="/api/photos/${photo.id}/thumb" style="width:120px;border-radius:4px"><br><small>${photo.taken_at || ''}</small>`)
        .on('popupopen', () => {
          const popupEl = marker.getPopup().getElement();
          if (popupEl) {
            popupEl.querySelector('img').style.cursor = 'pointer';
            popupEl.querySelector('img').onclick = () => openDetail(i, photos);
          }
        });
      activitiesState.photoMarkers.push(marker);
    }
  }
}

function activityDefaultTitle(act) {
  const typeName = activityTypeName(act.activity_type);
  const date = act.start_time ? act.start_time.slice(0, 10) : '';
  return date ? `${date} ${typeName}` : typeName;
}

function activityTypeName(type) {
  const names = { running: '跑步', hiking: '徒步', cycling: '骑行', walking: '步行', trail_running: '越野跑', swimming: '游泳', other: '运动' };
  return names[type] || '运动';
}

function formatActivityDate(isoStr) {
  if (!isoStr) return '';
  try {
    const d = new Date(isoStr);
    return d.toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
  } catch { return isoStr.slice(0, 16); }
}

function formatDuration(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function formatPace(act) {
  if (!act.distance_meters || !act.duration_seconds || act.distance_meters === 0) return null;
  const type = act.activity_type;
  if (type === 'running' || type === 'hiking' || type === 'walking' || type === 'trail_running') {
    const secPerKm = act.duration_seconds / (act.distance_meters / 1000);
    const m = Math.floor(secPerKm / 60);
    const s = Math.round(secPerKm % 60);
    return `${m}'${String(s).padStart(2, '0')}" /km`;
  }
  const kmh = (act.distance_meters / act.duration_seconds * 3.6).toFixed(1);
  return `${kmh} km/h`;
}

// ── Activity Trim Modal ───────────────────────────────────────────────────────

const trimState = {
  activityId: null,
  axis: 'time',       // 'time' | 'distance'
  points: [],
  cumTime: [],        // seconds from start, one per point
  cumDist: [],        // metres from start, one per point
  paceData: [],       // min/km per point (null if not computable)
  elevData: [],       // elevation per point (null if absent)
  startIdx: 0,
  endIdx: 0,
  trimMap: null,
  polyFull: null,
  polySelected: null,
  dragging: null,
  rafId: null,
};

async function openTrimModal(activityId) {
  trimState.activityId = activityId;
  trimState.axis = 'time';
  trimState.dragging = null;

  const modal = document.getElementById('trim-modal');
  modal.classList.remove('hidden');

  const [actData, trackData] = await Promise.all([
    fetchJSON(`/api/activities/${activityId}`),
    fetchJSON(`/api/activities/${activityId}/track`),
  ]);

  document.getElementById('trim-modal-title').textContent =
    actData?.title || `活动 #${activityId}`;

  const pts = trackData?.points || [];
  trimState.points = pts;
  trimState.startIdx = 0;
  trimState.endIdx = pts.length > 0 ? pts.length - 1 : 0;

  // Cumulative time & distance
  const cumTime = [0], cumDist = [0];
  for (let i = 1; i < pts.length; i++) {
    const dt = (new Date(pts[i].ts) - new Date(pts[i - 1].ts)) / 1000;
    const dd = trimHaversine(pts[i - 1].lat, pts[i - 1].lon, pts[i].lat, pts[i].lon);
    cumTime.push(cumTime[i - 1] + Math.max(0, dt));
    cumDist.push(cumDist[i - 1] + dd);
  }
  trimState.cumTime = cumTime;
  trimState.cumDist = cumDist;

  // Pace & elevation per point
  trimState.paceData = pts.map((_, i) => {
    if (i === 0) return null;
    const dDist = cumDist[i] - cumDist[i - 1];
    const dTime = cumTime[i] - cumTime[i - 1];
    if (dDist < 1 || dTime <= 0) return null;
    return dTime / 60 / (dDist / 1000); // min/km
  });
  trimState.elevData = pts.map(p => p.elevation ?? null);

  // Axis tab wiring (idempotent)
  document.querySelectorAll('.trim-tab').forEach(btn => {
    btn.onclick = () => {
      document.querySelectorAll('.trim-tab').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      trimState.axis = btn.dataset.axis;
      renderTrimChart();
      updateTrimHandlePositions();
      updateTrimStats();
      renderTrimAxisLabels();
    };
  });
  document.querySelector('.trim-tab[data-axis="time"]').classList.add('active');
  document.querySelector('.trim-tab[data-axis="distance"]').classList.remove('active');

  initTrimMap(pts);
  initTrimHandles();
  renderTrimChart();
  updateTrimHandlePositions();
  updateTrimStats();
  renderTrimAxisLabels();
}

function closeTrimModal() {
  document.getElementById('trim-modal').classList.add('hidden');
  if (trimState.trimMap) { trimState.trimMap.remove(); trimState.trimMap = null; }
  trimState.points = [];
}

function initTrimMap(pts) {
  if (trimState.trimMap) { trimState.trimMap.remove(); trimState.trimMap = null; }
  if (pts.length === 0) return;

  const map = L.map('trim-map');
  trimState.trimMap = map;
  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '© OpenStreetMap', maxZoom: 19,
  }).addTo(map);

  const all = pts.map(p => [p.lat, p.lon]);
  trimState.polyFull = L.polyline(all, { color: '#ccc', weight: 3 }).addTo(map);
  trimState.polySelected = L.polyline(all, { color: '#4a9eff', weight: 3 }).addTo(map);
  map.fitBounds(trimState.polyFull.getBounds(), { padding: [20, 20] });
}

function updateTrimMap() {
  if (!trimState.trimMap || trimState.points.length === 0) return;
  const { points, startIdx, endIdx } = trimState;
  const sel = points.slice(startIdx, endIdx + 1).map(p => [p.lat, p.lon]);
  trimState.polySelected?.setLatLngs(sel);
}

function renderTrimChart() {
  const canvas = document.getElementById('trim-chart');
  const wrapper = document.getElementById('trim-chart-wrapper');
  if (!canvas || trimState.points.length === 0) return;

  const W = wrapper.clientWidth, H = wrapper.clientHeight;
  canvas.width = W; canvas.height = H;
  const ctx = canvas.getContext('2d');
  ctx.clearRect(0, 0, W, H);

  // Use elevation if available, else pace
  const hasElevation = trimState.elevData.some(v => v !== null);
  const rawVals = hasElevation ? trimState.elevData : trimState.paceData;
  document.getElementById('trim-chart-label').textContent = hasElevation ? '海拔 (m)' : '配速 (min/km)';

  const axisVals = trimState.axis === 'time' ? trimState.cumTime : trimState.cumDist;
  const total = axisVals[axisVals.length - 1] || 1;
  const N = Math.min(200, trimState.points.length);
  const buckets = new Array(N).fill(null).map(() => ({ sum: 0, cnt: 0 }));

  for (let i = 0; i < rawVals.length; i++) {
    if (rawVals[i] == null) continue;
    const b = Math.min(N - 1, Math.floor(axisVals[i] / total * N));
    buckets[b].sum += rawVals[i];
    buckets[b].cnt++;
  }

  const vals = buckets.map(b => b.cnt > 0 ? b.sum / b.cnt : null);
  const maxVal = Math.max(...vals.filter(v => v !== null), 1);
  const minVal = hasElevation ? Math.min(...vals.filter(v => v !== null), 0) : 0;
  const range = maxVal - minVal || 1;

  // Draw bars
  const bw = W / N;
  for (let i = 0; i < N; i++) {
    if (vals[i] == null) continue;
    const h = ((vals[i] - minVal) / range) * (H - 4);
    ctx.fillStyle = '#93c5fd';
    ctx.fillRect(i * bw, H - h, bw - 0.5, h);
  }
}

function initTrimHandles() {
  const wrapper = document.getElementById('trim-chart-wrapper');
  const lh = document.getElementById('trim-handle-left');
  const rh = document.getElementById('trim-handle-right');

  const startDrag = (side) => (e) => {
    trimState.dragging = side;
    e.preventDefault();
  };
  lh.addEventListener('mousedown', startDrag('left'));
  rh.addEventListener('mousedown', startDrag('right'));
  lh.addEventListener('touchstart', startDrag('left'), { passive: false });
  rh.addEventListener('touchstart', startDrag('right'), { passive: false });

  const onMove = (e) => {
    if (!trimState.dragging || trimState.points.length === 0) return;
    e.preventDefault();
    const clientX = e.touches ? e.touches[0].clientX : e.clientX;
    const rect = wrapper.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));

    const axisVals = trimState.axis === 'time' ? trimState.cumTime : trimState.cumDist;
    const total = axisVals[axisVals.length - 1] || 1;
    const target = frac * total;
    let idx = axisVals.findIndex(v => v >= target);
    if (idx < 0) idx = trimState.points.length - 1;

    if (trimState.dragging === 'left') {
      trimState.startIdx = Math.min(idx, trimState.endIdx - 1);
    } else {
      trimState.endIdx = Math.max(idx, trimState.startIdx + 1);
    }

    if (trimState.rafId) cancelAnimationFrame(trimState.rafId);
    trimState.rafId = requestAnimationFrame(() => {
      updateTrimHandlePositions();
      updateTrimStats();
      updateTrimMap();
    });
  };

  const onEnd = () => { trimState.dragging = null; };
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onEnd);
  document.addEventListener('touchmove', onMove, { passive: false });
  document.addEventListener('touchend', onEnd);
}

function updateTrimHandlePositions() {
  const wrapper = document.getElementById('trim-chart-wrapper');
  if (!wrapper || trimState.points.length === 0) return;
  const W = wrapper.clientWidth;
  const axisVals = trimState.axis === 'time' ? trimState.cumTime : trimState.cumDist;
  const total = axisVals[axisVals.length - 1] || 1;

  const startFrac = axisVals[trimState.startIdx] / total;
  const endFrac   = axisVals[trimState.endIdx]   / total;

  const lh = document.getElementById('trim-handle-left');
  const rh = document.getElementById('trim-handle-right');
  const ol = document.getElementById('trim-overlay-left');
  const or_ = document.getElementById('trim-overlay-right');

  lh.style.left  = `${startFrac * 100}%`;
  rh.style.left  = `${endFrac   * 100}%`;
  ol.style.width = `${startFrac * 100}%`;
  or_.style.width = `${(1 - endFrac) * 100}%`;
}

function updateTrimStats() {
  const { cumTime, cumDist, startIdx, endIdx, points } = trimState;
  if (points.length === 0) return;

  const totalSec  = cumTime[points.length - 1] || 0;
  const cutStart  = cumTime[startIdx] || 0;
  const cutEnd    = totalSec - (cumTime[endIdx] || 0);

  document.getElementById('trim-stat-total').textContent  = fmtDurHMS(totalSec);
  document.getElementById('trim-stat-start').textContent  = fmtDurHMS(cutStart);
  document.getElementById('trim-stat-end').textContent    = fmtDurHMS(cutEnd);

  const selSec  = (cumTime[endIdx] || 0) - (cumTime[startIdx] || 0);
  const selDist = (cumDist[endIdx] || 0) - (cumDist[startIdx] || 0);
  document.getElementById('trim-selection-info').textContent =
    `选中：${fmtDurHMS(selSec)}  ${(selDist / 1000).toFixed(2)} km`;
}

function renderTrimAxisLabels() {
  const { cumTime, cumDist, axis, points } = trimState;
  if (points.length === 0) return;
  const axisEl = document.getElementById('trim-time-axis');
  const vals = axis === 'time' ? cumTime : cumDist;
  const total = vals[vals.length - 1] || 1;
  const ticks = 5;
  axisEl.innerHTML = Array.from({ length: ticks + 1 }, (_, i) => {
    const v = total * i / ticks;
    return `<span>${axis === 'time' ? fmtDurHMS(v) : (v / 1000).toFixed(1) + 'km'}</span>`;
  }).join('');
}

function fmtDurHMS(secs) {
  const s = Math.round(secs);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  return `${String(h).padStart(2,'0')}:${String(m).padStart(2,'0')}:${String(ss).padStart(2,'0')}`;
}

function trimHaversine(lat1, lon1, lat2, lon2) {
  const R = 6371000;
  const [la1, la2] = [lat1 * Math.PI / 180, lat2 * Math.PI / 180];
  const dLon = (lon2 - lon1) * Math.PI / 180;
  const dLat = la2 - la1;
  const a = Math.sin(dLat/2)**2 + Math.cos(la1)*Math.cos(la2)*Math.sin(dLon/2)**2;
  return 2 * R * Math.asin(Math.sqrt(a));
}

async function saveTrim() {
  const { activityId, points, startIdx, endIdx } = trimState;
  if (!activityId || points.length === 0) return;

  const startTime = points[startIdx].ts;
  const endTime   = points[endIdx].ts;
  const cutStart  = fmtDurHMS(trimState.cumTime[startIdx] || 0);
  const cutEnd    = fmtDurHMS((trimState.cumTime[points.length-1]||0) - (trimState.cumTime[endIdx]||0));

  const confirmed = confirm(
    `确认剪辑运动？此操作不可撤销。\n\n` +
    `• 删除开头 ${cutStart}\n` +
    `• 删除结尾 ${cutEnd}\n\n` +
    `轨迹点和统计数据将被永久修改。`
  );
  if (!confirmed) return;

  const btn = document.getElementById('trim-save-btn');
  btn.disabled = true;
  btn.textContent = '保存中…';

  const res = await fetch(`/api/activities/${activityId}/trim`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ start_time: startTime, end_time: endTime }),
  });

  btn.disabled = false;
  btn.textContent = '保存剪辑';

  if (!res.ok) {
    alert('保存失败，请重试');
    return;
  }

  closeTrimModal();
  // Reload the activity detail to reflect new data
  showActivityDetail(activityId);
}

// ── Activity Merge ────────────────────────────────────────────────────────────

function openMergeModal() {
  const sel = activitiesState.selected;
  if (sel.size < 2) return;

  // Sort selected activities by start_time
  const acts = [...sel.values()].sort((a, b) => {
    if (!a.start_time) return 1;
    if (!b.start_time) return -1;
    return a.start_time < b.start_time ? -1 : 1;
  });

  const type = acts[0].activity_type;
  const typeNames = { running:'跑步', hiking:'徒步', cycling:'骑行', walking:'步行', trail_running:'越野跑', swimming:'游泳', other:'运动' };
  const typeName = typeNames[type] || '运动';

  // Summary stats
  const totalDuration = acts.reduce((s, a) => s + (a.duration_seconds || 0), 0);
  const totalDistance = acts.reduce((s, a) => s + (a.distance_meters || 0), 0);
  const totalElevation = acts.reduce((s, a) => s + (a.elevation_gain_meters || 0), 0);
  const maxHr = Math.max(...acts.map(a => a.max_heart_rate || 0)) || null;

  // Gap time: span from first start to last end minus total active duration
  let gapSecs = 0;
  const firstStart = acts[0].start_time ? new Date(acts[0].start_time) : null;
  const lastEnd = acts[acts.length - 1].end_time ? new Date(acts[acts.length - 1].end_time) : null;
  if (firstStart && lastEnd) {
    const spanSecs = (lastEnd - firstStart) / 1000;
    gapSecs = Math.max(0, spanSecs - totalDuration);
  }

  // Auto-generated title (client-side approximation, server will add city)
  const dateStr = acts[0].start_time
    ? new Date(acts[0].start_time).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' }).replace('/', '-')
    : '';
  const distStr = totalDistance >= 1000
    ? `${(totalDistance / 1000).toFixed(1)}km`
    : `${Math.round(totalDistance)}m`;
  const autoTitle = dateStr ? `${typeName}-${dateStr}-${distStr}` : '';

  // Populate modal
  document.getElementById('act-merge-subtitle').textContent =
    `即将合并以下 ${acts.length} 次${typeName}：`;

  const list = document.getElementById('act-merge-list');
  list.innerHTML = acts.map(a => {
    const date = a.start_time ? new Date(a.start_time).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '未知时间';
    const dist = a.distance_meters ? `  ${(a.distance_meters / 1000).toFixed(2)} km` : '';
    const dur = a.duration_seconds ? `  ${formatDuration(a.duration_seconds)}` : '';
    return `<li>${a.title || activityDefaultTitle(a)}<span style="color:#888;font-size:12px">&nbsp;${date}${dist}${dur}</span></li>`;
  }).join('');

  const stats = document.getElementById('act-merge-stats');
  stats.innerHTML = [
    totalDuration ? `<span style="color:#555">时长</span><span>${formatDuration(totalDuration)}（含间隔 ${formatDuration(Math.round(gapSecs))}）</span>` : '',
    totalDistance ? `<span style="color:#555">距离</span><span>${(totalDistance / 1000).toFixed(2)} km</span>` : '',
    totalElevation ? `<span style="color:#555">爬升</span><span>${Math.round(totalElevation)} m</span>` : '',
    maxHr ? `<span style="color:#555">最高心率</span><span>${maxHr} bpm</span>` : '',
  ].filter(Boolean).join('');

  const gapWarn = document.getElementById('act-merge-gap-warn');
  if (gapSecs > 86400) {
    gapWarn.textContent = `⚠ 所选运动之间最大间隔超过 24 小时，请确认是否需要合并。`;
    gapWarn.classList.remove('hidden');
  } else {
    gapWarn.classList.add('hidden');
  }

  document.getElementById('act-merge-title-input').value = autoTitle;

  const modal = document.getElementById('act-merge-modal');
  modal.classList.remove('hidden');

  document.getElementById('act-merge-cancel-btn').onclick = () => modal.classList.add('hidden');
  document.getElementById('act-merge-confirm-btn').onclick = () => saveMerge(acts);
}

async function saveMerge(acts) {
  const titleInput = document.getElementById('act-merge-title-input').value.trim();
  const ids = acts.map(a => a.id);
  const body = { ids, title: titleInput || undefined };

  document.getElementById('act-merge-confirm-btn').disabled = true;
  document.getElementById('act-merge-confirm-btn').textContent = '合并中…';

  const res = await fetch('/api/activities/merge', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  document.getElementById('act-merge-confirm-btn').disabled = false;
  document.getElementById('act-merge-confirm-btn').textContent = '确认合并';

  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    const msgs = {
      type_mismatch: '类型不同，无法合并',
      time_overlap: '所选运动时间有重叠，无法合并',
      missing_times: '部分记录缺少时间信息，无法合并',
      not_found: '部分记录未找到',
      too_few: '请至少选择 2 个运动',
    };
    alert(msgs[err.error] || `合并失败（${res.status}）`);
    return;
  }

  document.getElementById('act-merge-modal').classList.add('hidden');
  exitMultiSelect();
}

// ── Helpers ───────────────────────────────────────────────────────────────────
async function fetchJSON(url, options = {}) {
  try {
    const res = await fetch(url, options);
    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}
