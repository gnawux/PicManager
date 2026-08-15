<script lang="ts">
  import { onMount } from 'svelte';
  import type { TimelineItem } from '../api/types';
  import { justifyRows } from './layout';

  interface Props {
    label: string;
    items: TimelineItem[];
    width: number;
    targetHeight: number;
    selected: Set<number>;
    onselect: (item: TimelineItem, event: MouseEvent) => void;
    onkey: (item: TimelineItem, event: KeyboardEvent) => void;
  }

  let { label, items, width, targetHeight, selected, onselect, onkey }: Props = $props();
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
              class:selected={selected.has(cell.item.id)}
              style:width={`${cell.width}px`}
              type="button"
              aria-label={`打开照片 ${cell.item.id}`}
              aria-pressed={selected.has(cell.item.id)}
              data-photo-id={cell.item.id}
              onclick={(event) => onselect(cell.item, event)}
              onkeydown={(event) => onkey(cell.item, event)}
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
              {#if selected.has(cell.item.id)}<span class="check" aria-hidden="true">✓</span>{/if}
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
  .photo-cell.selected { box-shadow: inset 0 0 0 4px white, inset 0 0 0 7px var(--accent); }
  img { width: 100%; height: 100%; object-fit: cover; transition: transform 180ms ease, filter 180ms ease; }
  .photo-cell:hover img { transform: scale(1.018); filter: brightness(0.96); }
  .edited { position: absolute; right: 7px; bottom: 7px; padding: 3px 7px; border-radius: 999px; color: white; font-size: 10px; font-weight: 700; background: rgba(0, 0, 0, 0.58); backdrop-filter: blur(8px); }
  .check { position: absolute; top: 8px; right: 8px; display: grid; width: 25px; height: 25px; place-items: center; border: 2px solid white; border-radius: 50%; color: white; font-weight: 800; background: var(--accent); box-shadow: 0 2px 8px rgba(0,0,0,.25); }
  @media (prefers-reduced-motion: reduce) { img { transition: none; } }
</style>
