<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AlbumPhotoPage, GeoCluster, GeoClusterPage } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient; initialPage: GeoClusterPage }
  let { api, initialPage }: Props = $props();

  const TILE_SIZE = 256;
  const MIN_ZOOM = 2;
  const MAX_ZOOM = 18;
  const initialFocus = untrack(() => initialPage.clusters.reduce<GeoCluster | null>(
    (largest, cluster) => !largest || cluster.photo_count > largest.photo_count ? cluster : largest,
    null,
  ));
  let mapElement = $state<HTMLDivElement>();
  let width = $state(960);
  let height = $state(480);
  let zoom = $state(2);
  let centerLat = $state(initialFocus?.gps_lat ?? 20);
  let centerLon = $state(initialFocus?.gps_lon ?? 0);
  let clusters = $state<GeoCluster[]>(untrack(() => initialPage.clusters));
  let visibleTotal = $state(untrack(() => initialPage.total_photos));
  let loading = $state(false);
  let fullscreen = $state(false);
  let selectedCluster = $state<GeoCluster | null>(null);
  let selectedPhotos = $state<AlbumPhotoPage | null>(null);
  let selectedLoading = $state(false);
  let mapError = $state<string | null>(null);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let drag = $state<{ x: number; y: number; worldX: number; worldY: number; moved: boolean } | null>(null);

  onMount(() => {
    if (mapElement) {
      width = mapElement.clientWidth || width;
      height = mapElement.clientHeight || height;
    }
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => {
      if (!mapElement) return;
      width = mapElement.clientWidth || width;
      height = mapElement.clientHeight || height;
      scheduleRefresh();
    });
    if (mapElement) observer?.observe(mapElement);
    return () => {
      observer?.disconnect();
      if (refreshTimer) clearTimeout(refreshTimer);
    };
  });

  function scale(atZoom = zoom) { return TILE_SIZE * 2 ** atZoom; }

  function project(lat: number, lon: number, atZoom = zoom) {
    const boundedLat = Math.max(-85.05112878, Math.min(85.05112878, lat));
    const sin = Math.sin((boundedLat * Math.PI) / 180);
    const size = scale(atZoom);
    return {
      x: ((lon + 180) / 360) * size,
      y: (0.5 - Math.log((1 + sin) / (1 - sin)) / (4 * Math.PI)) * size,
    };
  }

  function unproject(x: number, y: number, atZoom = zoom) {
    const size = scale(atZoom);
    const lon = (x / size) * 360 - 180;
    const n = Math.PI - (2 * Math.PI * y) / size;
    return { lat: (180 / Math.PI) * Math.atan(Math.sinh(n)), lon };
  }

  function viewportBounds() {
    const size = scale();
    const center = project(centerLat, centerLon);
    const left = Math.max(0, center.x - width / 2);
    const right = Math.min(size, center.x + width / 2);
    const top = Math.max(0, center.y - height / 2);
    const bottom = Math.min(size, center.y + height / 2);
    const northwest = unproject(left, top);
    const southeast = unproject(right, bottom);
    return {
      west: northwest.lon,
      east: southeast.lon,
      south: southeast.lat,
      north: northwest.lat,
    };
  }

  function visibleTiles() {
    const center = project(centerLat, centerLon);
    const count = 2 ** zoom;
    const startX = Math.max(0, Math.floor((center.x - width / 2) / TILE_SIZE));
    const endX = Math.min(count - 1, Math.floor((center.x + width / 2) / TILE_SIZE));
    const startY = Math.max(0, Math.floor((center.y - height / 2) / TILE_SIZE));
    const endY = Math.min(count - 1, Math.floor((center.y + height / 2) / TILE_SIZE));
    const tiles: Array<{ x: number; y: number; left: number; top: number; url: string }> = [];
    for (let y = startY; y <= endY; y += 1) {
      for (let x = startX; x <= endX; x += 1) {
        tiles.push({
          x, y,
          left: x * TILE_SIZE - center.x + width / 2,
          top: y * TILE_SIZE - center.y + height / 2,
          url: `https://tile.openstreetmap.org/${zoom}/${x}/${y}.png`,
        });
      }
    }
    return tiles;
  }

  function markerPosition(cluster: GeoCluster) {
    const center = project(centerLat, centerLon);
    const point = project(cluster.gps_lat, cluster.gps_lon);
    return { left: point.x - center.x + width / 2, top: point.y - center.y + height / 2 };
  }

  function clusterSize(cluster: GeoCluster) {
    return Math.min(46, 18 + Math.log10(Math.max(1, cluster.photo_count)) * 7);
  }

  function clusterBounds(cluster: GeoCluster) {
    return { west: cluster.west, east: cluster.east, south: cluster.south, north: cluster.north };
  }

  function scheduleRefresh() {
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => { void refreshClusters(); }, 180);
  }

  async function refreshClusters() {
    const bounds = viewportBounds();
    loading = true;
    mapError = null;
    try {
      const page = await api.geo.clusters(
        Math.max(8, Math.min(48, Math.ceil(width / 48))),
        Math.max(6, Math.min(24, Math.ceil(height / 48))),
        bounds,
      );
      clusters = page.clusters;
      visibleTotal = page.total_photos;
    } catch (reason) {
      mapError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      loading = false;
    }
  }

  function setZoom(nextZoom: number) {
    zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, nextZoom));
    selectedCluster = null;
    selectedPhotos = null;
    scheduleRefresh();
  }

  function onPointerDown(event: PointerEvent) {
    if ((event.target as HTMLElement).closest('button, a')) return;
    const center = project(centerLat, centerLon);
    drag = { x: event.clientX, y: event.clientY, worldX: center.x, worldY: center.y, moved: false };
    mapElement?.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    if (!drag) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    const size = scale();
    const point = unproject(
      Math.max(0, Math.min(size, drag.worldX - dx)),
      Math.max(0, Math.min(size, drag.worldY - dy)),
    );
    centerLat = point.lat;
    centerLon = point.lon;
  }

  function onPointerUp(event: PointerEvent) {
    if (!drag) return;
    mapElement?.releasePointerCapture(event.pointerId);
    const moved = drag.moved;
    drag = null;
    if (moved) scheduleRefresh();
  }

  async function openCluster(cluster: GeoCluster) {
    selectedCluster = cluster;
    selectedPhotos = null;
    selectedLoading = true;
    mapError = null;
    try {
      selectedPhotos = await api.geo.clusterPhotos(clusterBounds(cluster));
    } catch (reason) {
      mapError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      selectedLoading = false;
    }
  }

  async function loadMorePhotos() {
    if (!selectedCluster || !selectedPhotos) return;
    const nextPage = selectedPhotos.page + 1;
    const next = await api.geo.clusterPhotos(clusterBounds(selectedCluster), nextPage);
    selectedPhotos = { ...next, photos: [...selectedPhotos.photos, ...next.photos] };
  }

  function zoomToCluster() {
    if (!selectedCluster) return;
    centerLat = selectedCluster.gps_lat;
    centerLon = selectedCluster.gps_lon;
    setZoom(zoom + 2);
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape' && fullscreen) fullscreen = false; }} />

<div class="map-shell" class:fullscreen>
  <div class="map-toolbar">
    <div><strong>照片地图</strong><span>{visibleTotal} 张位于当前地图范围</span></div>
    <div class="map-actions">
      <button type="button" aria-label="缩小地图" disabled={zoom === MIN_ZOOM} onclick={() => setZoom(zoom - 1)}>−</button>
      <span aria-label="地图缩放级别">{zoom}</span>
      <button type="button" aria-label="放大地图" disabled={zoom === MAX_ZOOM} onclick={() => setZoom(zoom + 1)}>+</button>
      <button type="button" aria-label={fullscreen ? '退出全屏地图' : '全屏显示地图'} onclick={() => { fullscreen = !fullscreen; scheduleRefresh(); }}>{fullscreen ? '收起' : '全屏'}</button>
    </div>
  </div>
  <div
    class="map"
    class:dragging={drag?.moved}
    bind:this={mapElement}
    role="application"
    aria-label={`OpenStreetMap 照片地图，缩放级别 ${zoom}`}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    ondblclick={() => setZoom(zoom + 1)}
  >
    <div class="tiles" aria-hidden="true">
      {#each visibleTiles() as tile (`${zoom}:${tile.x}:${tile.y}`)}
        <img src={tile.url} alt="" draggable="false" style:left={`${tile.left}px`} style:top={`${tile.top}px`} />
      {/each}
    </div>
    {#each clusters as cluster (`${cluster.x_bin}:${cluster.y_bin}`)}
      {@const position = markerPosition(cluster)}
      <button
        class="map-cluster"
        class:active={selectedCluster?.x_bin === cluster.x_bin && selectedCluster?.y_bin === cluster.y_bin}
        type="button"
        aria-label={`查看此区域的 ${cluster.photo_count} 张照片`}
        style:left={`${position.left}px`}
        style:top={`${position.top}px`}
        style:width={`${clusterSize(cluster)}px`}
        style:height={`${clusterSize(cluster)}px`}
        onclick={() => void openCluster(cluster)}
      >{cluster.photo_count}</button>
    {/each}
    {#if loading}<div class="map-loading" role="status">正在更新地图…</div>{/if}
    <a class="attribution" href="https://www.openstreetmap.org/copyright" target="_blank" rel="noreferrer">© OpenStreetMap contributors</a>

    {#if selectedCluster}
      <aside class="cluster-panel" aria-label="地图区域照片">
        <div class="cluster-heading">
          <div><strong>此区域</strong><span>{selectedCluster.photo_count} 张照片</span></div>
          <div>
            <Button variant="ghost" onclick={zoomToCluster}>放大到此处</Button>
            <button class="close-panel" type="button" aria-label="关闭地图照片" onclick={() => { selectedCluster = null; selectedPhotos = null; }}>×</button>
          </div>
        </div>
        {#if selectedLoading}
          <PageState kind="loading" title="正在读取照片" />
        {:else if selectedPhotos}
          <div class="cluster-photos">
            {#each selectedPhotos.photos as photo (photo.id)}
              <img src={`/api/photos/${photo.id}/thumb?size=256`} alt={`地图照片 ${photo.id}`} loading="lazy" />
            {/each}
          </div>
          {#if selectedPhotos.photos.length < selectedPhotos.total}
            <Button variant="ghost" onclick={loadMorePhotos}>载入更多</Button>
          {/if}
        {/if}
      </aside>
    {/if}
  </div>
  {#if mapError}<p class="map-error" role="alert">{mapError}</p>{/if}
</div>

<style>
  .map-shell { overflow: hidden; border: 1px solid rgba(23,105,224,.18); border-radius: 22px; background: var(--surface-solid); box-shadow: 0 12px 36px rgba(25,55,90,.08); }
  .map-shell.fullscreen { position: fixed; z-index: 1000; inset: 0; display: flex; flex-direction: column; padding: 14px; border: 0; border-radius: 0; background: var(--surface-solid); }
  .map-toolbar { display: flex; min-height: 58px; padding: 10px 14px; align-items: center; justify-content: space-between; gap: 14px; }
  .map-toolbar > div:first-child { display: flex; align-items: baseline; gap: 10px; }
  .map-toolbar span { color: var(--muted); font-size: 12px; }
  .map-actions { display: flex; align-items: center; gap: 4px; }
  .map-actions button, .map-actions > span { display: grid; min-width: 34px; height: 34px; padding: 0 9px; place-items: center; border: 1px solid var(--line); border-radius: 9px; color: var(--text); background: var(--surface-solid); }
  .map-actions button { cursor: pointer; } .map-actions button:disabled { opacity: .4; cursor: default; }
  .map { position: relative; height: min(48vw, 520px); min-height: 320px; overflow: hidden; touch-action: none; cursor: grab; background: #b8d8e8; user-select: none; }
  .fullscreen .map { flex: 1; height: auto; min-height: 0; border-radius: 14px; }
  .map.dragging { cursor: grabbing; }
  .tiles { position: absolute; inset: 0; }
  .tiles img { position: absolute; width: 256px; height: 256px; max-width: none; pointer-events: none; }
  .map-cluster { position: absolute; z-index: 2; display: grid; min-width: 18px; min-height: 18px; padding: 0; place-items: center; border: 2px solid white; border-radius: 50%; color: white; background: rgba(23,105,224,.9); box-shadow: 0 2px 9px rgba(0,0,0,.34); transform: translate(-50%,-50%); cursor: pointer; font-size: 10px; font-weight: 800; }
  .map-cluster.active { z-index: 3; background: #e5484d; transform: translate(-50%,-50%) scale(1.12); }
  .map-loading { position: absolute; z-index: 5; top: 12px; left: 50%; padding: 7px 11px; border-radius: 999px; color: var(--text); background: rgba(255,255,255,.9); box-shadow: 0 4px 14px rgba(0,0,0,.12); transform: translateX(-50%); font-size: 12px; }
  .attribution { position: absolute; z-index: 4; right: 4px; bottom: 3px; padding: 2px 4px; color: #24516e; background: rgba(255,255,255,.78); font-size: 10px; text-decoration: none; }
  .cluster-panel { position: absolute; z-index: 6; right: 12px; bottom: 26px; width: min(380px, calc(100% - 24px)); max-height: min(60%, 430px); padding: 12px; overflow: auto; border-radius: 16px; background: rgba(255,255,255,.96); box-shadow: 0 18px 50px rgba(0,0,0,.26); cursor: default; user-select: text; }
  .cluster-heading { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 10px; }
  .cluster-heading > div { display: flex; align-items: center; gap: 8px; }
  .cluster-heading span { color: var(--muted); font-size: 12px; }
  .close-panel { width: 30px; height: 30px; padding: 0; border: 0; border-radius: 50%; color: var(--muted); background: var(--fill-subtle); cursor: pointer; font-size: 20px; }
  .cluster-photos { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; margin-bottom: 8px; }
  .cluster-photos img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 5px; background: var(--fill-subtle); }
  .map-error { margin: 0; padding: 9px 14px; color: #9f2d2d; background: #fff0f0; font-size: 13px; }
  @media (max-width: 680px) { .map-toolbar { align-items: flex-start; } .map-toolbar > div:first-child { display: grid; gap: 2px; } .map { height: 390px; } .cluster-panel { max-height: 52%; } }
</style>
