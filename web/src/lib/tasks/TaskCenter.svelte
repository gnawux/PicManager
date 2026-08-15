<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { DedupGroup, TaskDetail, TaskSummary } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import StatusBadge from '../components/StatusBadge.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let tasks = $state<TaskSummary[]>([]);
  let groups = $state<DedupGroup[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let expanded = $state<TaskDetail | null>(null);
  let busyTask = $state<number | null>(null);
  let keeps = $state<Record<number, Set<number>>>({});
  let confirmGroup = $state<number | null>(null);
  let resolvingGroup = $state<number | null>(null);

  onMount(() => { void reload(); });

  async function reload() {
    status = 'loading';
    error = null;
    try {
      const [taskPage, dedupGroups] = await Promise.all([api.tasks.list(), api.dedup.list()]);
      tasks = taskPage.tasks;
      groups = dedupGroups;
      keeps = Object.fromEntries(groups.map((group) => [
        group.group_id,
        new Set(group.members.filter((member) => member.keep).map((member) => member.photo_id)),
      ]));
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function showDetail(task: TaskSummary) {
    error = null;
    try { expanded = await api.tasks.get(task.id); }
    catch (reason) { error = reason instanceof Error ? reason.message : String(reason); }
  }

  async function taskAction(task: TaskSummary, action: 'retry' | 'cancel') {
    busyTask = task.id;
    error = null;
    try {
      const updated = action === 'retry' ? await api.tasks.retry(task.id) : await api.tasks.cancel(task.id);
      tasks = tasks.map((item) => item.id === task.id ? updated : item);
      if (expanded?.id === task.id) expanded = updated;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally { busyTask = null; }
  }

  function toggleKeep(groupId: number, photoId: number) {
    const next = new Set(keeps[groupId] ?? []);
    next.has(photoId) ? next.delete(photoId) : next.add(photoId);
    keeps = { ...keeps, [groupId]: next };
    confirmGroup = null;
  }

  async function resolve(group: DedupGroup) {
    const selected = [...(keeps[group.group_id] ?? [])];
    if (selected.length === 0) return;
    if (confirmGroup !== group.group_id) {
      confirmGroup = group.group_id;
      return;
    }
    resolvingGroup = group.group_id;
    error = null;
    try {
      await api.dedup.resolve(group.group_id, selected);
      groups = groups.filter((item) => item.group_id !== group.group_id);
      confirmGroup = null;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally { resolvingGroup = null; }
  }

  function progress(task: TaskSummary) {
    return task.total_items > 0 ? Math.round(((task.completed_items + task.failed_items) / task.total_items) * 100) : 0;
  }
  function statusLabel(value: string) {
    return value === 'completed' ? '已完成' : value === 'failed' ? '失败' : value === 'running' ? '进行中' : value === 'queued' ? '等待中' : value === 'cancelled' ? '已取消' : value;
  }
  function tone(value: string): 'success' | 'warning' | 'danger' | 'neutral' {
    return value === 'completed' ? 'success' : value === 'failed' ? 'danger' : ['running', 'queued'].includes(value) ? 'warning' : 'neutral';
  }
</script>

{#if status === 'loading'}
  <PageState kind="loading" title="正在汇总任务" />
{:else if status === 'error'}
  <PageState kind="error" title="无法读取任务中心" message={error ?? undefined} />
{:else}
  <section aria-labelledby="task-title">
    <div class="view-heading"><div><p>Background work</p><h2 id="task-title">后台任务</h2><span>同步、导入和处理进度集中在这里。</span></div><Button onclick={reload}>刷新</Button></div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if tasks.length === 0}<PageState kind="empty" title="目前没有后台任务" />
    {:else}<div class="task-list">
      {#each tasks as task (task.id)}
        <article class="task-card">
          <button class="task-main" type="button" onclick={() => showDetail(task)}>
            <span class="task-icon" aria-hidden="true">{task.provider === 'apple_photos' ? '☁' : '↻'}</span>
            <span class="task-copy"><strong>{task.kind.replaceAll('_', ' ')}</strong><small>{task.provider?.replaceAll('_', ' ') ?? '本地'} · {task.created_at}</small></span>
            <span class="progress-copy">{task.completed_items}/{task.total_items}{#if task.failed_items}<em>{task.failed_items} 失败</em>{/if}</span>
            <StatusBadge label={statusLabel(task.status)} tone={tone(task.status)} />
          </button>
          <div class="progress"><span style:width={`${progress(task)}%`}></span></div>
          <div class="task-actions">
            {#if task.status === 'failed'}<Button disabled={busyTask === task.id} onclick={() => taskAction(task, 'retry')}>重试失败项</Button>{/if}
            {#if ['queued', 'running', 'paused', 'failed'].includes(task.status)}<Button variant="ghost" disabled={busyTask === task.id} onclick={() => taskAction(task, 'cancel')}>取消</Button>{/if}
          </div>
          {#if expanded?.id === task.id}
            <div class="task-detail">
              {#if expanded.items.length === 0}<p>这个任务没有明细项。</p>{:else}
                {#each expanded.items as item}<div><code>{item.external_id}</code><span>{item.operation}</span><StatusBadge label={item.status} tone={item.status === 'failed' ? 'danger' : item.status === 'succeeded' ? 'success' : 'neutral'} />{#if item.last_error}<small>{item.last_error}</small>{/if}</div>{/each}
              {/if}
            </div>
          {/if}
        </article>
      {/each}
    </div>{/if}
  </section>

  <section class="dedup" aria-labelledby="dedup-title">
    <div class="view-heading"><div><p>Review</p><h2 id="dedup-title">重复照片</h2><span>选择要保留的版本；其他项只从图库隐藏，不立即删除文件。</span></div><strong>{groups.length} 组</strong></div>
    {#if groups.length === 0}<p class="empty-copy">没有待确认的重复照片组。</p>
    {:else}<div class="group-list">
      {#each groups as group (group.group_id)}
        <article class="dedup-group">
          <div class="group-heading"><strong>重复组 #{group.group_id}</strong><span>选择一张或多张保留</span></div>
          <div class="members">
            {#each group.members as member (member.photo_id)}
              <button class:selected={keeps[group.group_id]?.has(member.photo_id)} type="button" aria-pressed={keeps[group.group_id]?.has(member.photo_id)} onclick={() => toggleKeep(group.group_id, member.photo_id)}>
                <img src={`/api/photos/${member.photo_id}/thumb?size=512`} alt="" loading="lazy" />
                <span class="keep-mark">{keeps[group.group_id]?.has(member.photo_id) ? '✓ 保留' : '选择保留'}</span>
                <strong title={member.path}>{member.filename}</strong>
                <small>{member.width && member.height ? `${member.width} × ${member.height}` : '未知尺寸'}{member.camera ? ` · ${member.camera}` : ''}</small>
              </button>
            {/each}
          </div>
          <div class="resolve-actions">
            {#if confirmGroup === group.group_id}<span role="alert">将隐藏未选中的照片；原文件不会立即删除。</span>{/if}
            <Button variant={confirmGroup === group.group_id ? 'primary' : 'secondary'} disabled={!keeps[group.group_id]?.size || resolvingGroup === group.group_id} onclick={() => resolve(group)}>{resolvingGroup === group.group_id ? '处理中…' : confirmGroup === group.group_id ? '确认保留并隐藏其余项' : '确认选择'}</Button>
          </div>
        </article>
      {/each}
    </div>{/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading { display: flex; justify-content: space-between; align-items: center; gap: 24px; margin-bottom: 20px; }
  .view-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; } h2 { margin: 0 0 5px; font-size: 28px; } .view-heading span, .view-heading > strong { color: var(--muted); }
  .error { padding: 10px 14px; border-radius: 10px; color: var(--danger); background: #fff0ee; }
  .task-list, .group-list { display: grid; gap: 9px; }
  .task-card, .dedup-group { overflow: hidden; border: 1px solid var(--line); border-radius: 16px; background: white; }
  .task-main { display: grid; grid-template-columns: auto 1fr auto auto; align-items: center; gap: 14px; width: 100%; padding: 14px 16px; border: 0; color: var(--text); text-align: left; background: transparent; cursor: pointer; }
  .task-icon { display: grid; width: 42px; height: 42px; place-items: center; border-radius: 12px; font-size: 22px; background: var(--fill-subtle); }
  .task-copy { display: grid; gap: 4px; } .task-copy small, .progress-copy { color: var(--muted); font-size: 11px; } .progress-copy { display: grid; text-align: right; } .progress-copy em { color: var(--danger); font-style: normal; }
  .progress { height: 3px; background: var(--fill-subtle); } .progress span { display: block; height: 100%; background: var(--accent); }
  .task-actions { display: flex; justify-content: end; gap: 6px; padding: 8px 12px; border-top: 1px solid var(--line); }
  .task-detail { display: grid; padding: 8px 16px 14px; background: var(--fill-subtle); } .task-detail > div { display: grid; grid-template-columns: 1fr auto auto; align-items: center; gap: 10px; padding: 8px 0; border-bottom: 1px solid var(--line); } .task-detail small { grid-column: 1 / -1; color: var(--danger); }
  .dedup { padding-top: 28px; border-top: 1px solid var(--line); }
  .empty-copy { padding: 18px; border-radius: 12px; color: var(--muted); background: var(--fill-subtle); }
  .group-heading { display: flex; justify-content: space-between; padding: 14px 16px; } .group-heading span { color: var(--muted); font-size: 12px; }
  .members { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 8px; padding: 0 12px 12px; }
  .members button { position: relative; display: grid; gap: 4px; min-width: 0; padding: 7px 7px 10px; border: 2px solid transparent; border-radius: 12px; color: var(--text); text-align: left; background: var(--fill-subtle); cursor: pointer; } .members button.selected { border-color: var(--accent); background: #edf5ff; }
  .members img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 8px; } .members strong, .members small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .members small { color: var(--muted); font-size: 11px; }
  .keep-mark { position: absolute; top: 14px; right: 14px; padding: 4px 7px; border-radius: 999px; color: white; font-size: 10px; font-weight: 750; background: rgba(0,0,0,.6); }
  .members button.selected .keep-mark { background: var(--accent); }
  .resolve-actions { display: flex; justify-content: end; align-items: center; gap: 12px; padding: 10px 12px; border-top: 1px solid var(--line); } .resolve-actions span { color: var(--danger); font-size: 12px; }
  @media (max-width: 720px) { .task-main { grid-template-columns: auto 1fr auto; } .progress-copy { grid-column: 2; text-align: left; } .view-heading { align-items: start; } .resolve-actions { align-items: stretch; flex-direction: column; } }
</style>
