<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AlbumPhotoPage, GeoHierarchy, GeoPoint } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let hierarchy = $state<GeoHierarchy | null>(null);
  let points = $state<GeoPoint[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selection = $state<{ country: string; state?: string; city?: string } | null>(null);
  let photos = $state<AlbumPhotoPage | null>(null);
  let mapPhoto = $state<number | null>(null);
  let updating = $state(false);
  let updateMessage = $state<string | null>(null);

  onMount(() => { void reload(); });

  async function reload() {
    status = 'loading';
    try {
      [hierarchy, points] = await Promise.all([api.geo.hierarchy(), api.geo.points()]);
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function choose(country: string, state?: string, city?: string) {
    selection = { country, state, city };
    photos = null;
    error = null;
    try {
      photos = await api.geo.photos(selection);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
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

  function x(point: GeoPoint) { return ((point.gps_lon + 180) / 360) * 100; }
  function y(point: GeoPoint) { return ((90 - point.gps_lat) / 180) * 100; }
</script>

{#if status === 'loading'}
  <PageState kind="loading" title="正在整理地点" />
{:else if status === 'error'}
  <PageState kind="error" title="无法读取地点" message={error ?? undefined} />
{:else}
  <section class="map-section" aria-labelledby="map-title">
    <div class="view-heading">
      <div><p>Map</p><h2 id="map-title">照片地图</h2><span>{points.length} 张照片带有位置信息</span></div>
      <Button disabled={updating} onclick={regeocode}>{updating ? '启动中…' : '更新地点信息'}</Button>
    </div>
    {#if updateMessage}<p class="notice" role="status">{updateMessage}</p>{/if}
    {#if points.length === 0}
      <PageState kind="empty" title="没有带位置的照片" message="保留 GPS 信息的照片会显示在这里。" />
    {:else}
      <div class="map" role="img" aria-label={`包含 ${points.length} 张照片的位置概览`}>
        <div class="grid" aria-hidden="true"></div>
        {#each points as point (point.id)}
          <button
            class="map-point"
            class:active={mapPhoto === point.id}
            type="button"
            aria-label={`查看地图照片 ${point.id}`}
            style:left={`${x(point)}%`}
            style:top={`${y(point)}%`}
            onclick={() => { mapPhoto = mapPhoto === point.id ? null : point.id; }}
          ></button>
        {/each}
        {#if mapPhoto}
          <div class="map-preview"><img src={`/api/photos/${mapPhoto}/thumb?size=512`} alt={`地图照片 ${mapPhoto}`} /></div>
        {/if}
      </div>
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
            <details open>
              <summary><button type="button" onclick={() => choose(country.name)}>{country.name}<span>{country.photo_count}</span></button></summary>
              {#each country.states as state}
                <div class="state-row">
                  <button type="button" onclick={() => choose(country.name, state.name)}>{state.name}<span>{state.photo_count}</span></button>
                  <div class="cities">
                    {#each state.cities as city}
                      <button type="button" class:active={selection?.city === city.name} onclick={() => choose(country.name, state.name, city.name)}>{city.name} <span>{city.photo_count}</span></button>
                    {/each}
                  </div>
                </div>
              {/each}
            </details>
          {/each}
        </div>
        <div class="results" aria-live="polite">
          {#if error}<PageState kind="error" title="无法读取地点照片" message={error} />
          {:else if selection && !photos}<PageState kind="loading" title="正在打开地点" />
          {:else if photos?.photos.length === 0}<PageState kind="empty" title="这个地点没有照片" />
          {:else if photos}
            <div class="result-heading"><strong>{selection?.city ?? selection?.state ?? selection?.country}</strong><span>{photos.total} 张</span></div>
            <div class="photo-grid">{#each photos.photos as photo (photo.id)}<img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`照片 ${photo.id}`} loading="lazy" />{/each}</div>
          {:else}<p class="hint">选择国家、地区或城市查看照片。</p>{/if}
        </div>
      </div>
    {/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading { display: flex; justify-content: space-between; align-items: center; gap: 20px; margin-bottom: 18px; }
  .view-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 4px; font-size: 28px; } .view-heading span { color: var(--muted); font-size: 13px; }
  .notice { padding: 10px 14px; border-radius: 10px; color: #126137; background: #e8f7ee; }
  .map { position: relative; height: min(46vw, 500px); min-height: 300px; overflow: hidden; border: 1px solid rgba(23,105,224,.14); border-radius: 22px; background: linear-gradient(145deg, #dceaf5, #eef2e8 54%, #dce5ee); }
  .grid { position: absolute; inset: 0; opacity: .28; background-image: linear-gradient(rgba(42,83,116,.25) 1px, transparent 1px), linear-gradient(90deg, rgba(42,83,116,.25) 1px, transparent 1px); background-size: 12.5% 25%; }
  .map-point { position: absolute; width: 15px; height: 15px; padding: 0; border: 3px solid white; border-radius: 50%; background: var(--accent); box-shadow: 0 2px 8px rgba(0,0,0,.28); transform: translate(-50%,-50%); cursor: pointer; }
  .map-point.active { width: 22px; height: 22px; background: #e5484d; }
  .map-preview { position: absolute; right: 18px; bottom: 18px; width: 150px; padding: 7px; border-radius: 14px; background: white; box-shadow: 0 14px 40px rgba(0,0,0,.2); }
  .map-preview img { display: block; width: 100%; aspect-ratio: 4/3; object-fit: cover; border-radius: 9px; }
  .browse { padding-top: 28px; border-top: 1px solid var(--line); }
  .place-layout { display: grid; grid-template-columns: minmax(230px, 340px) 1fr; gap: 24px; }
  .place-list, .results { min-height: 320px; border: 1px solid var(--line); border-radius: 16px; background: var(--surface-solid); }
  .place-list { padding: 10px; }
  details + details { border-top: 1px solid var(--line); }
  summary { list-style: none; } summary::-webkit-details-marker { display: none; }
  summary button, .state-row > button { display: flex; justify-content: space-between; width: 100%; padding: 11px 10px; border: 0; color: var(--text); font-weight: 700; text-align: left; background: transparent; cursor: pointer; }
  .state-row { padding: 0 0 10px 10px; }
  .state-row > button { color: var(--muted); font-weight: 600; }
  .cities { display: flex; flex-wrap: wrap; gap: 5px; padding: 0 8px; }
  .cities button { padding: 6px 9px; border: 0; border-radius: 999px; color: var(--muted); background: var(--fill-subtle); cursor: pointer; }
  .cities button.active { color: white; background: var(--accent); }
  button span { color: var(--muted); font-size: 11px; }
  .results { padding: 14px; }
  .result-heading { display: flex; justify-content: space-between; padding: 4px 2px 14px; } .result-heading span, .hint { color: var(--muted); }
  .hint { display: grid; height: 260px; margin: 0; place-items: center; }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 4px; }
  .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 4px; }
  @media (max-width: 760px) { .place-layout { grid-template-columns: 1fr; } .map { height: 360px; } .view-heading { align-items: start; } }
</style>
