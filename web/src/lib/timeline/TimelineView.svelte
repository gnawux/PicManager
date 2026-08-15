<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import PhotoViewer from './PhotoViewer.svelte';
  import SelectionBar from './SelectionBar.svelte';
  import { groupTimelineItems } from './layout';
  import { createTimelineStore } from './store';
  import VirtualDateGroup from './VirtualDateGroup.svelte';

  interface Props { api: ApiClient }
  let { api: client }: Props = $props();
  const timeline = createTimelineStore({
    timeline: { page: (cursor?: string) => client.timeline.page(cursor) },
  } as ApiClient);
  let host: HTMLElement;
  let width = $state(1200);
  let density = $state(190);
  let selected = $state(new Set<number>());
  let anchorId = $state<number | null>(null);
  let batchBusy = $state(false);
  let batchError = $state<string | null>(null);
  let viewerId = $state<number | null>(null);
  let groups = $derived(groupTimelineItems($timeline.items));
  let viewerIndex = $derived(viewerId === null
    ? -1
    : $timeline.items.findIndex((item) => item.id === viewerId));
  let viewerItem = $derived(viewerIndex >= 0 ? $timeline.items[viewerIndex] : null);

  onMount(() => {
    void timeline.reload();
    const updateWidth = () => { width = Math.max(280, host.clientWidth); };
    updateWidth();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(updateWidth);
    observer.observe(host);
    return () => observer.disconnect();
  });

  function maybeLoadMore() {
    const remaining = document.documentElement.scrollHeight - (window.scrollY + window.innerHeight);
    if (remaining < 1200 && $timeline.hasMore && $timeline.status !== 'loading') {
      void timeline.loadMore();
    }
  }

  function toggleSelection(itemId: number, event: { shiftKey: boolean }) {
    const next = new Set(selected);
    const ids = $timeline.items.map((item) => item.id);
    if (event.shiftKey && anchorId !== null) {
      const start = ids.indexOf(anchorId);
      const end = ids.indexOf(itemId);
      if (start >= 0 && end >= 0) {
        for (const id of ids.slice(Math.min(start, end), Math.max(start, end) + 1)) next.add(id);
      }
    } else if (next.has(itemId)) {
      next.delete(itemId);
    } else {
      next.add(itemId);
    }
    anchorId = itemId;
    selected = next;
    batchError = null;
  }

  function handlePhotoKey(itemId: number, event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      viewerId = itemId;
      return;
    }
    if (event.key === ' ') {
      event.preventDefault();
      toggleSelection(itemId, event);
      return;
    }
    if (event.key === 'Escape') {
      selected = new Set();
      return;
    }
    const direction = event.key === 'ArrowRight' || event.key === 'ArrowDown'
      ? 1
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
        ? -1
        : 0;
    if (!direction) return;
    event.preventDefault();
    const buttons = [...host.querySelectorAll<HTMLButtonElement>('[data-photo-id]')];
    const index = buttons.findIndex((button) => Number(button.dataset.photoId) === itemId);
    buttons[index + direction]?.focus();
  }

  function openPhoto(itemId: number, event: MouseEvent) {
    if (event.shiftKey || event.metaKey || event.ctrlKey) {
      toggleSelection(itemId, event);
    } else {
      viewerId = itemId;
    }
  }

  async function navigateViewer(direction: -1 | 1) {
    const index = viewerIndex;
    const target = $timeline.items[index + direction];
    if (target) {
      viewerId = target.id;
      return;
    }
    if (direction === 1 && $timeline.hasMore) {
      await timeline.loadMore();
      viewerId = $timeline.items[index + 1]?.id ?? viewerId;
    }
  }

  function selectAllLoaded() {
    selected = new Set($timeline.items.map((item) => item.id));
    anchorId = $timeline.items.at(-1)?.id ?? null;
  }

  async function runBatch(action: 'rotate_left' | 'rotate_right' | 'flip_h' | 'flip_v') {
    if (selected.size === 0 || batchBusy) return;
    batchBusy = true;
    batchError = null;
    const update = action === 'rotate_left' ? { rotation_delta: -90 }
      : action === 'rotate_right' ? { rotation_delta: 90 }
      : action === 'flip_h' ? { flip_h_toggle: true }
      : { flip_v_toggle: true };
    try {
      await client.photos.batchUpdate([...selected], update);
      selected = new Set();
      anchorId = null;
      await timeline.reload();
    } catch (error) {
      batchError = error instanceof Error ? error.message : String(error);
    } finally {
      batchBusy = false;
    }
  }
</script>

<svelte:window onscroll={maybeLoadMore} />

<div class="timeline-controls">
  <span>{$timeline.items.length.toLocaleString()} 张已载入</span>
  <label>
    <span>密度</span>
    <input aria-label="照片密度" type="range" min="120" max="280" step="10" bind:value={density} />
  </label>
  <div class="selection-actions">
    {#if selected.size > 0}
      <button type="button" onclick={() => { selected = new Set(); }}>清空选择</button>
    {:else}
      <button type="button" onclick={selectAllLoaded} disabled={$timeline.items.length === 0}>全选已载入</button>
    {/if}
  </div>
</div>

<div class="timeline" bind:this={host} aria-busy={$timeline.status === 'loading'}>
  {#if $timeline.status === 'loading' && $timeline.items.length === 0}
    <PageState kind="loading" title="正在整理时间线" message="首批照片很快就会出现。" />
  {:else if $timeline.status === 'error' && $timeline.items.length === 0}
    <PageState kind="error" title="暂时无法读取时间线" message={$timeline.error?.message} />
  {:else if $timeline.status === 'ready' && $timeline.items.length === 0}
    <PageState kind="empty" title="图库还是空的" message="完成一次导入后，照片会按日期出现在这里。" />
  {:else}
    {#each groups as group (group.key)}
      <VirtualDateGroup
        label={group.label}
        items={group.items}
        {width}
        targetHeight={density}
        {selected}
        onopen={(item, event) => openPhoto(item.id, event)}
        onselect={(item, event) => toggleSelection(item.id, event)}
        onkey={(item, event) => handlePhotoKey(item.id, event)}
      />
    {/each}

    {#if $timeline.hasMore}
      <div class="load-more">
        <Button disabled={$timeline.status === 'loading'} onclick={() => timeline.loadMore()}>
          {$timeline.status === 'loading' ? '正在载入…' : '载入更多'}
        </Button>
      </div>
    {:else if $timeline.items.length > 0}
      <p class="end-marker">已到达图库开端</p>
    {/if}
    {#if $timeline.status === 'error' && $timeline.items.length > 0}
      <div class="retry" role="alert">
        <span>{$timeline.error?.message}</span>
        <Button variant="ghost" onclick={() => timeline.loadMore()}>重试</Button>
      </div>
    {/if}
  {/if}
</div>

{#if selected.size > 0}
  <SelectionBar
    count={selected.size}
    busy={batchBusy}
    error={batchError}
    onaction={runBatch}
    onclear={() => { selected = new Set(); }}
  />
{/if}

{#if viewerItem}
  <PhotoViewer
    api={client}
    item={viewerItem}
    previous={$timeline.items[viewerIndex - 1]}
    next={$timeline.items[viewerIndex + 1]}
    onclose={() => { viewerId = null; }}
    onnavigate={(direction) => { void navigateViewer(direction); }}
  />
{/if}

<style>
  .timeline-controls { position: sticky; top: 64px; z-index: 6; display: flex; justify-content: space-between; align-items: center; min-height: 48px; border-bottom: 1px solid var(--line); color: var(--muted); font-size: 13px; background: color-mix(in srgb, var(--page) 88%, transparent); backdrop-filter: blur(18px); }
  label { display: flex; align-items: center; gap: 9px; }
  input { width: 112px; accent-color: var(--accent); }
  .selection-actions button { padding: 6px 10px; border: 0; border-radius: 8px; color: var(--accent); background: transparent; cursor: pointer; }
  .selection-actions button:disabled { color: var(--muted); cursor: default; }
  .timeline { width: 100%; min-height: 50vh; }
  .load-more { display: grid; place-items: center; padding: 32px; }
  .end-marker { margin: 0; padding: 36px; color: var(--muted); text-align: center; }
  .retry { display: flex; justify-content: center; align-items: center; gap: 12px; padding: 20px; color: var(--danger); }
</style>
