'use strict';

const state = {
  page: 1,
  perPage: 50,
  total: 0,
  albumId: null,
  importPollId: null,
  photos: [],       // current page photos for detail navigation
  detailIdx: -1,    // index into state.photos of currently open detail
};

// ── Init ──────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  loadAlbums();
  loadPhotos();

  document.getElementById('import-btn').addEventListener('click', startImport);
  document.getElementById('prev-btn').addEventListener('click', () => changePage(-1));
  document.getElementById('next-btn').addEventListener('click', () => changePage(1));
  document.getElementById('dedup-btn').addEventListener('click', openDedupModal);
  document.getElementById('close-dedup').addEventListener('click', () => {
    document.getElementById('dedup-modal').classList.add('hidden');
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
    card.className = 'photo-card';
    const label = p.taken_at ? p.taken_at.slice(0, 10) : p.path.split('/').pop();
    card.innerHTML = `
      <img src="/api/photos/${p.id}/thumb" loading="lazy" alt="${label}">
      <div class="meta">${label}</div>`;
    card.addEventListener('click', () => openDetail(idx));
    grid.appendChild(card);
  });
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
  if (detail) renderDetailMeta(detail);

  // Fetch and draw face boxes
  const faces = await fetchJSON(`/api/photos/${photo.id}/faces`);
  if (faces) renderFaceOverlay(faces);

  updateDetailNav();
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

// ── Helpers ───────────────────────────────────────────────────────────────────
async function fetchJSON(url) {
  try {
    const res = await fetch(url);
    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}
