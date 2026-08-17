<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient, GarminStatus } from '../api/client';
  import type { ActivityPhotos, ActivitySummary, ActivityTrack } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import { photoGridLayout, photoPageCount } from '../photos/pagination';
  import ActivityMap from './ActivityMap.svelte';

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
  let detailRequest = 0;
  let photoRequest = 0;
  let photoColumns = $state(4);
  let pageSize = $state(16);
  let syncing = $state(false);
  let mfaCode = $state('');
  let loginNotice = $state<string | null>(null);
  let garminNotice = $state<string | null>(null);
  let garminNeedsMfa = $state(false);

  onMount(() => {
    const credentialsFinished = (event: Event) => {
      const detail = (event as CustomEvent<unknown>).detail;
      if (!detail || typeof detail !== 'object' || !('outcome' in detail)) {
        garminNotice = 'Garmin 原生凭据窗口返回了无效结果。';
        return;
      }
      const result = detail as { outcome?: unknown; message?: unknown };
      const message = typeof result.message === 'string' ? result.message : null;
      if (result.outcome === 'saved') {
        garminNotice = message ?? 'Garmin 凭据已保存，本地服务已刷新；现在可以验证登录。';
        void loadGarminStatus();
      } else if (result.outcome === 'cancelled') {
        garminNotice = '已取消 Garmin 凭据编辑。';
      } else {
        garminNotice = message ?? '无法保存 Garmin 凭据。';
      }
    };
    window.addEventListener('picmanager:garmin-credentials', credentialsFinished);
    void reload(); void loadGarminStatus();
    return () => window.removeEventListener('picmanager:garmin-credentials', credentialsFinished);
  });

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
  async function syncGarmin() {
    syncing = true; error = null; garminNotice = null;
    try {
      const result = await api.activities.syncGarmin(mfaCode || undefined);
      garminNeedsMfa = result.status === 'mfa_required';
      garminNotice = result.message ?? garminMessage(result);
      if (result.status === 'completed' || result.status === 'partial_failed') {
        mfaCode = '';
        await reload();
      }
    }
    catch (reason) { garminNotice = reason instanceof Error ? reason.message : String(reason); }
    finally { syncing = false; }
  }
  async function authenticateGarmin() {
    syncing = true; error = null; garminNotice = null;
    try {
      const result = await api.activities.authenticateGarmin(mfaCode || undefined);
      garminNeedsMfa = result.status === 'mfa_required';
      garminNotice = result.message ?? garminMessage(result);
      if (result.status === 'authenticated') mfaCode = '';
    } catch (reason) { garminNotice = reason instanceof Error ? reason.message : String(reason); }
    finally { syncing = false; }
  }
  async function loadGarminStatus() {
    try {
      const result = await api.activities.garminStatus();
      if (result.status !== 'ready_to_authenticate') garminNotice = result.message ?? garminMessage(result);
    } catch (reason) {
      garminNotice = `无法读取 Garmin 状态：${reason instanceof Error ? reason.message : String(reason)}`;
    }
  }
  function garminMessage(result: GarminStatus) {
    if (result.status === 'authenticated') return 'Garmin Connect 已认证，可以开始同步。';
    if (result.status === 'completed') return `Garmin 同步完成：下载 ${result.downloaded} 条，导入 ${result.imported} 条，跳过 ${result.skipped} 条。`;
    if (result.status === 'partial_failed') return `Garmin 同步部分完成：导入 ${result.imported} 条，失败 ${result.failed} 条；失败项会保留以便重试。`;
    switch (result.error_code ?? result.status) {
      case 'mfa_required': return 'Garmin 需要一次性 MFA 验证码；输入验证码后再次验证登录。';
      case 'invalid_mfa': return 'Garmin 未接受一次性 MFA 验证码；请重新输入最新验证码。';
      case 'invalid_credentials': return 'Garmin 未接受保存的账号或密码；请重新配置凭据。';
      case 'network_error': return '无法连接 Garmin Connect；请检查网络或系统代理后重试。';
      case 'rate_limited': return 'Garmin 当前限制登录尝试；请等待后重试。';
      case 'sso_contract_error': return 'Garmin 登录协议或客户端兼容性发生变化；请导出诊断信息。';
      case 'dependency_error': return '本地 Garmin 同步组件不可用；请重新安装 PicManager。';
      case 'token_store_error': return '本地 Garmin 登录令牌无法安全保存。';
    }
    return `Garmin 状态：${result.status}`;
  }
  function configureGarmin() {
    const bridge = (window as unknown as { webkit?: { messageHandlers?: { picmanager?: { postMessage(value: unknown): void } } } }).webkit?.messageHandlers?.picmanager;
    if (!bridge) { loginNotice = 'Garmin 登录需要从 PicManager Mac 应用的内嵌窗口运行。'; return; }
    bridge.postMessage({ action: 'configureGarmin' }); loginNotice = '正在打开 Mac 原生 Garmin 登录窗口…';
  }

  async function open(activity: ActivitySummary) {
    const request = ++detailRequest;
    detailLoading = true;
    error = null;
    selected = activity;
    track = null;
    photos = null;
    photoRequest += 1;
    try {
      const [detail, activityTrack, photoPage] = await Promise.all([
        api.activities.get(activity.id), api.activities.track(activity.id), api.activities.photos(activity.id, 1, pageSize),
      ]);
      if (request === detailRequest) [selected, track, photos] = [detail, activityTrack, photoPage];
    } catch (reason) {
      if (request === detailRequest) error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }

  async function loadPhotoPage(page: number, perPage = pageSize) {
    if (!selected) return;
    const request = ++photoRequest;
    const activityId = selected.id;
    try {
      const result = await api.activities.photos(activityId, page, perPage);
      if (request === photoRequest && selected?.id === activityId) photos = result;
    } catch (reason) {
      if (request === photoRequest) error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function measurePhotos(node: HTMLElement) {
    if (typeof ResizeObserver === 'undefined') return {};
    const observer = new ResizeObserver(([entry]) => {
      const layout = photoGridLayout(entry.contentRect.width, 4, 160, 5);
      if (layout.columns === photoColumns) return;
      photoColumns = layout.columns;
      pageSize = layout.perPage;
      if (selected && photos && photos.per_page !== pageSize) void loadPhotoPage(1, pageSize);
    });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  function previousPhotoPage() { if (photos && photos.page > 1) void loadPhotoPage(photos.page - 1); }
  function nextPhotoPage() {
    if (photos && photos.page < photoPageCount(photos.total, photos.per_page)) void loadPhotoPage(photos.page + 1);
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
  function equipment(value: unknown) {
    if (!Array.isArray(value)) return [];
    return value.map((item) => {
      if (!item || typeof item !== 'object') return null;
      const record = item as Record<string, unknown>;
      const type = typeof record.sensor_type === 'string' ? record.sensor_type : 'equipment';
      const name = typeof record.name === 'string' ? record.name : typeof record.manufacturer === 'string' ? record.manufacturer : type;
      return `${type}: ${name}`;
    }).filter((item): item is string => item !== null);
  }
</script>

{#if selected}
  <section class="activity-detail" aria-labelledby="activity-title" use:measurePhotos>
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
            <ActivityMap {track} {photos} />
          {:else}<PageState kind="empty" title="这条活动没有路线数据" />{/if}
        </div>
        <dl>
          <div><dt>时间</dt><dd>{date(selected.start_time)}</dd></div>
          <div><dt>距离</dt><dd>{selected.distance_meters ? `${(selected.distance_meters / 1000).toFixed(2)} km` : '—'}</dd></div>
          <div><dt>时长</dt><dd>{duration(selected.duration_seconds)}</dd></div>
          <div><dt>累计爬升</dt><dd>{selected.elevation_gain_meters ? `${Math.round(selected.elevation_gain_meters)} m` : '—'}</dd></div>
          <div><dt>平均 / 最大心率</dt><dd>{selected.avg_heart_rate ?? '—'} / {selected.max_heart_rate ?? '—'} bpm</dd></div>
          <div><dt>设备</dt><dd>{selected.device ?? '未知'} · {selected.file_format.toUpperCase()}</dd></div>
          {#if equipment(selected.sensors).length}<div><dt>传感器 / 装备</dt><dd>{equipment(selected.sensors).join(' · ')}</dd></div>{/if}
        </dl>
      </div>
      <div class="activity-photos">
        <h3>活动中的照片 <span>{photos ? `第 ${photos.page} / ${photoPageCount(photos.total, photos.per_page)} 页 · 共 ${photos.total}` : '0'}</span></h3>
        {#if photos?.photos.length}
          <div class="photo-grid" style={`--photo-columns: ${photoColumns}`}>{#each photos.photos as photo (photo.id)}<img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`活动照片 ${photo.id}`} loading="lazy" />{/each}</div>
          {#if photoPageCount(photos.total, photos.per_page) > 1}
            <nav class="pager" aria-label="活动照片分页">
              <Button variant="ghost" disabled={photos.page <= 1} onclick={previousPhotoPage}>上一页</Button>
              <span>第 {photos.page} / {photoPageCount(photos.total, photos.per_page)} 页</span>
              <Button variant="ghost" disabled={photos.page >= photoPageCount(photos.total, photos.per_page)} onclick={nextPhotoPage}>下一页</Button>
            </nav>
          {/if}
        {:else}<p>活动时间和路线附近没有匹配的照片。</p>{/if}
      </div>
    {/if}
  </section>
{:else}
  <section aria-labelledby="activities-title">
    <div class="view-heading">
      <div><p>Motion</p><h2 id="activities-title">运动与活动</h2><span>{total} 条记录</span></div>
      <div class="activity-actions"><Button onclick={configureGarmin}>配置 Garmin</Button>{#if garminNeedsMfa}<input aria-label="Garmin MFA 验证码" placeholder="输入 MFA 验证码" bind:value={mfaCode} />{/if}<Button disabled={syncing} onclick={() => void authenticateGarmin()}>{syncing ? '正在验证…' : '验证登录'}</Button><Button disabled={syncing} onclick={() => void syncGarmin()}>{syncing ? '正在同步 Garmin…' : '同步 Garmin 数据'}</Button><label>类型<select bind:value={typeFilter} onchange={() => { void reload(); }}><option value="">全部</option><option value="running">跑步</option><option value="cycling">骑行</option><option value="walking">步行</option><option value="hiking">徒步</option><option value="swimming">游泳</option></select></label></div>
    </div>
    {#if loginNotice}<p class="login-notice" role="status">{loginNotice}</p>{/if}
    {#if garminNotice}<p class="login-notice" role="status">{garminNotice}</p>{/if}
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
  .login-notice { margin: -10px 0 16px; padding: 10px 14px; border-radius: 10px; color: var(--muted); background: var(--fill-subtle); }
  .view-heading p, .detail-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 5px; font-size: 28px; } .view-heading span { color: var(--muted); }
  label,.activity-actions { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: 13px; } select,.activity-actions input { min-height: 38px; padding: 0 10px; border: 1px solid var(--line); border-radius: 10px; background: white; }
  .activity-list { display: grid; gap: 8px; }
  .activity-card { display: grid; grid-template-columns: auto minmax(160px, 1fr) repeat(3, minmax(100px, auto)) auto; align-items: center; gap: 18px; width: 100%; padding: 14px 18px; border: 1px solid var(--line); border-radius: 16px; color: var(--text); text-align: left; background: var(--surface-solid); cursor: pointer; }
  .activity-card:hover { border-color: rgba(23,105,224,.28); box-shadow: 0 8px 24px rgba(0,0,0,.05); }
  .activity-icon { display: grid; width: 48px; height: 48px; place-items: center; border-radius: 14px; font-size: 25px; background: var(--fill-subtle); }
  .activity-copy, .stat { display: grid; gap: 4px; } .activity-copy small, .stat small { color: var(--muted); font-size: 11px; } .stat strong { font-size: 13px; }
  .arrow { color: var(--muted); font-size: 30px; }
  .detail-layout { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(250px, .7fr); gap: 20px; }
  .route-card { display: grid; min-height: 350px; overflow: hidden; place-items: center; border-radius: 20px; background: linear-gradient(145deg, #dceaf5, #eff3ea); }
  dl { display: grid; align-content: start; gap: 0; margin: 0; padding: 12px 20px; border: 1px solid var(--line); border-radius: 18px; background: white; }
  dl div { display: flex; justify-content: space-between; gap: 14px; padding: 16px 0; border-bottom: 1px solid var(--line); } dl div:last-child { border: 0; }
  dt { color: var(--muted); font-size: 12px; } dd { margin: 0; text-align: right; }
  .activity-photos { margin-top: 30px; } .activity-photos h3 { font-size: 18px; } .activity-photos h3 span, .activity-photos p { color: var(--muted); }
  .photo-grid { display: grid; grid-template-columns: repeat(var(--photo-columns), minmax(0, 1fr)); gap: 5px; } .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 5px; }
  .pager { display: flex; justify-content: center; align-items: center; gap: 12px; padding: 18px 0 6px; }
  .pager span { color: var(--muted); font-size: 12px; }
  @media (max-width: 800px) { .activity-card { grid-template-columns: auto 1fr auto; } .stat.optional { display: none; } .stat { grid-row: 2; } .arrow { grid-column: 3; grid-row: 1 / 3; } .detail-layout { grid-template-columns: 1fr; } .route-card { min-height: 260px; } }
</style>
