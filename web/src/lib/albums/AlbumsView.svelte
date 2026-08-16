<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AlbumPhotoPage, AlbumSummary, CollectionSummary } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient }
  interface Selection { id: number; name: string; collection: boolean }
  interface AlbumSection { kind: string; kinds: string[]; label: string; icon: string; albums: AlbumSummary[] }

  let { api }: Props = $props();
  let albums = $state<AlbumSummary[]>([]);
  let collections = $state<CollectionSummary[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selected = $state<Selection | null>(null);
  let photos = $state<AlbumPhotoPage | null>(null);
  let photosLoading = $state(false);
  let newName = $state('');
  let creating = $state(false);

  const sortedCollections = $derived([...collections].sort(compareByCountAndName));
  const albumSections = $derived(buildSections(albums));

  onMount(() => { void reload(); });

  function compareByCountAndName(a: { photo_count: number; name: string }, b: { photo_count: number; name: string }) {
    return b.photo_count - a.photo_count || a.name.localeCompare(b.name, 'zh-CN');
  }

  function buildSections(source: AlbumSummary[]): AlbumSection[] {
    const definitions = [
      { kind: 'month', kinds: ['time', 'month'], label: '按月份', icon: '▦' },
      { kind: 'location', kinds: ['location'], label: '按地点', icon: '⌖' },
      { kind: 'camera', kinds: ['camera'], label: '按相机', icon: '◉' },
    ];
    const known = new Set([...definitions.flatMap((item) => item.kinds), 'curated']);
    const sections = definitions.map((definition) => ({
      ...definition,
      albums: source.filter((album) => definition.kinds.includes(album.kind)).sort(compareByCountAndName),
    }));
    const other = source.filter((album) => !known.has(album.kind)).sort(compareByCountAndName);
    if (other.length > 0) sections.push({ kind: 'other', kinds: [], label: '其他智能相册', icon: '✦', albums: other });
    return sections.filter((section) => section.albums.length > 0);
  }

  async function reload() {
    status = 'loading';
    error = null;
    try {
      [albums, collections] = await Promise.all([api.albums.list(), api.collections.list()]);
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function open(id: number, name: string, collection: boolean) {
    selected = { id, name, collection };
    photos = null;
    photosLoading = true;
    error = null;
    try {
      photos = collection ? await api.collections.photos(id) : await api.albums.photos(id);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      photosLoading = false;
    }
  }

  async function createCollection(event: SubmitEvent) {
    event.preventDefault();
    const name = newName.trim();
    if (!name || creating) return;
    creating = true;
    error = null;
    try {
      await api.collections.create(name);
      newName = '';
      collections = await api.collections.list();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      creating = false;
    }
  }
</script>

{#if status === 'loading'}
  <PageState kind="loading" title="正在整理相册" />
{:else if status === 'error'}
  <PageState kind="error" title="无法读取相册" message={error ?? undefined} />
{:else}
  <section class="albums" aria-labelledby="albums-title">
    <div class="view-heading">
      <div><p>Albums</p><h2 id="albums-title">相册</h2><span>从左侧选择相册，在右侧连续浏览照片。</span></div>
    </div>

    <div class="album-layout">
      <aside class="album-sidebar" aria-label="相册导航">
        <details class="album-section" open>
          <summary><span class="section-icon">✦</span><strong>我的精选集</strong><small>{collections.length}</small></summary>
          <form onsubmit={createCollection}>
            <input aria-label="新精选集名称" placeholder="新精选集名称" bind:value={newName} />
            <Button type="submit" variant="primary" disabled={creating || !newName.trim()}>{creating ? '创建中…' : '创建'}</Button>
          </form>
          {#if sortedCollections.length === 0}
            <p class="empty-copy">建立精选集，把旅行、家人或任何主题放在一起。</p>
          {:else}
            <div class="album-list">
              {#each sortedCollections as collection (collection.id)}
                <button class:active={selected?.collection && selected.id === collection.id} type="button" onclick={() => open(collection.id, collection.name, true)}>
                  <span class="row-icon collection-icon">✦</span><strong>{collection.name}</strong><small>{collection.photo_count}</small>
                </button>
              {/each}
            </div>
          {/if}
        </details>

        {#each albumSections as section, index (section.kind)}
          <details class="album-section" open={index === 0}>
            <summary><span class="section-icon">{section.icon}</span><strong>{section.label}</strong><small>{section.albums.length}</small></summary>
            <div class="album-list">
              {#each section.albums as album (album.id)}
                <button class:active={!selected?.collection && selected?.id === album.id} type="button" onclick={() => open(album.id, album.name, false)}>
                  <span class="row-icon">{section.icon}</span><strong>{album.name}</strong><small>{album.photo_count}</small>
                </button>
              {/each}
            </div>
          </details>
        {/each}

        {#if albumSections.length === 0 && collections.length === 0}
          <p class="empty-copy">导入照片后会自动按月份、相机和地点整理。</p>
        {/if}
      </aside>

      <div class="album-results" aria-live="polite">
        {#if error}
          <PageState kind="error" title="无法打开相册" message={error} />
        {:else if photosLoading}
          <PageState kind="loading" title="正在打开相册" />
        {:else if photos?.photos.length === 0}
          <PageState kind="empty" title="这个相册还是空的" />
        {:else if photos}
          <div class="result-heading">
            <div><small>{selected?.collection ? '我的精选集' : '智能相册'}</small><strong>{selected?.name}</strong></div>
            <span>{photos.total} 张</span>
          </div>
          <div class="photo-grid">
            {#each photos.photos as photo (photo.id)}
              <img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`照片 ${photo.id}`} loading="lazy" />
            {/each}
          </div>
        {:else}
          <p class="hint">选择一个相册查看照片。</p>
        {/if}
      </div>
    </div>
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading { margin-bottom: 18px; }
  .view-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 4px; font-size: 28px; }
  .view-heading span { color: var(--muted); font-size: 13px; }
  .album-layout { display: grid; grid-template-columns: minmax(250px, 350px) 1fr; gap: 24px; }
  .album-sidebar, .album-results { height: min(72vh, 720px); min-height: 420px; overflow: auto; border: 1px solid var(--line); border-radius: 16px; background: var(--surface-solid); }
  .album-sidebar { padding: 8px; overscroll-behavior: contain; }
  .album-section + .album-section { border-top: 1px solid var(--line); }
  summary { display: flex; align-items: center; gap: 8px; padding: 13px 10px; list-style: none; border-radius: 10px; cursor: pointer; }
  summary::-webkit-details-marker { display: none; }
  summary::before { width: 13px; content: '›'; color: var(--muted); font-size: 20px; transform-origin: center; transition: transform 120ms ease; }
  details[open] > summary::before { transform: rotate(90deg); }
  summary strong { flex: 1; }
  summary small, .album-list small { color: var(--muted); font-size: 11px; }
  .section-icon { display: grid; width: 22px; color: var(--accent); place-items: center; }
  form { display: grid; grid-template-columns: 1fr auto; gap: 6px; padding: 0 8px 9px 29px; }
  input { min-width: 0; min-height: 36px; padding: 0 10px; border: 1px solid var(--line); border-radius: 9px; background: var(--surface-solid); }
  .album-list { display: grid; gap: 2px; padding: 0 4px 9px 25px; }
  .album-list button { display: grid; grid-template-columns: 24px minmax(0, 1fr) auto; align-items: center; gap: 7px; width: 100%; padding: 8px 9px; border: 0; border-radius: 9px; color: var(--text); text-align: left; background: transparent; cursor: pointer; }
  .album-list button:hover { background: var(--fill-subtle); }
  .album-list button.active { color: white; background: var(--accent); }
  .album-list button.active small { color: rgba(255,255,255,.75); }
  .album-list strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .row-icon { color: var(--accent); text-align: center; }
  .collection-icon { color: #db5c89; }
  .album-list button.active .row-icon { color: white; }
  .empty-copy { margin: 0 8px 10px 29px; padding: 10px; border-radius: 9px; color: var(--muted); background: var(--fill-subtle); font-size: 12px; }
  .album-results { padding: 14px; }
  .result-heading { display: flex; justify-content: space-between; align-items: end; padding: 3px 2px 14px; }
  .result-heading div { display: grid; gap: 3px; }
  .result-heading strong { font-size: 18px; }
  .result-heading small, .result-heading span, .hint { color: var(--muted); }
  .hint { display: grid; height: 300px; margin: 0; place-items: center; }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 4px; }
  .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 4px; background: var(--fill-subtle); }
  @media (max-width: 760px) { .album-layout { grid-template-columns: 1fr; } .album-sidebar, .album-results { height: auto; max-height: 62vh; min-height: 300px; } }
</style>
