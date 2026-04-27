'use strict';

const state = {
  page: 1,
  perPage: 50,
  total: 0,
  albumId: null,
  importPollId: null,
  photos: [],       // current page photos for the Photos tab
  detailPhotos: [], // photos used for prev/next navigation in the open detail modal
  detailIdx: -1,    // index into detailPhotos of the currently open detail
  selectMode: false,
  selected: new Set(), // selected photo IDs
  currentDetail: null, // full detail object of the open photo
  currentView: 'photos', // 'photos' | 'people' | 'locations' | 'animals'
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
  // Animals
  animalSpecies: null,  // current species being browsed
  animalPage: 1,
  animalTotal: 0,
  animalPhotos: [],
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
  loadPhotos();

  document.getElementById('import-btn').addEventListener('click', startImport);
  document.getElementById('prev-btn').addEventListener('click', () => changePage(-1));
  document.getElementById('next-btn').addEventListener('click', () => changePage(1));
  document.getElementById('dedup-btn').addEventListener('click', openDedupModal);
  document.getElementById('close-dedup').addEventListener('click', () => {
    document.getElementById('dedup-modal').classList.add('hidden');
  });

  // Detail edit
  // Geo view sub-tabs
  document.querySelectorAll('[data-geoview]').forEach(btn => {
    btn.addEventListener('click', () => switchGeoView(btn.dataset.geoview));
  });

  // People view
  document.getElementById('fill-meta-btn').addEventListener('click', fillMissingMeta);
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

  // Batch select
  document.getElementById('select-toggle-btn').addEventListener('click', toggleSelectMode);
  document.getElementById('batch-deselect-btn').addEventListener('click', () => {
    clearSelection();
    toggleSelectMode(); // exit select mode
  });
  document.getElementById('batch-time-btn').addEventListener('click', () => {
    document.getElementById('batch-taken-at').value = '';
    document.getElementById('batch-timezone').value = '';
    document.getElementById('batch-time-modal').classList.remove('hidden');
  });
  document.getElementById('batch-time-cancel-btn').addEventListener('click', () => {
    document.getElementById('batch-time-modal').classList.add('hidden');
  });
  document.getElementById('batch-time-save-btn').addEventListener('click', saveBatchTime);

  // Animals view
  document.getElementById('animal-back-btn').addEventListener('click', showAnimalSpeciesList);
  document.getElementById('animal-prev-btn').addEventListener('click', () => changeAnimalPage(-1));
  document.getElementById('animal-next-btn').addEventListener('click', () => changeAnimalPage(1));

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
  const url = state.albumId
    ? `/api/albums/${state.albumId}/photos?page=${state.page}&per_page=${state.perPage}`
    : `/api/photos?page=${state.page}&per_page=${state.perPage}`;

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

  // Fetch and draw animal boxes
  const animals = await fetchJSON(`/api/photos/${photo.id}/animals`);
  if (animals) renderAnimalOverlay(animals);

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
  document.getElementById('detail-animals').innerHTML = '';
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
  li.classList.add('active');
  state.albumId = albumId;
  state.page = 1;
  loadPhotos();
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
    div.innerHTML = `<h3>重复组 #${g.group_id}</h3>
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
      el.innerHTML = `
        <img src="/api/photos/${m.photo_id}/thumb" alt="">
        <p title="${m.path}">${m.path.split('/').pop()}</p>`;
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
    container.appendChild(div);
  }

  document.getElementById('dedup-modal').classList.remove('hidden');
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
  if (view === 'animals') loadAnimalSpecies();
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
      geoCurrentGeoFilter = { country: countryName, state: st.name, city: cityParam };
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
}

async function loadCentroidPhotoIds(personId) {
  const data = await fetchJSON(`/api/people/${personId}/centroid-faces`);
  state.centroidPhotoIds = new Set(data ? data.photo_ids : []);
}

function showFaceLightbox(src) {
  const lb = document.getElementById('face-lightbox');
  const img = document.getElementById('face-lightbox-img');
  img.src = src;
  lb.style.display = 'flex';
}

function hideFaceLightbox() {
  document.getElementById('face-lightbox').style.display = 'none';
}

document.addEventListener('DOMContentLoaded', () => {
  const lb = document.getElementById('face-lightbox');
  if (lb) lb.addEventListener('click', hideFaceLightbox);
});

async function loadMergeSuggestions(personId, person) {
  const panel = document.getElementById('merge-suggestions-panel');
  const list = document.getElementById('merge-suggestions-list');
  list.innerHTML = '';
  if (!person || !person.name) {
    panel.classList.add('hidden');
    return;
  }
  const suggestions = await fetchJSON(`/api/people/${personId}/merge-suggestions?limit=5`);
  if (!suggestions || suggestions.length === 0) {
    panel.classList.add('hidden');
    return;
  }
  panel.classList.remove('hidden');
  for (const s of suggestions) {
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
    mergeBtn.addEventListener('click', async () => {
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
    });

    card.append(img, nameEl, meta, mergeBtn);
    list.appendChild(card);
  }
}

async function loadOutlierFaces(personId) {
  const panel = document.getElementById('outlier-faces-panel');
  const list = document.getElementById('outlier-faces-list');
  list.innerHTML = '';

  const outliers = await fetchJSON(`/api/people/${personId}/outlier-faces?limit=5`);
  if (!outliers || outliers.length === 0) {
    panel.classList.add('hidden');
    return;
  }
  panel.classList.remove('hidden');

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
        if (list.children.length === 0) panel.classList.add('hidden');
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
      if (list.children.length === 0) panel.classList.add('hidden');
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
  if (!tree) { el.textContent = '顶级'; return; }

  const path = [];
  function findPath(nodes, targetId) {
    for (const n of nodes) {
      if (n.id === targetId) { path.push(n.name || '未命名'); return true; }
      if (findPath(n.children || [], targetId)) { path.unshift(n.name || '未命名'); return true; }
    }
    return false;
  }
  findPath(tree.people || [], personId);
  // Remove the last element (the person itself) – show ancestors only
  path.pop();
  el.textContent = path.length ? path.join(' > ') : '顶级';
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

  menu.querySelector('[data-action="ignore"]').addEventListener('click', async (e) => {
    e.stopPropagation();
    closePersonMenu();
    if (!confirm('确定要忽略此人？此操作可撤销。')) return;
    const ok = await patchPerson(personId, { status: 'ignored' });
    if (ok) {
      removePersonCard(personId, card);
      pushUndo('忽略此人', async () => { await patchPerson(personId, { status: 'active' }); });
    }
  });
  menu.querySelector('[data-action="not-person"]').addEventListener('click', async (e) => {
    e.stopPropagation();
    closePersonMenu();
    if (!confirm('确定要标记为非人物？此操作可撤销。')) return;
    const ok = await patchPerson(personId, { status: 'not_a_person' });
    if (ok) {
      removePersonCard(personId, card);
      pushUndo('标记为非人物', async () => { await patchPerson(personId, { status: 'active' }); });
    }
  });
}

function removePersonCard(personId, card) {
  state.allPeople = state.allPeople.filter(p => p.id !== personId);
  document.getElementById('people-count').textContent = `共 ${state.allPeople.length} 人`;
  card.style.transition = 'opacity 0.25s';
  card.style.opacity = '0';
  setTimeout(() => card.remove(), 250);
}

async function fillMissingMeta() {
  const btn = document.getElementById('fill-meta-btn');
  const statusEl = document.getElementById('fill-meta-status');
  btn.disabled = true;
  statusEl.textContent = '启动中…';

  const [faceRes, geoRes] = await Promise.all([
    fetchJSON('/api/faces/analyze', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ missing_only: true }) }),
    fetchJSON('/api/geo/regeocode', { method: 'POST' }),
  ]);

  if (!faceRes && !geoRes) {
    statusEl.textContent = '启动失败';
    btn.disabled = false;
    return;
  }

  const faceJobId = faceRes && faceRes.job_id;
  let geoCount = geoRes ? geoRes.count : 0;
  let geoStatus = geoRes ? geoRes.status : 'done';

  function updateStatus(faceText, geoText) {
    statusEl.textContent = `人脸：${faceText} | 地理：${geoText}`;
  }

  const pollId = setInterval(async () => {
    const [faceJob, geoSt] = await Promise.all([
      faceJobId ? fetchJSON(`/api/faces/jobs/${faceJobId}`) : Promise.resolve(null),
      fetchJSON('/api/geo/regeocode/status'),
    ]);

    const faceText = faceJob
      ? (faceJob.status === 'running'
          ? `${faceJob.processed}/${faceJob.total ?? '?'}`
          : faceJob.status === 'done' ? '完成' : '失败')
      : (faceRes ? '完成' : '跳过');

    const geoRunning = geoSt && geoSt.running;
    const geoText = geoRunning
      ? `处理中（${geoCount} 张待处理）`
      : (geoStatus === 'already_running' ? '已在运行' : `完成（${geoCount} 张）`);

    const faceDone = !faceJob || faceJob.status !== 'running';
    if (faceDone && !geoRunning) {
      clearInterval(pollId);
      btn.disabled = false;
      updateStatus(faceText, geoText);
      if (state.currentView === 'locations') loadGeoHierarchy();
      if (state.currentView === 'people') loadPeopleList();
    } else {
      updateStatus(faceText, geoText);
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
  const people = await fetchJSON('/api/people');
  if (!people) return;
  state.allPeople = people;
  state.mergeTargetId = null;
  document.getElementById('merge-confirm-btn').disabled = true;
  document.getElementById('merge-search').value = '';
  renderMergeList(people.filter(p => p.id !== state.currentPersonId));
  document.getElementById('merge-modal').classList.remove('hidden');
}

function renderMergeList(people) {
  const ul = document.getElementById('merge-target-list');
  ul.innerHTML = '';
  for (const p of people) {
    const li = document.createElement('li');
    li.style.cssText = 'padding:6px 10px;cursor:pointer;';
    li.textContent = (p.name || '未命名') + ` (${p.photo_count} 张)`;
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
    p.id !== state.currentPersonId &&
    (p.name || '未命名').toLowerCase().includes(q)
  );
  renderMergeList(filtered);
}

async function confirmMerge() {
  if (!state.mergeTargetId) return;
  await fetch('/api/people/merge', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ source_id: state.currentPersonId, target_id: state.mergeTargetId }),
  });
  document.getElementById('merge-modal').classList.add('hidden');
  loadPeopleList();
  showPersonDetail(state.mergeTargetId);
}

// ── Animals ───────────────────────────────────────────────────────────────────
const SPECIES_EMOJI = {
  bird: '🐦', cat: '🐱', dog: '🐶', horse: '🐴', sheep: '🐑',
  cow: '🐄', elephant: '🐘', bear: '🐻', zebra: '🦓', giraffe: '🦒',
};

async function loadAnimalSpecies() {
  const data = await fetchJSON('/api/animals/species');
  if (!data) return;
  const grid = document.getElementById('animal-species-grid');
  const count = document.getElementById('animal-count');
  count.textContent = `共 ${data.length} 种动物`;
  grid.innerHTML = '';
  for (const s of data) {
    const card = document.createElement('div');
    card.className = 'species-card';
    const emoji = SPECIES_EMOJI[s.species] || '🐾';
    card.innerHTML = `
      <span class="species-emoji">${emoji}</span>
      <div class="species-name">${s.chinese}</div>
      <div class="species-count">${s.photo_count} 张</div>`;
    card.addEventListener('click', () => showAnimalSpeciesPhotos(s.species, s.chinese));
    grid.appendChild(card);
  }
  showAnimalSpeciesList();
}

function showAnimalSpeciesList() {
  document.getElementById('animal-species-section').classList.remove('hidden');
  document.getElementById('animal-photos-section').classList.add('hidden');
  state.animalSpecies = null;
}

async function showAnimalSpeciesPhotos(species, chinese) {
  state.animalSpecies = species;
  state.animalPage = 1;
  document.getElementById('animal-species-title').textContent = `${SPECIES_EMOJI[species] || '🐾'} ${chinese}`;
  document.getElementById('animal-species-section').classList.add('hidden');
  document.getElementById('animal-photos-section').classList.remove('hidden');
  await loadAnimalPhotos();
}

async function loadAnimalPhotos() {
  const data = await fetchJSON(
    `/api/animals/${state.animalSpecies}/photos?page=${state.animalPage}&per_page=${state.perPage}`
  );
  if (!data) return;
  state.animalTotal = data.total;
  state.animalPhotos = data.photos;

  // Render into animal-photos-grid using the same card style
  const grid = document.getElementById('animal-photos-grid');
  grid.innerHTML = '';
  data.photos.forEach((p, idx) => {
    const card = document.createElement('div');
    card.className = 'photo-card';
    const label = p.taken_at ? p.taken_at.slice(0, 10) : p.path.split('/').pop();
    card.innerHTML = `
      <img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    card.addEventListener('click', () => openDetail(idx, state.animalPhotos));
    grid.appendChild(card);
  });

  // Pagination
  const totalPages = Math.max(1, Math.ceil(data.total / state.perPage));
  document.getElementById('animal-page-info').textContent =
    `${state.animalPage} / ${totalPages}`;
  document.getElementById('animal-prev-btn').disabled = state.animalPage <= 1;
  document.getElementById('animal-next-btn').disabled = state.animalPage >= totalPages;
}

function changeAnimalPage(delta) {
  state.animalPage = Math.max(1, state.animalPage + delta);
  loadAnimalPhotos();
}


function renderAnimalOverlay(animals) {
  const svg = document.getElementById('detail-animals');
  svg.innerHTML = '';
  if (!animals.length) return;

  const img = document.getElementById('detail-img');
  const w = img.naturalWidth || img.width || 1;
  const h = img.naturalHeight || img.height || 1;
  svg.setAttribute('viewBox', `0 0 ${w} ${h}`);

  for (const a of animals) {
    const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    rect.setAttribute('x', a.x);
    rect.setAttribute('y', a.y);
    rect.setAttribute('width', a.width);
    rect.setAttribute('height', a.height);
    rect.setAttribute('class', 'animal-box');
    svg.appendChild(rect);

    const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    text.setAttribute('x', a.x + 2);
    text.setAttribute('y', a.y > 14 ? a.y - 3 : a.y + a.height + 12);
    text.setAttribute('class', 'animal-label');
    text.textContent = `${SPECIES_EMOJI[a.species] || '🐾'} ${a.species}`;
    svg.appendChild(text);
  }
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
