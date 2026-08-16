<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AppleLinkCandidate, AppleSourcePage, AppleSourceSummary } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import StatusBadge from '../components/StatusBadge.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let page = $state<AppleSourcePage | null>(null);
  let candidates = $state<AppleLinkCandidate[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let filter = $state('all');
  let search = $state('');
  let error = $state<string | null>(null);
  let busyId = $state<number | null>(null);
  let reviewingId = $state<number | null>(null);

  onMount(() => { void loadWorkspace(); });

  async function loadSources() {
    const result = await api.apple.sources(filter === 'pending' ? 'all' : filter, search.trim() || undefined);
    return filter === 'pending'
      ? { ...result, sources: result.sources.filter((source) => !['ready', 'excluded', 'missing'].includes(source.sync_status)) }
      : result;
  }

  async function loadWorkspace() {
    status = 'loading';
    error = null;
    try {
      [page, candidates] = await Promise.all([
        loadSources(), api.apple.candidates(),
      ]);
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function applyFilter(next: string) {
    filter = next;
    status = 'loading';
    try {
      page = await loadSources();
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function retry(source: AppleSourceSummary) {
    busyId = source.id;
    error = null;
    try {
      const result = await api.apple.retry(source.id);
      page = page ? { ...page, sources: page.sources.map((item) => item.id === source.id ? result.source : item) } : page;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busyId = null;
    }
  }

  async function review(candidate: AppleLinkCandidate, accept: boolean) {
    reviewingId = candidate.id;
    error = null;
    try {
      await api.apple.reviewCandidate(candidate.id, accept);
      candidates = candidates.filter((item) => item.id !== candidate.id);
      page = await loadSources();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      reviewingId = null;
    }
  }

  function count(key: string) { return page?.status_counts[key] ?? 0; }
  function pendingCount() {
    if (!page) return 0;
    return Object.entries(page.status_counts)
      .filter(([key]) => !['synced', 'excluded', 'missing'].includes(key))
      .reduce((sum, [, value]) => sum + value, 0);
  }
  function statusLabel(value: string) {
    return value === 'ready' || value === 'synced' ? '已同步'
      : value === 'failed' ? '失败'
      : value === 'excluded' ? '已排除'
      : value === 'missing' ? 'Apple 中缺失'
      : value === 'queued' ? '等待同步'
      : value === 'downloading' ? '下载中'
      : value === 'downloaded' ? '已下载'
      : value === 'importing' ? '导入中'
      : '尚未同步';
  }
  function tone(value: string): 'success' | 'warning' | 'danger' | 'neutral' {
    return value === 'ready' || value === 'synced' ? 'success'
      : value === 'failed' || value === 'missing' ? 'danger'
      : ['queued', 'downloading', 'downloaded', 'importing', 'discovered'].includes(value) ? 'warning' : 'neutral';
  }
  function syncApple() { (window as unknown as { webkit?: { messageHandlers?: { picmanager?: { postMessage(value: unknown): void } } } }).webkit?.messageHandlers?.picmanager?.postMessage({ action: 'syncApple' }); }
</script>

<section aria-labelledby="apple-title">
  <div class="view-heading">
    <div><p>Apple Photos</p><h2 id="apple-title">同步清单</h2><span>逐项核对 Apple 照片图库与本地归档。</span></div>
    <form onsubmit={(event) => { event.preventDefault(); void loadWorkspace(); }}>
      <Button type="button" onclick={syncApple}>同步 iCloud 照片</Button>
      <input aria-label="搜索 Apple 照片" placeholder="文件名或 Apple 标识" bind:value={search} />
      <Button type="submit">搜索</Button>
    </form>
  </div>

  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="summary" aria-label="Apple 照片同步概览">
    <button type="button" class:active={filter === 'all'} onclick={() => applyFilter('all')}><small>全部</small><strong>{Object.values(page?.status_counts ?? {}).reduce((a, b) => a + b, 0)}</strong></button>
    <button type="button" class:active={filter === 'synced'} onclick={() => applyFilter('synced')}><small>已同步</small><strong>{count('synced')}</strong></button>
    <button type="button" class:active={filter === 'pending'} onclick={() => applyFilter('pending')}><small>尚未同步</small><strong>{pendingCount()}</strong></button>
    <button type="button" class:active={filter === 'failed'} onclick={() => applyFilter('failed')}><small>失败</small><strong>{count('failed')}</strong></button>
    <button type="button" class:active={filter === 'excluded'} onclick={() => applyFilter('excluded')}><small>已排除</small><strong>{count('excluded')}</strong></button>
    <button type="button" class:active={filter === 'missing'} onclick={() => applyFilter('missing')}><small>缺失</small><strong>{count('missing')}</strong></button>
  </div>

  {#if status === 'loading'}
    <PageState kind="loading" title="正在核对同步状态" />
  {:else if status === 'error'}
    <PageState kind="error" title="无法读取 Apple 照片清单" message={error ?? undefined} />
  {:else if page?.sources.length === 0}
    <PageState kind="empty" title="此筛选条件下没有照片" message={search ? '请尝试其他文件名或状态。' : '运行 Apple Photos inventory 后会在这里逐项显示。'} />
  {:else}
    <div class="source-list">
      {#each page?.sources ?? [] as source (source.id)}
        <article class="source-row">
          <div class="source-preview">
            {#if source.photo_id}<img src={`/api/photos/${source.photo_id}/thumb?size=256`} alt="" loading="lazy" />{:else}<span aria-hidden="true">☁</span>{/if}
          </div>
          <div class="source-copy">
            <strong>{source.original_filename ?? '未记录原始文件名'}</strong>
            <span>{source.taken_at ?? '未知拍摄时间'} · {source.media_type ?? '未知格式'}</span>
            <code title={source.external_id}>{source.external_id}</code>
          </div>
          <div class="dimensions">{source.width && source.height ? `${source.width} × ${source.height}` : '—'}</div>
          <div class="source-status"><StatusBadge label={statusLabel(source.sync_status)} tone={tone(source.sync_status)} />{#if source.last_error}<small>{source.last_error}</small>{:else if source.exclusion_reason}<small>{source.exclusion_reason}</small>{/if}</div>
          {#if source.sync_status === 'failed'}<Button disabled={busyId === source.id} onclick={() => retry(source)}>{busyId === source.id ? '重试中…' : '重试'}</Button>{/if}
        </article>
      {/each}
    </div>
  {/if}
</section>

<section class="candidates" aria-labelledby="candidate-title">
  <div class="section-heading"><div><p>Review</p><h2 id="candidate-title">待确认匹配</h2></div><span>{candidates.length} 项</span></div>
  {#if candidates.length === 0}<p class="empty-copy">没有需要人工确认的历史照片匹配。</p>
  {:else}<div class="candidate-grid">
    {#each candidates as candidate (candidate.id)}
      <article>
        <div class="candidate-images"><div><span>Apple</span><strong>{candidate.original_filename ?? '未知文件名'}</strong></div><img src={`/api/photos/${candidate.photo_id}/thumb?size=512`} alt={`候选照片 ${candidate.photo_id}`} /></div>
        <p>{candidate.method.replaceAll('_', ' ')} · 置信度 {(candidate.confidence * 100).toFixed(0)}%</p>
        <div class="candidate-actions"><Button disabled={reviewingId === candidate.id} onclick={() => review(candidate, false)}>不是同一张</Button><Button variant="primary" disabled={reviewingId === candidate.id} onclick={() => review(candidate, true)}>确认匹配</Button></div>
      </article>
    {/each}
  </div>{/if}
</section>

<style>
  section { margin-top: 34px; }
  .view-heading, .section-heading { display: flex; justify-content: space-between; align-items: center; gap: 24px; margin-bottom: 20px; }
  .view-heading p, .section-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 5px; font-size: 28px; } .view-heading span, .section-heading > span { color: var(--muted); }
  form { display: flex; gap: 8px; } input { width: 260px; min-height: 38px; padding: 0 11px; border: 1px solid var(--line); border-radius: 10px; background: white; }
  .error { padding: 10px 14px; border-radius: 10px; color: var(--danger); background: #fff0ee; }
  .summary { display: grid; grid-template-columns: repeat(6, 1fr); gap: 8px; margin-bottom: 18px; }
  .summary button { display: grid; gap: 6px; padding: 14px; border: 1px solid var(--line); border-radius: 14px; color: var(--text); text-align: left; background: white; cursor: pointer; }
  .summary button.active { border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); } .summary small { color: var(--muted); } .summary strong { font-size: 24px; }
  .source-list { display: grid; gap: 6px; }
  .source-row { display: grid; grid-template-columns: 68px minmax(220px, 1fr) auto minmax(150px, auto) auto; align-items: center; gap: 16px; padding: 10px 14px 10px 10px; border: 1px solid var(--line); border-radius: 14px; background: white; }
  .source-preview { display: grid; width: 58px; height: 58px; overflow: hidden; place-items: center; border-radius: 10px; color: var(--muted); font-size: 24px; background: var(--fill-subtle); } .source-preview img { width: 100%; height: 100%; object-fit: cover; }
  .source-copy { display: grid; min-width: 0; gap: 4px; } .source-copy strong, .source-copy span, code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .source-copy span, code, .dimensions { color: var(--muted); font-size: 11px; } code { max-width: 260px; }
  .source-status { display: grid; justify-items: start; gap: 5px; } .source-status small { max-width: 220px; color: var(--danger); font-size: 11px; }
  .candidates { padding-top: 28px; border-top: 1px solid var(--line); }
  .empty-copy { padding: 18px; border-radius: 12px; color: var(--muted); background: var(--fill-subtle); }
  .candidate-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 14px; }
  .candidate-grid article { padding: 12px; border: 1px solid var(--line); border-radius: 16px; background: white; }
  .candidate-images { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; height: 140px; } .candidate-images > div { display: grid; align-content: center; gap: 6px; min-width: 0; padding: 14px; border-radius: 10px; background: var(--fill-subtle); } .candidate-images span { color: var(--accent); font-size: 10px; font-weight: 750; } .candidate-images strong { overflow-wrap: anywhere; } .candidate-images img { width: 100%; height: 100%; object-fit: cover; border-radius: 10px; }
  .candidate-grid article > p { color: var(--muted); font-size: 12px; } .candidate-actions { display: flex; justify-content: end; gap: 7px; }
  @media (max-width: 820px) { .summary { grid-template-columns: repeat(3, 1fr); } .source-row { grid-template-columns: 58px 1fr auto; } .dimensions { display: none; } .source-status { grid-column: 2; } .view-heading { align-items: stretch; flex-direction: column; } form, input { width: 100%; } }
</style>
