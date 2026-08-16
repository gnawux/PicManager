<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { ActivityPhotos, ActivitySummary, ActivityTrack } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let activities = $state<ActivitySummary[]>([]);
  let total = $state(0);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let typeFilter = $state('');
  let selected = $state<ActivitySummary | null>(null);
  let track = $state<ActivityTrack | null>(null);
  let photos = $state<ActivityPhotos | null>(null);
  let detailLoading = $state(false);
  let loadingMore = $state(false);
  let loadMoreError = $state<string | null>(null);
  let detailRequest = 0;

  onMount(() => { void reload(); });

  async function reload() {
    status = 'loading';
    error = null;
    try {
      const page = await api.activities.list(typeFilter || undefined);
      activities = page.activities;
      total = page.total;
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function open(activity: ActivitySummary) {
    const request = ++detailRequest;
    detailLoading = true;
    error = null;
    selected = activity;
    track = null;
    photos = null;
    loadingMore = false;
    loadMoreError = null;
    try {
      const [detail, activityTrack, photoPage] = await Promise.all([
        api.activities.get(activity.id), api.activities.track(activity.id), api.activities.photos(activity.id, 1, 100),
      ]);
      if (request === detailRequest) [selected, track, photos] = [detail, activityTrack, photoPage];
    } catch (reason) {
      if (request === detailRequest) error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }

  async function loadMorePhotos() {
    if (!selected || !photos || loadingMore || photos.photos.length >= photos.total) return;
    const request = detailRequest;
    loadingMore = true;
    loadMoreError = null;
    try {
      const next = await api.activities.photos(selected.id, photos.page + 1, photos.per_page);
      if (request !== detailRequest) return;
      const existing = new Set(photos.photos.map((photo) => photo.id));
      photos = { ...next, photos: [...photos.photos, ...next.photos.filter((photo) => !existing.has(photo.id))] };
    } catch (reason) {
      if (request === detailRequest) loadMoreError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === detailRequest) loadingMore = false;
    }
  }

  function icon(type: string) {
    return type === 'running' ? '🏃' : type === 'cycling' ? '🚴' : type === 'walking' || type === 'hiking' ? '🥾' : type === 'swimming' ? '🏊' : '🏅';
  }
  function typeName(type: string) {
    return type === 'running' ? '跑步' : type === 'cycling' ? '骑行' : type === 'walking' ? '步行' : type === 'hiking' ? '徒步' : type === 'swimming' ? '游泳' : type;
  }
  function duration(seconds: number | null) {
    if (!seconds) return '—';
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    return hours ? `${hours} 小时 ${minutes} 分` : `${minutes} 分钟`;
  }
  function date(value: string | null) {
    if (!value) return '未知时间';
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? value : new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' }).format(parsed);
  }
  function routePoints(activityTrack: ActivityTrack | null) {
    const values = activityTrack?.points ?? [];
    if (values.length < 2) return '';
    const lats = values.map((point) => point.lat);
    const lons = values.map((point) => point.lon);
    const minLat = Math.min(...lats), maxLat = Math.max(...lats);
    const minLon = Math.min(...lons), maxLon = Math.max(...lons);
    const latSpan = maxLat - minLat || 1;
    const lonSpan = maxLon - minLon || 1;
    return values.map((point) => `${40 + ((point.lon - minLon) / lonSpan) * 920},${380 - ((point.lat - minLat) / latSpan) * 340}`).join(' ');
  }
</script>

{#if selected}
  <section class="activity-detail" aria-labelledby="activity-title">
    <div class="detail-heading">
      <Button variant="ghost" onclick={() => { detailRequest += 1; selected = null; track = null; photos = null; }}>← 返回活动</Button>
      <div><p>{typeName(selected.activity_type)}</p><h2 id="activity-title">{selected.title ?? `${typeName(selected.activity_type)} · ${date(selected.start_time)}`}</h2></div>
    </div>
    {#if detailLoading}
      <PageState kind="loading" title="正在打开活动" />
    {:else if error}
      <PageState kind="error" title="无法读取活动" message={error} />
    {:else}
      <div class="detail-layout">
        <div class="route-card">
          {#if track && track.points.length > 1}
            <svg viewBox="0 0 1000 420" role="img" aria-label={`包含 ${track.original_count} 个轨迹点的路线`}>
              <defs><linearGradient id="route" x1="0" x2="1"><stop stop-color="#2e7de9"/><stop offset="1" stop-color="#8b5cf6"/></linearGradient></defs>
              <path d="M0 80 C180 20 250 150 410 100 S720 40 1000 120 V420 H0Z" fill="#dce9e1" opacity=".7" />
              <polyline points={routePoints(track)} fill="none" stroke="url(#route)" stroke-width="9" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          {:else}<PageState kind="empty" title="这条活动没有路线数据" />{/if}
        </div>
        <dl>
          <div><dt>时间</dt><dd>{date(selected.start_time)}</dd></div>
          <div><dt>距离</dt><dd>{selected.distance_meters ? `${(selected.distance_meters / 1000).toFixed(2)} km` : '—'}</dd></div>
          <div><dt>时长</dt><dd>{duration(selected.duration_seconds)}</dd></div>
          <div><dt>累计爬升</dt><dd>{selected.elevation_gain_meters ? `${Math.round(selected.elevation_gain_meters)} m` : '—'}</dd></div>
          <div><dt>平均 / 最大心率</dt><dd>{selected.avg_heart_rate ?? '—'} / {selected.max_heart_rate ?? '—'} bpm</dd></div>
          <div><dt>设备</dt><dd>{selected.device ?? '未知'} · {selected.file_format.toUpperCase()}</dd></div>
        </dl>
      </div>
      <div class="activity-photos">
        <h3>活动中的照片 <span>{photos ? `已显示 ${photos.photos.length} / ${photos.total}` : '0'}</span></h3>
        {#if photos?.photos.length}
          <div class="photo-grid">{#each photos.photos as photo (photo.id)}<img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`活动照片 ${photo.id}`} loading="lazy" />{/each}</div>
          {#if loadMoreError}<p class="load-error" role="alert">载入下一页失败：{loadMoreError}</p>{/if}
          {#if photos.photos.length < photos.total}
            <div class="load-more"><Button disabled={loadingMore} onclick={loadMorePhotos}>{loadingMore ? '正在载入…' : '载入更多'}</Button></div>
          {/if}
        {:else}<p>活动时间和路线附近没有匹配的照片。</p>{/if}
      </div>
    {/if}
  </section>
{:else}
  <section aria-labelledby="activities-title">
    <div class="view-heading">
      <div><p>Motion</p><h2 id="activities-title">运动与活动</h2><span>{total} 条记录</span></div>
      <label>类型<select bind:value={typeFilter} onchange={() => { void reload(); }}><option value="">全部</option><option value="running">跑步</option><option value="cycling">骑行</option><option value="walking">步行</option><option value="hiking">徒步</option><option value="swimming">游泳</option></select></label>
    </div>
    {#if status === 'loading'}<PageState kind="loading" title="正在载入活动" />
    {:else if status === 'error'}<PageState kind="error" title="无法读取活动" message={error ?? undefined} />
    {:else if activities.length === 0}<PageState kind="empty" title="还没有活动记录" message="可通过命令行导入 GPX 或 FIT 文件。" />
    {:else}
      <div class="activity-list">
        {#each activities as activity (activity.id)}
          <button class="activity-card" type="button" onclick={() => open(activity)}>
            <span class="activity-icon" aria-hidden="true">{icon(activity.activity_type)}</span>
            <span class="activity-copy"><strong>{activity.title ?? typeName(activity.activity_type)}</strong><small>{date(activity.start_time)}</small></span>
            <span class="stat"><small>距离</small><strong>{activity.distance_meters ? `${(activity.distance_meters / 1000).toFixed(2)} km` : '—'}</strong></span>
            <span class="stat"><small>时长</small><strong>{duration(activity.duration_seconds)}</strong></span>
            <span class="stat optional"><small>心率</small><strong>{activity.avg_heart_rate ? `${activity.avg_heart_rate} bpm` : '—'}</strong></span>
            <span class="arrow">›</span>
          </button>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading, .detail-heading { display: flex; justify-content: space-between; align-items: center; gap: 24px; margin-bottom: 22px; }
  .view-heading p, .detail-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 5px; font-size: 28px; } .view-heading span { color: var(--muted); }
  label { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: 13px; } select { min-height: 38px; padding: 0 10px; border: 1px solid var(--line); border-radius: 10px; background: white; }
  .activity-list { display: grid; gap: 8px; }
  .activity-card { display: grid; grid-template-columns: auto minmax(160px, 1fr) repeat(3, minmax(100px, auto)) auto; align-items: center; gap: 18px; width: 100%; padding: 14px 18px; border: 1px solid var(--line); border-radius: 16px; color: var(--text); text-align: left; background: var(--surface-solid); cursor: pointer; }
  .activity-card:hover { border-color: rgba(23,105,224,.28); box-shadow: 0 8px 24px rgba(0,0,0,.05); }
  .activity-icon { display: grid; width: 48px; height: 48px; place-items: center; border-radius: 14px; font-size: 25px; background: var(--fill-subtle); }
  .activity-copy, .stat { display: grid; gap: 4px; } .activity-copy small, .stat small { color: var(--muted); font-size: 11px; } .stat strong { font-size: 13px; }
  .arrow { color: var(--muted); font-size: 30px; }
  .detail-layout { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(250px, .7fr); gap: 20px; }
  .route-card { display: grid; min-height: 350px; overflow: hidden; place-items: center; border-radius: 20px; background: linear-gradient(145deg, #dceaf5, #eff3ea); }
  svg { width: 100%; height: 100%; }
  dl { display: grid; align-content: start; gap: 0; margin: 0; padding: 12px 20px; border: 1px solid var(--line); border-radius: 18px; background: white; }
  dl div { display: flex; justify-content: space-between; gap: 14px; padding: 16px 0; border-bottom: 1px solid var(--line); } dl div:last-child { border: 0; }
  dt { color: var(--muted); font-size: 12px; } dd { margin: 0; text-align: right; }
  .activity-photos { margin-top: 30px; } .activity-photos h3 { font-size: 18px; } .activity-photos h3 span, .activity-photos p { color: var(--muted); }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 5px; } .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 5px; }
  .load-more { display: flex; justify-content: center; padding: 18px 0 6px; }
  .load-error { margin: 14px 0 0; color: var(--danger); text-align: center; font-size: 13px; }
  @media (max-width: 800px) { .activity-card { grid-template-columns: auto 1fr auto; } .stat.optional { display: none; } .stat { grid-row: 2; } .arrow { grid-column: 3; grid-row: 1 / 3; } .detail-layout { grid-template-columns: 1fr; } .route-card { min-height: 260px; } }
</style>
