<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';
  import { groupTimelineItems } from './layout';
  import { createTimelineStore } from './store';
  import VirtualDateGroup from './VirtualDateGroup.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  const timeline = createTimelineStore({
    timeline: { page: (cursor?: string) => api.timeline.page(cursor) },
  } as ApiClient);
  let host: HTMLElement;
  let width = $state(1200);
  let density = $state(190);
  let groups = $derived(groupTimelineItems($timeline.items));

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
</script>

<svelte:window onscroll={maybeLoadMore} />

<div class="timeline-controls">
  <span>{$timeline.items.length.toLocaleString()} 张已载入</span>
  <label>
    <span>密度</span>
    <input aria-label="照片密度" type="range" min="120" max="280" step="10" bind:value={density} />
  </label>
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

<style>
  .timeline-controls { position: sticky; top: 64px; z-index: 6; display: flex; justify-content: space-between; align-items: center; min-height: 48px; border-bottom: 1px solid var(--line); color: var(--muted); font-size: 13px; background: color-mix(in srgb, var(--page) 88%, transparent); backdrop-filter: blur(18px); }
  label { display: flex; align-items: center; gap: 9px; }
  input { width: 112px; accent-color: var(--accent); }
  .timeline { width: 100%; min-height: 50vh; }
  .load-more { display: grid; place-items: center; padding: 32px; }
  .end-marker { margin: 0; padding: 36px; color: var(--muted); text-align: center; }
  .retry { display: flex; justify-content: center; align-items: center; gap: 12px; padding: 20px; color: var(--danger); }
</style>
