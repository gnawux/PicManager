<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { PhotoDetail, TimelineItem } from '../api/types';
  import PageState from '../components/PageState.svelte';

  interface Props {
    api: ApiClient;
    item: TimelineItem;
    previous?: TimelineItem;
    next?: TimelineItem;
    onclose: () => void;
    onnavigate: (direction: -1 | 1) => void;
  }

  let { api, item, previous, next, onclose, onnavigate }: Props = $props();
  let closeButton: HTMLButtonElement;
  let viewerElement: HTMLElement;
  let detail = $state<PhotoDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let drawerOpen = $state(true);
  let rendition = $state<'display' | 'current' | 'original'>('display');
  let requestVersion = 0;
  let imageUrl = $derived(
    rendition === 'original'
      ? detail?.renditions.original
      : rendition === 'current'
        ? detail?.renditions.current
        : detail?.renditions.display,
  );

  onMount(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const background = [...document.querySelectorAll<HTMLElement>(
      '.topbar, .page-heading, .timeline-controls, .timeline, .selection-bar',
    )].map((element) => ({
      element,
      inert: element.inert,
      ariaHidden: element.getAttribute('aria-hidden'),
    }));
    for (const item of background) {
      item.element.inert = true;
      item.element.setAttribute('aria-hidden', 'true');
    }
    closeButton.focus();
    return () => {
      document.body.style.overflow = previousOverflow;
      for (const item of background) {
        item.element.inert = item.inert;
        if (item.ariaHidden === null) item.element.removeAttribute('aria-hidden');
        else item.element.setAttribute('aria-hidden', item.ariaHidden);
      }
      previousFocus?.focus();
    };
  });

  $effect(() => {
    const id = item.id;
    const version = ++requestVersion;
    loading = true;
    error = null;
    detail = null;
    rendition = item.has_current ? 'current' : 'display';
    void api.photos.get(id).then(
      (result) => {
        if (version !== requestVersion) return;
        detail = result;
        if (rendition === 'current' && !result.renditions.current) rendition = 'display';
        loading = false;
      },
      (reason) => {
        if (version !== requestVersion) return;
        error = reason instanceof Error ? reason.message : String(reason);
        loading = false;
      },
    );
  });

  $effect(() => {
    for (const adjacent of [previous, next]) {
      if (adjacent && typeof Image !== 'undefined') {
        const image = new Image();
        image.src = adjacent.file_url;
      }
    }
  });

  function handleKey(event: KeyboardEvent) {
    if (event.key === 'Tab') {
      const focusable = [...viewerElement.querySelectorAll<HTMLElement>(
        'button:not(:disabled), [href], input:not(:disabled), [tabindex]:not([tabindex="-1"])',
      )];
      const first = focusable[0];
      const last = focusable.at(-1);
      if (event.shiftKey && document.activeElement === first) last?.focus();
      else if (!event.shiftKey && document.activeElement === last) first?.focus();
      else return;
    } else if (event.key === 'Escape') onclose();
    else if (event.key === 'ArrowLeft' && previous) onnavigate(-1);
    else if (event.key === 'ArrowRight' && next) onnavigate(1);
    else if (event.key.toLowerCase() === 'i') drawerOpen = !drawerOpen;
    else return;
    event.preventDefault();
  }

  function formatDate(value: string | null) {
    if (!value) return '未知时间';
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat('zh-CN', {
      dateStyle: 'long', timeStyle: 'medium',
    }).format(date);
  }

  function providerLabel(provider: string) {
    return provider === 'apple_photos' ? 'Apple 照片'
      : provider === 'google_photos' ? 'Google Photos'
      : provider === 'legacy_local' ? '已有图库'
      : provider.replaceAll('_', ' ');
  }
</script>

<svelte:window onkeydown={handleKey} />

<div
  class="viewer"
  bind:this={viewerElement}
  role="dialog"
  aria-modal="true"
  aria-label={`照片 ${item.id}`}
  tabindex="-1"
>
  <header>
    <button bind:this={closeButton} class="icon-button" type="button" aria-label="关闭查看器" onclick={onclose}>×</button>
    <div class="title">
      <strong>{detail?.sources.find((source) => source.original_filename)?.original_filename ?? `照片 ${item.id}`}</strong>
      <span>{formatDate(detail?.taken_at ?? item.taken_at)}</span>
    </div>
    <div class="toolbar">
      {#if detail?.renditions.original && detail?.renditions.current}
        <div class="rendition-switch" aria-label="照片版本">
          <button class:active={rendition === 'current'} aria-pressed={rendition === 'current'} type="button" onclick={() => { rendition = 'current'; }}>当前效果</button>
          <button class:active={rendition === 'original'} aria-pressed={rendition === 'original'} type="button" onclick={() => { rendition = 'original'; }}>原始文件</button>
        </div>
      {/if}
      <button class="icon-button info" class:active={drawerOpen} type="button" aria-label="照片信息" aria-pressed={drawerOpen} onclick={() => { drawerOpen = !drawerOpen; }}>i</button>
    </div>
  </header>

  <div class="viewer-content" class:with-drawer={drawerOpen}>
    <section class="stage" aria-live="polite">
      {#if loading}
        <PageState kind="loading" title="正在载入照片" />
      {:else if error}
        <PageState kind="error" title="无法载入照片" message={error} />
      {:else if imageUrl}
        <img class="viewer-photo" src={imageUrl} alt={detail?.sources[0]?.original_filename ?? `照片 ${item.id}`} />
      {/if}
      <button class="nav previous" type="button" aria-label="上一张照片" disabled={!previous} onclick={() => onnavigate(-1)}>‹</button>
      <button class="nav next" type="button" aria-label="下一张照片" disabled={!next} onclick={() => onnavigate(1)}>›</button>
    </section>

    {#if drawerOpen}
      <aside aria-label="照片元数据">
        <h2>信息</h2>
        {#if detail}
          <dl>
            <div><dt>拍摄时间</dt><dd>{formatDate(detail.taken_at)}</dd></div>
            <div><dt>相机</dt><dd>{detail.camera ?? '未知'}</dd></div>
            <div><dt>尺寸</dt><dd>{detail.width && detail.height ? `${detail.width} × ${detail.height}` : '未知'}</dd></div>
            <div><dt>格式</dt><dd>{detail.format.toUpperCase()}</dd></div>
            {#if detail.gps_lat !== null && detail.gps_lon !== null}
              <div><dt>位置</dt><dd>{detail.gps_lat.toFixed(5)}, {detail.gps_lon.toFixed(5)}</dd></div>
            {/if}
          </dl>
          <h3>来源</h3>
          {#if detail.sources.length > 0}
            <ul>
              {#each detail.sources as source}
                <li><strong>{providerLabel(source.provider)}</strong><span>{source.original_filename ?? '未记录文件名'}</span></li>
              {/each}
            </ul>
          {:else}
            <p class="muted">本地图库</p>
          {/if}
        {/if}
      </aside>
    {/if}
  </div>
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 100; display: grid; grid-template-rows: 64px 1fr; color: #f8f8fa; background: rgba(13, 14, 17, .98); }
  header { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 16px; padding: 0 18px; border-bottom: 1px solid rgba(255,255,255,.12); }
  .icon-button { display: grid; width: 40px; height: 40px; place-items: center; padding: 0; border: 0; border-radius: 50%; color: white; font-size: 30px; background: rgba(255,255,255,.1); cursor: pointer; }
  .icon-button:focus-visible, button:focus-visible { outline: 3px solid #75aaff; outline-offset: 2px; }
  .title { display: grid; min-width: 0; text-align: center; }
  .title strong { max-width: 36vw; overflow: hidden; font-size: 14px; text-overflow: ellipsis; white-space: nowrap; }
  .title span { color: #aaaeb8; font-size: 11px; }
  .toolbar { display: flex; justify-self: end; align-items: center; gap: 10px; }
  .info { font: 700 17px Georgia, serif; }
  .info.active { color: #0e1116; background: white; }
  .rendition-switch { display: flex; padding: 3px; border-radius: 10px; background: rgba(255,255,255,.1); }
  .rendition-switch button { padding: 7px 11px; border: 0; border-radius: 8px; color: #c7cad1; font-size: 12px; background: transparent; cursor: pointer; }
  .rendition-switch button.active { color: #111318; background: white; }
  .viewer-content { display: grid; min-height: 0; }
  .viewer-content.with-drawer { grid-template-columns: minmax(0, 1fr) 320px; }
  .stage { position: relative; display: grid; min-width: 0; min-height: 0; grid-template: minmax(0, 1fr) / minmax(0, 1fr); place-items: center; padding: 32px 72px; overflow: hidden; }
  .viewer-photo { width: 100%; height: 100%; object-fit: contain; filter: drop-shadow(0 24px 70px rgba(0,0,0,.34)); }
  .nav { position: absolute; top: 50%; display: grid; width: 48px; height: 64px; place-items: center; border: 0; border-radius: 14px; color: white; font-size: 44px; line-height: 1; background: rgba(255,255,255,.1); transform: translateY(-50%); cursor: pointer; }
  .nav:disabled { opacity: 0; pointer-events: none; }
  .previous { left: 16px; } .next { right: 16px; }
  aside { min-width: 0; padding: 28px 24px; overflow-y: auto; color: #1e2025; background: #f5f5f7; }
  aside h2 { margin: 0 0 26px; font-size: 24px; }
  aside h3 { margin: 28px 0 12px; font-size: 13px; text-transform: uppercase; letter-spacing: .08em; }
  dl { display: grid; gap: 18px; margin: 0; }
  dl div { display: grid; gap: 4px; }
  dt { color: #73767d; font-size: 12px; } dd { margin: 0; font-size: 14px; line-height: 1.4; }
  ul { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  li { display: grid; gap: 3px; padding: 12px; border-radius: 10px; background: white; }
  li strong { font-size: 13px; } li span, .muted { color: #73767d; font-size: 12px; overflow-wrap: anywhere; }
  @media (max-width: 760px) {
    header { grid-template-columns: auto 1fr auto; padding: 0 10px; }
    .title { text-align: left; } .title strong { max-width: 30vw; }
    .rendition-switch button { padding-inline: 8px; }
    .viewer-content.with-drawer { grid-template: minmax(0, 1fr) minmax(180px, 38vh) / 1fr; }
    .stage { padding: 16px 42px; }
    aside { padding: 18px 20px; }
    aside h2 { margin-bottom: 14px; }
  }
</style>
