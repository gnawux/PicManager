<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { AlbumPhotoPage, AlbumSummary, CollectionSummary } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let albums = $state<AlbumSummary[]>([]);
  let collections = $state<CollectionSummary[]>([]);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selected = $state<{ id: number; name: string; collection: boolean } | null>(null);
  let photos = $state<AlbumPhotoPage | null>(null);
  let photosLoading = $state(false);
  let newName = $state('');
  let creating = $state(false);

  onMount(() => { void reload(); });

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
      photos = collection
        ? await api.collections.photos(id)
        : await api.albums.photos(id);
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

  function kindLabel(kind: string) {
    return kind === 'month' ? '月份' : kind === 'camera' ? '相机' : kind === 'location' ? '地点' : '智能相册';
  }
</script>

{#if selected}
  <section class="album-detail" aria-labelledby="album-detail-title">
    <div class="detail-heading">
      <Button variant="ghost" onclick={() => { selected = null; photos = null; }}>← 返回相册</Button>
      <div><p>{selected.collection ? '自建精选集' : '智能相册'}</p><h2 id="album-detail-title">{selected.name}</h2></div>
      <span>{photos?.total ?? 0} 张</span>
    </div>
    {#if photosLoading}
      <PageState kind="loading" title="正在打开相册" />
    {:else if error}
      <PageState kind="error" title="无法打开相册" message={error} />
    {:else if photos?.photos.length === 0}
      <PageState kind="empty" title="这个相册还是空的" />
    {:else if photos}
      <div class="photo-grid">
        {#each photos.photos as photo (photo.id)}
          <img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`照片 ${photo.id}`} loading="lazy" />
        {/each}
      </div>
    {/if}
  </section>
{:else if status === 'loading'}
  <PageState kind="loading" title="正在整理相册" />
{:else if status === 'error'}
  <PageState kind="error" title="无法读取相册" message={error ?? undefined} />
{:else}
  <section class="collections" aria-labelledby="collections-title">
    <div class="section-heading">
      <div><p>Collections</p><h2 id="collections-title">我的精选集</h2></div>
      <form onsubmit={createCollection}>
        <input aria-label="新精选集名称" placeholder="新精选集名称" bind:value={newName} />
        <Button type="submit" variant="primary" disabled={creating || !newName.trim()}>{creating ? '创建中…' : '创建'}</Button>
      </form>
    </div>
    {#if collections.length === 0}
      <p class="empty-copy">建立精选集，把旅行、家人或任何主题放在一起。</p>
    {:else}
      <div class="card-grid">
        {#each collections as collection (collection.id)}
          <button class="album-card collection-card" type="button" onclick={() => open(collection.id, collection.name, true)}>
            <span class="cover"><span>✦</span></span>
            <strong>{collection.name}</strong><small>{collection.photo_count} 张照片</small>
          </button>
        {/each}
      </div>
    {/if}
  </section>

  <section class="smart" aria-labelledby="smart-title">
    <div class="section-heading"><div><p>Library</p><h2 id="smart-title">智能相册</h2></div></div>
    {#if albums.length === 0}
      <PageState kind="empty" title="还没有智能相册" message="导入照片后会自动按月份、相机和地点整理。" />
    {:else}
      <div class="card-grid">
        {#each albums.filter((album) => album.kind !== 'curated') as album (album.id)}
          <button class="album-card" type="button" onclick={() => open(album.id, album.name, false)}>
            <span class="cover" aria-hidden="true">{album.kind === 'month' ? '▦' : album.kind === 'camera' ? '◉' : '⌖'}</span>
            <span class="kind">{kindLabel(album.kind)}</span>
            <strong>{album.name}</strong><small>{album.photo_count} 张照片</small>
          </button>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .section-heading, .detail-heading { display: flex; justify-content: space-between; align-items: end; gap: 24px; margin-bottom: 20px; }
  .section-heading p, .detail-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0; font-size: 26px; letter-spacing: -.025em; }
  form { display: flex; gap: 8px; }
  input { min-width: 220px; min-height: 38px; padding: 0 12px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface-solid); }
  .card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 18px; }
  .album-card { display: grid; gap: 5px; min-width: 0; padding: 0 0 14px; overflow: hidden; border: 1px solid var(--line); border-radius: 16px; color: var(--text); text-align: left; background: var(--surface-solid); box-shadow: 0 8px 24px rgba(0,0,0,.04); cursor: pointer; transition: transform 150ms ease, box-shadow 150ms ease; }
  .album-card:hover { transform: translateY(-2px); box-shadow: 0 14px 34px rgba(0,0,0,.08); }
  .cover { display: grid; height: 132px; margin-bottom: 8px; place-items: center; overflow: hidden; color: white; font-size: 42px; background: linear-gradient(145deg, #78aefd, #6a5ae0 62%, #dd78b0); }
  .collection-card .cover { background: linear-gradient(145deg, #ff9b72, #db5c89 55%, #725ad8); }
  .album-card strong, .album-card small, .kind { margin-inline: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .album-card small { color: var(--muted); }
  .kind { color: var(--accent); font-size: 10px; font-weight: 750; letter-spacing: .08em; text-transform: uppercase; }
  .smart { padding-top: 26px; border-top: 1px solid var(--line); }
  .empty-copy { padding: 18px 20px; border-radius: 12px; color: var(--muted); background: var(--fill-subtle); }
  .detail-heading { align-items: center; }
  .detail-heading > span { color: var(--muted); }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 5px; }
  .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 5px; background: var(--fill-subtle); }
  @media (max-width: 680px) { .section-heading { align-items: stretch; flex-direction: column; } form, input { width: 100%; } .card-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; } .cover { height: 106px; } .detail-heading { flex-wrap: wrap; } }
</style>
