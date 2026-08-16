<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AlbumPhotoPage, GeoClusterPage, GeoHierarchy, GeoNamePolicy } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import PhotoMap from './PhotoMap.svelte';

  interface Props { api: ApiClient }
  interface PlaceSelection {
    label: string;
    country: string;
    state?: string;
    city?: string;
  }
  let { api }: Props = $props();
  let hierarchy = $state<GeoHierarchy | null>(null);
  let clusterPage = $state<GeoClusterPage>({ clusters: [], total_photos: 0 });
  let namePolicy = $state<GeoNamePolicy | null>(null);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selection = $state<PlaceSelection | null>(null);
  let photos = $state<AlbumPhotoPage | null>(null);
  let photosLoading = $state(false);
  let loadingMore = $state(false);
  let loadMoreError = $state<string | null>(null);
  let photoRequest = 0;
  let updating = $state(false);
  let updateMessage = $state<string | null>(null);

  onMount(() => { void reload(); });

  async function reload() {
    status = 'loading';
    try {
      [hierarchy, clusterPage, namePolicy] = await Promise.all([
        api.geo.hierarchy(), api.geo.clusters(), api.geo.namePolicy(),
      ]);
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function choose(nextSelection: PlaceSelection) {
    const request = ++photoRequest;
    selection = nextSelection;
    photos = null;
    photosLoading = true;
    loadingMore = false;
    loadMoreError = null;
    error = null;
    try {
      const { label: _label, ...filters } = nextSelection;
      const page = await api.geo.photos(filters, 1, 200);
      if (request === photoRequest) photos = page;
    } catch (reason) {
      if (request === photoRequest) error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === photoRequest) photosLoading = false;
    }
  }

  async function loadMore() {
    if (!selection || !photos || loadingMore || photos.photos.length >= photos.total) return;
    const request = photoRequest;
    const { label: _label, ...filters } = selection;
    loadingMore = true;
    loadMoreError = null;
    try {
      const next = await api.geo.photos(filters, photos.page + 1, photos.per_page);
      if (request !== photoRequest) return;
      const existing = new Set(photos.photos.map((photo) => photo.id));
      photos = { ...next, photos: [...photos.photos, ...next.photos.filter((photo) => !existing.has(photo.id))] };
    } catch (reason) {
      if (request === photoRequest) loadMoreError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === photoRequest) loadingMore = false;
    }
  }

  async function regeocode() {
    updating = true;
    updateMessage = null;
    try {
      const result = await api.geo.regeocode();
      updateMessage = result.status === 'already_running'
        ? '地点信息正在后台更新'
        : `已开始更新 ${result.count ?? 0} 个地点`;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      updating = false;
    }
  }

  async function normalizeNames() {
    updating = true;
    updateMessage = null;
    try {
      const result = await api.geo.normalizeNames();
      if (result.status === 'already_running') {
        updateMessage = '地点信息正在后台更新';
      } else if (result.status === 'up_to_date') {
        updateMessage = '已有地名已经符合当前规则';
        if (namePolicy) namePolicy.outdated_photos = 0;
      } else {
        updateMessage = `已开始统一 ${result.count ?? namePolicy?.outdated_photos ?? 0} 张照片的地名`;
      }
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      updating = false;
    }
  }

</script>

{#if status === 'loading'}
  <PageState kind="loading" title="正在整理地点" />
{:else if status === 'error'}
  <PageState kind="error" title="无法读取地点" message={error ?? undefined} />
{:else}
  <section class="map-section" aria-labelledby="map-title">
    <div class="view-heading">
      <div><p>Map</p><h2 id="map-title">照片地图</h2><span>{clusterPage.total_photos} 张照片带有位置信息</span></div>
      <div class="actions">
        {#if (namePolicy?.outdated_photos ?? 0) > 0}
          <Button disabled={updating} onclick={normalizeNames}>
            {updating ? '启动中…' : `统一已有地名（${namePolicy?.outdated_photos}）`}
          </Button>
        {/if}
        <Button disabled={updating} onclick={regeocode}>{updating ? '启动中…' : '更新地点信息'}</Button>
      </div>
    </div>
    {#if (namePolicy?.outdated_photos ?? 0) > 0}
      <p class="normalization-note">旧地名可按简体中文、其他中文、英文、当地名称的顺序在后台重新整理；网络失败不会覆盖现有名称。</p>
    {/if}
    {#if updateMessage}<p class="notice" role="status">{updateMessage}</p>{/if}
    {#if clusterPage.total_photos === 0}
      <PageState kind="empty" title="没有带位置的照片" message="保留 GPS 信息的照片会显示在这里。" />
    {:else}
      <PhotoMap {api} initialPage={clusterPage} />
    {/if}
  </section>

  <section class="browse" aria-labelledby="browse-title">
    <div class="view-heading"><div><p>Places</p><h2 id="browse-title">按地点浏览</h2></div></div>
    {#if hierarchy?.countries.length === 0}
      <PageState kind="empty" title="地点名称尚未整理" message="可以更新地点信息，把 GPS 坐标转换为国家、地区和城市。" />
    {:else}
      <div class="place-layout">
        <div class="place-list">
          {#each hierarchy?.countries ?? [] as country}
            <details class="country-group">
              <summary><strong>{country.name}</strong><span>{country.photo_count}</span></summary>
              <button class="browse-all" type="button" onclick={() => choose({ label: country.name, country: country.query_value })}>查看 {country.name} 的全部照片</button>
              <div class="states">
              {#each country.states as state}
                <details class="state-group">
                  <summary><strong>{state.name}</strong><span>{state.photo_count}</span></summary>
                  <button class="browse-all state-all" type="button" onclick={() => choose({ label: state.name, country: country.query_value, state: state.query_value })}>查看 {state.name} 的全部照片</button>
                  <div class="cities">
                    {#each state.cities as city}
                      <button
                        type="button"
                        class:active={selection?.country === country.query_value && selection?.state === state.query_value && selection?.city === city.query_value}
                        onclick={() => choose({ label: city.name, country: country.query_value, state: state.query_value, city: city.query_value })}
                      >{city.name} <span>{city.photo_count}</span></button>
                    {/each}
                  </div>
                </details>
              {/each}
              </div>
            </details>
          {/each}
        </div>
        <div class="results" aria-live="polite">
          {#if error}<PageState kind="error" title="无法读取地点照片" message={error} />
          {:else if photosLoading}<PageState kind="loading" title="正在打开地点" />
          {:else if photos?.photos.length === 0}<PageState kind="empty" title="这个地点没有照片" />
          {:else if photos}
            <div class="result-heading"><strong>{selection?.label}</strong><span>已显示 {photos.photos.length} / {photos.total} 张</span></div>
            <div class="photo-grid">{#each photos.photos as photo (photo.id)}<img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`照片 ${photo.id}`} loading="lazy" />{/each}</div>
            {#if loadMoreError}<p class="load-error" role="alert">载入下一页失败：{loadMoreError}</p>{/if}
            {#if photos.photos.length < photos.total}
              <div class="load-more"><Button disabled={loadingMore} onclick={loadMore}>{loadingMore ? '正在载入…' : '载入更多'}</Button></div>
            {/if}
          {:else}<p class="hint">选择国家、地区或城市查看照片。</p>{/if}
        </div>
      </div>
    {/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading { display: flex; justify-content: space-between; align-items: center; gap: 20px; margin-bottom: 18px; }
  .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
  .view-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 4px; font-size: 28px; } .view-heading span { color: var(--muted); font-size: 13px; }
  .notice { padding: 10px 14px; border-radius: 10px; color: #126137; background: #e8f7ee; }
  .normalization-note { margin: -8px 0 18px; color: var(--muted); font-size: 13px; }
  .browse { padding-top: 28px; border-top: 1px solid var(--line); }
  .place-layout { display: grid; grid-template-columns: minmax(230px, 340px) 1fr; gap: 24px; }
  .place-list, .results { height: min(64vh, 620px); min-height: 360px; overflow: auto; border: 1px solid var(--line); border-radius: 16px; background: var(--surface-solid); }
  .place-list { padding: 8px; overscroll-behavior: contain; }
  .country-group + .country-group { border-top: 1px solid var(--line); }
  summary { display: flex; justify-content: space-between; align-items: center; padding: 12px 10px; list-style: none; border-radius: 10px; cursor: pointer; }
  summary::-webkit-details-marker { display: none; }
  summary::before { width: 16px; content: '›'; color: var(--muted); font-size: 20px; transform-origin: center; transition: transform 120ms ease; }
  details[open] > summary::before { transform: rotate(90deg); }
  summary strong { flex: 1; }
  summary span { color: var(--muted); font-size: 11px; }
  .states { padding: 0 0 8px 12px; }
  .state-group > summary { padding-block: 9px; color: var(--muted); font-size: 13px; }
  .browse-all { width: calc(100% - 20px); margin: 0 10px 5px; padding: 7px 9px; border: 0; border-radius: 8px; color: var(--accent); text-align: left; background: var(--fill-subtle); cursor: pointer; font-size: 12px; }
  .state-all { width: calc(100% - 12px); margin-inline: 6px; }
  .cities { display: flex; flex-wrap: wrap; gap: 5px; padding: 0 8px; }
  .cities button { padding: 6px 9px; border: 0; border-radius: 999px; color: var(--muted); background: var(--fill-subtle); cursor: pointer; }
  .cities button.active { color: white; background: var(--accent); }
  button span { color: var(--muted); font-size: 11px; }
  .results { padding: 14px; }
  .result-heading { display: flex; justify-content: space-between; padding: 4px 2px 14px; } .result-heading span, .hint { color: var(--muted); }
  .hint { display: grid; height: 260px; margin: 0; place-items: center; }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 4px; }
  .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 4px; }
  .load-more { display: flex; justify-content: center; padding: 18px 0 6px; }
  .load-error { margin: 14px 0 0; color: var(--danger); text-align: center; font-size: 13px; }
  @media (max-width: 760px) { .place-layout { grid-template-columns: 1fr; } .view-heading { align-items: start; } }
</style>
