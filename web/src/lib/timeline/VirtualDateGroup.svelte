<script lang="ts">
  import { onMount } from 'svelte';
  import type { TimelineItem } from '../api/types';
  import { justifyRows } from './layout';

  interface Props {
    label: string;
    items: TimelineItem[];
    width: number;
    targetHeight: number;
  }

  let { label, items, width, targetHeight }: Props = $props();
  let host: HTMLElement;
  let visible = $state(true);
  let measuredHeight = $state(360);
  let rows = $derived(justifyRows(items, width, targetHeight));

  onMount(() => {
    if (typeof IntersectionObserver === 'undefined') return;
    const intersection = new IntersectionObserver(
      ([entry]) => { visible = entry.isIntersecting; },
      { rootMargin: '900px 0px' },
    );
    intersection.observe(host);
    const resize = typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver(([entry]) => {
          if (visible && entry.contentRect.height > 0) measuredHeight = entry.contentRect.height;
        });
    resize?.observe(host);
    return () => {
      intersection.disconnect();
      resize?.disconnect();
    };
  });
</script>

<section
  class="date-group"
  bind:this={host}
  aria-label={label}
  style:min-height={visible ? undefined : `${measuredHeight}px`}
>
  {#if visible}
    <h2>{label}</h2>
    <div class="rows">
      {#each rows as row}
        <div class="photo-row" style:height={`${row.height}px`}>
          {#each row.cells as cell (cell.item.id)}
            <button
              class="photo-cell"
              style:width={`${cell.width}px`}
              type="button"
              aria-label={`打开照片 ${cell.item.id}`}
            >
              <img
                src={cell.item.preview.src}
                srcset={cell.item.preview.srcset}
                sizes={`${Math.ceil(cell.width)}px`}
                alt=""
                loading="lazy"
                decoding="async"
              />
              {#if cell.item.has_current}<span class="edited">已编辑</span>{/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .date-group { position: relative; contain: layout style; }
  h2 { position: sticky; top: 64px; z-index: 4; width: max-content; margin: 0; padding: 18px 10px 10px 0; color: var(--text); font-size: 15px; background: linear-gradient(90deg, var(--page) 78%, transparent); }
  .rows { display: grid; gap: 4px; }
  .photo-row { display: flex; gap: 4px; overflow: hidden; }
  .photo-cell { position: relative; flex: 0 0 auto; height: 100%; padding: 0; overflow: hidden; border: 0; border-radius: 5px; background: var(--fill-subtle); cursor: pointer; }
  .photo-cell:focus-visible { z-index: 2; outline: 3px solid var(--focus); outline-offset: -3px; }
  img { width: 100%; height: 100%; object-fit: cover; transition: transform 180ms ease, filter 180ms ease; }
  .photo-cell:hover img { transform: scale(1.018); filter: brightness(0.96); }
  .edited { position: absolute; right: 7px; bottom: 7px; padding: 3px 7px; border-radius: 999px; color: white; font-size: 10px; font-weight: 700; background: rgba(0, 0, 0, 0.58); backdrop-filter: blur(8px); }
  @media (prefers-reduced-motion: reduce) { img { transition: none; } }
</style>
