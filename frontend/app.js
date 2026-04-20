'use strict';

const state = {
  page: 1,
  perPage: 50,
  total: 0,
  albumId: null,
  importPollId: null,
  photos: [],       // current page photos for detail navigation
  detailIdx: -1,    // index into state.photos of currently open detail
  selectMode: false,
  selected: new Set(), // selected photo IDs
  currentDetail: null, // full detail object of the open photo
  currentView: 'photos', // 'photos' | 'people' | 'locations' | 'animals'
  currentPersonId: null, // person being viewed in detail
  allPeople: [],    // cached people list for merge dialog
  mergeTargetId: null,
  // Animals
  animalSpecies: null,  // current species being browsed
  animalPage: 1,
  animalTotal: 0,
  animalPhotos: [],
};

// ── Init ──────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // Tab navigation
  document.querySelectorAll('.tab-btn').forEach(btn => {
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
  document.getElementById('recluster-btn').addEventListener('click', triggerRecluster);
  document.getElementById('person-back-btn').addEventListener('click', () => showPeopleList());
  document.getElementById('person-name-input').addEventListener('change', savePersonName);
  document.getElementById('person-merge-btn').addEventListener('click', openMergeDialog);
  document.getElementById('merge-cancel-btn').addEventListener('click', () => {
    document.getElementById('merge-modal').classList.add('hidden');
  });
  document.getElementById('merge-confirm-btn').addEventListener('click', confirmMerge);
  document.getElementById('merge-search').addEventListener('input', filterMergeList);

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
async function openDetail(idx) {
  const photo = state.photos[idx];
  if (!photo) return;
  state.detailIdx = idx;

  const modal = document.getElementById('detail-modal');
  modal.classList.remove('hidden');

  // Show thumb immediately, then swap to original if available
  const img = document.getElementById('detail-img');
  img.src = `/api/photos/${photo.id}/thumb`;

  // Fetch full metadata
  const detail = await fetchJSON(`/api/photos/${photo.id}`);
  if (detail) { state.currentDetail = detail; renderDetailMeta(detail); }
  cancelDetailEdit();

  // Fetch and draw face boxes
  const faces = await fetchJSON(`/api/photos/${photo.id}/faces`);
  if (faces) renderFaceOverlay(faces);

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
}

function navigateDetail(delta) {
  const next = state.detailIdx + delta;
  if (next >= 0 && next < state.photos.length) openDetail(next);
}

function updateDetailNav() {
  document.getElementById('detail-prev').disabled = state.detailIdx <= 0;
  document.getElementById('detail-next').disabled =
    state.detailIdx >= state.photos.length - 1;
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
  allLi.innerHTML = `全部照片 <span class="count"></span>`;
  allLi.addEventListener('click', () => selectAlbum(null, allLi));
  allLi.classList.add('active');
  ul.appendChild(allLi);

  for (const a of albums) {
    const li = document.createElement('li');
    li.dataset.kind = a.kind;
    li.innerHTML = `${a.name} <span class="count">${a.photo_count}</span>`;
    li.addEventListener('click', () => selectAlbum(a.id, li));
    ul.appendChild(li);
  }
}

function selectAlbum(albumId, li) {
  document.querySelectorAll('#album-list li').forEach(el => el.classList.remove('active'));
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

async function loadGeoHierarchy() {
  geoData = await fetchJSON('/api/geo/hierarchy');
  if (!geoData) return;
  renderGeoCountries(geoData.countries);
  document.getElementById('geo-col-state').classList.add('hidden');
  document.getElementById('geo-col-city').classList.add('hidden');
  document.getElementById('geo-photos').style.display = 'none';
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
      renderGeoStates(c);
      setBreadcrumb(c.name);
    });
    ul.appendChild(li);
  }
}

function renderGeoStates(country) {
  const stateCol = document.getElementById('geo-col-state');
  stateCol.classList.remove('hidden');
  document.getElementById('geo-col-city').classList.add('hidden');
  document.getElementById('geo-photos').style.display = 'none';

  const ul = document.getElementById('geo-state-list');
  ul.innerHTML = '';
  for (const s of country.states) {
    const li = document.createElement('li');
    li.innerHTML = `<span>${s.name}</span><span class="geo-count">${s.photo_count}</span>`;
    li.addEventListener('click', () => {
      ul.querySelectorAll('li').forEach(x => x.classList.remove('active'));
      li.classList.add('active');
      renderGeoCities(s, country.name);
      setBreadcrumb(`${country.name} › ${s.name}`);
    });
    ul.appendChild(li);
  }
}

function renderGeoCities(st, countryName) {
  const cityCol = document.getElementById('geo-col-city');
  cityCol.classList.remove('hidden');
  document.getElementById('geo-photos').style.display = 'none';

  const ul = document.getElementById('geo-city-list');
  ul.innerHTML = '';
  for (const c of st.cities) {
    const li = document.createElement('li');
    li.innerHTML = `<span>${c.name}</span><span class="geo-count">${c.photo_count}</span>`;
    li.addEventListener('click', async () => {
      ul.querySelectorAll('li').forEach(x => x.classList.remove('active'));
      li.classList.add('active');
      setBreadcrumb(`${countryName} › ${st.name} › ${c.name}`);
      // Show photos for this city (via album if one exists)
      const albums = await fetchJSON('/api/albums');
      const album = albums && albums.find(a => a.kind === 'location' && a.name === c.name);
      if (album) {
        const data = await fetchJSON(`/api/albums/${album.id}/photos?per_page=100`);
        const photos = data ? (data.photos || []) : [];
        renderGeoPhotos(photos);
      }
    });
    ul.appendChild(li);
  }
}

function renderGeoPhotos(photos) {
  const grid = document.getElementById('geo-photos');
  grid.style.display = '';
  grid.innerHTML = '';
  for (const p of photos) {
    const card = document.createElement('div');
    card.className = 'photo-card';
    const label = p.taken_at ? p.taken_at.slice(0, 10) : '';
    card.innerHTML = `<img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    grid.appendChild(card);
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
  const mapDiv = document.getElementById('geo-map-view');
  mapDiv.classList.toggle('hidden', view !== 'map');
  if (view === 'map') {
    // Delay to let the div become visible before initialising Leaflet
    setTimeout(initMap, 50);
  }
}

let leafletMap = null;

async function initMap() {
  // Leaflet may not be loaded if offline
  if (typeof L === 'undefined') {
    document.getElementById('leaflet-map').textContent = '地图需要网络连接才能加载（Leaflet CDN）';
    return;
  }

  const mapEl = document.getElementById('leaflet-map');
  if (leafletMap) {
    leafletMap.invalidateSize();
    return;
  }

  leafletMap = L.map(mapEl).setView([20, 0], 2);
  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '© OpenStreetMap contributors',
    maxZoom: 19,
  }).addTo(leafletMap);

  const pts = await fetchJSON('/api/photos/gps-points');
  if (!pts || pts.length === 0) return;

  const cluster = L.markerClusterGroup();
  for (const p of pts) {
    const marker = L.marker([p.gps_lat, p.gps_lon]);
    const date = p.taken_at ? p.taken_at.slice(0, 10) : '未知日期';
    marker.bindPopup(`
      <div style="text-align:center">
        <img src="/api/photos/${p.id}/thumb" style="max-width:120px;max-height:120px;display:block;margin:0 auto 4px">
        <div style="font-size:12px">${date}</div>
      </div>`);
    cluster.addLayer(marker);
  }
  leafletMap.addLayer(cluster);

  // Fit to markers
  if (pts.length > 0) {
    const bounds = L.latLngBounds(pts.map(p => [p.gps_lat, p.gps_lon]));
    leafletMap.fitBounds(bounds, { padding: [40, 40] });
  }
}

// ── People list ───────────────────────────────────────────────────────────────
async function loadPeopleList() {
  const people = await fetchJSON('/api/people');
  if (!people) return;
  state.allPeople = people;

  document.getElementById('people-count').textContent = `共 ${people.length} 人`;
  const grid = document.getElementById('people-grid');
  grid.innerHTML = '';

  if (people.length === 0) {
    grid.innerHTML = '<p style="padding:24px;color:#888">尚无人物，请先导入含人脸的照片，再点击"重新聚类"。</p>';
    return;
  }

  for (const p of people) {
    const card = document.createElement('div');
    card.className = 'person-card';
    const thumbSrc = p.cover_face_id
      ? `/api/faces/${p.cover_face_id}/thumb`
      : 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="%23ddd"/></svg>';
    card.innerHTML = `
      <img src="${thumbSrc}" loading="lazy" alt="">
      <div class="person-meta">
        <div class="person-name">${p.name || '未命名'}</div>
        <div class="person-count">${p.photo_count} 张照片</div>
      </div>`;
    card.addEventListener('click', () => showPersonDetail(p.id));
    grid.appendChild(card);
  }
}

function showPeopleList() {
  state.currentPersonId = null;
  document.getElementById('people-list-section').classList.remove('hidden');
  document.getElementById('person-detail-section').classList.add('hidden');
  loadPeopleList();
}

async function showPersonDetail(personId) {
  state.currentPersonId = personId;
  document.getElementById('people-list-section').classList.add('hidden');
  document.getElementById('person-detail-section').classList.remove('hidden');

  // Load person info for the name field
  const people = state.allPeople;
  const person = people.find(p => p.id === personId);
  document.getElementById('person-name-input').value = person ? (person.name || '') : '';
  document.getElementById('person-name-input').dataset.personId = personId;

  // Load photos for this person
  const data = await fetchJSON(`/api/people/${personId}/photos?per_page=100`);
  const photos = data ? (data.photos || data) : [];
  const grid = document.getElementById('person-photos-grid');
  grid.innerHTML = '';
  for (const p of photos) {
    const card = document.createElement('div');
    card.className = 'photo-card';
    const label = p.taken_at ? p.taken_at.slice(0, 10) : '';
    card.innerHTML = `<img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    grid.appendChild(card);
  }

  // Load sub-persons
  await loadSubPersons(personId);
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
    row.innerHTML = `<span>${child.name || '未命名'}</span>
      <button class="btn-ghost" data-cid="${child.id}">移出</button>`;
    row.querySelector('button').addEventListener('click', async () => {
      await fetch(`/api/people/${child.id}/reparent`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ new_parent_id: null }),
      });
      loadSubPersons(personId);
    });
    list.appendChild(row);
  }
}

async function savePersonName() {
  const input = document.getElementById('person-name-input');
  const personId = +input.dataset.personId;
  if (!personId) return;
  // We don't have a PATCH /api/people/:id yet; store in state for now
  // and will add the endpoint in a later step if needed
  const p = state.allPeople.find(x => x.id === personId);
  if (p) p.name = input.value;
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
      ? `处理中（${geoCount} 张待编码）`
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

async function triggerRecluster() {
  const btn = document.getElementById('recluster-btn');
  btn.disabled = true;
  document.getElementById('recluster-status').textContent = '聚类中…';
  const result = await fetchJSON('/api/people/cluster', { method: 'POST' });
  btn.disabled = false;
  if (result) {
    document.getElementById('recluster-status').textContent =
      `完成，生成 ${result.people_created} 个人物`;
    loadPeopleList();
  } else {
    document.getElementById('recluster-status').textContent = '聚类失败';
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
    card.addEventListener('click', () => openAnimalPhotoDetail(idx));
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

async function openAnimalPhotoDetail(idx) {
  // Temporarily swap state.photos so the detail navigator works within this species view
  const saved = state.photos;
  state.photos = state.animalPhotos;
  await openDetail(idx);
  state.photos = saved;
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
