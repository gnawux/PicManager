<script lang="ts">
  import { onMount } from 'svelte';
  import type { ApiClient } from '../api/client';
  import type { PersonSummary, PhotoPage } from '../api/types';
  import Button from '../components/Button.svelte';
  import PageState from '../components/PageState.svelte';

  interface Props { api: ApiClient }
  let { api }: Props = $props();
  let people = $state<PersonSummary[]>([]);
  let statusFilter = $state('active');
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let error = $state<string | null>(null);
  let selected = $state<PersonSummary | null>(null);
  let photos = $state<PhotoPage | null>(null);
  let photosLoading = $state(false);
  let loadingMore = $state(false);
  let loadMoreError = $state<string | null>(null);
  let photoRequest = 0;
  let editingId = $state<number | null>(null);
  let editName = $state('');
  let busyId = $state<number | null>(null);
  let discovering = $state(false);
  let discoveryMessage = $state<string | null>(null);

  onMount(() => { void reload(); });

  async function reload() {
    status = 'loading';
    error = null;
    try {
      people = await api.people.list(statusFilter);
      status = 'ready';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      status = 'error';
    }
  }

  async function open(person: PersonSummary) {
    const request = ++photoRequest;
    selected = person;
    photos = null;
    photosLoading = true;
    loadingMore = false;
    loadMoreError = null;
    error = null;
    try {
      const page = await api.people.photos(person.id, 1, 100);
      if (request === photoRequest) photos = page;
    } catch (reason) {
      if (request === photoRequest) error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === photoRequest) photosLoading = false;
    }
  }

  async function loadMore() {
    if (!selected || !photos || loadingMore || photos.photos.length >= photos.total) return;
    const request = photoRequest;
    loadingMore = true;
    loadMoreError = null;
    try {
      const next = await api.people.photos(selected.id, photos.page + 1, photos.per_page);
      if (request !== photoRequest) return;
      const existing = new Set(photos.photos.map((photo) => photo.id));
      photos = { ...next, photos: [...photos.photos, ...next.photos.filter((photo) => !existing.has(photo.id))] };
    } catch (reason) {
      if (request === photoRequest) loadMoreError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (request === photoRequest) loadingMore = false;
    }
  }

  function startRename(person: PersonSummary) {
    editingId = person.id;
    editName = person.name ?? '';
  }

  async function saveName(person: PersonSummary) {
    const name = editName.trim();
    if (!name) return;
    busyId = person.id;
    try {
      await api.people.update(person.id, { name });
      person.name = name;
      people = [...people];
      editingId = null;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busyId = null;
    }
  }

  async function setPersonStatus(person: PersonSummary, nextStatus: string) {
    busyId = person.id;
    error = null;
    try {
      await api.people.update(person.id, { status: nextStatus });
      people = people.filter((candidate) => candidate.id !== person.id);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busyId = null;
    }
  }

  async function discover() {
    discovering = true;
    discoveryMessage = null;
    try {
      const result = await api.people.discover();
      discoveryMessage = result.people_created > 0
        ? `发现 ${result.people_created} 个新人物`
        : '没有发现新的可识别人脸';
      await reload();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      discovering = false;
    }
  }
</script>

{#if selected}
  <section class="person-detail" aria-labelledby="person-title">
    <div class="detail-heading">
      <Button variant="ghost" onclick={() => { photoRequest += 1; selected = null; photos = null; }}>← 返回人物</Button>
      <div><p>Person</p><h2 id="person-title">{selected.name ?? `未命名人物 ${selected.id}`}</h2></div>
      <span>{photos ? `已显示 ${photos.photos.length} / ${photos.total} 张` : `${selected.photo_count} 张`}</span>
    </div>
    {#if error}
      <PageState kind="error" title="无法读取人物照片" message={error} />
    {:else if photosLoading || !photos}
      <PageState kind="loading" title="正在整理人物照片" />
    {:else if photos.photos.length === 0}
      <PageState kind="empty" title="没有关联照片" />
    {:else}
      <div class="photo-grid">
        {#each photos.photos as photo (photo.id)}
          <img src={`/api/photos/${photo.id}/thumb?size=512`} alt={`照片 ${photo.id}`} loading="lazy" />
        {/each}
      </div>
      {#if loadMoreError}<p class="load-error" role="alert">载入下一页失败：{loadMoreError}</p>{/if}
      {#if photos.photos.length < photos.total}
        <div class="load-more"><Button disabled={loadingMore} onclick={loadMore}>{loadingMore ? '正在载入…' : '载入更多'}</Button></div>
      {/if}
    {/if}
  </section>
{:else}
  <section aria-labelledby="people-title">
    <div class="view-heading">
      <div><p>Faces</p><h2 id="people-title">人物</h2><span>在照片中快速找到重要的人。</span></div>
      <div class="controls">
        <label>显示
          <select bind:value={statusFilter} onchange={() => { void reload(); }}>
            <option value="active">活跃</option><option value="ignored">已忽略</option><option value="all">全部</option>
          </select>
        </label>
        <Button disabled={discovering} onclick={discover}>{discovering ? '分析中…' : '发现人物'}</Button>
      </div>
    </div>
    {#if discoveryMessage}<p class="notice" role="status">{discoveryMessage}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if status === 'loading'}
      <PageState kind="loading" title="正在载入人物" />
    {:else if status === 'error'}
      <PageState kind="error" title="无法读取人物" message={error ?? undefined} />
    {:else if people.length === 0}
      <PageState kind="empty" title={statusFilter === 'ignored' ? '没有已忽略人物' : '还没有识别出人物'} message="可以运行“发现人物”分析已导入照片。" />
    {:else}
      <div class="people-grid">
        {#each people as person (person.id)}
          <article class="person-card">
            <button class="portrait" type="button" aria-label={`打开${person.name ?? `未命名人物 ${person.id}`}`} onclick={() => open(person)}>
              {#if person.cover_face_id}<img src={`/api/faces/${person.cover_face_id}/thumb`} alt="" loading="lazy" />{:else}<span aria-hidden="true">人</span>{/if}
            </button>
            {#if editingId === person.id}
              <form onsubmit={(event) => { event.preventDefault(); void saveName(person); }}>
                <input aria-label={`人物 ${person.id} 名称`} bind:value={editName} />
                <Button type="submit" variant="primary" disabled={busyId === person.id || !editName.trim()}>保存</Button>
              </form>
            {:else}
              <div class="identity"><strong>{person.name ?? '未命名'}</strong><span>{person.photo_count} 张照片 · {person.face_count} 张人脸</span></div>
              <div class="actions">
                <button type="button" onclick={() => startRename(person)}>命名</button>
                <button type="button" disabled={busyId === person.id} onclick={() => setPersonStatus(person, person.status === 'ignored' ? 'active' : 'ignored')}>{person.status === 'ignored' ? '恢复' : '忽略'}</button>
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  section { margin-top: 34px; }
  .view-heading, .detail-heading { display: flex; justify-content: space-between; align-items: center; gap: 24px; margin-bottom: 24px; }
  .view-heading p, .detail-heading p { margin: 0 0 7px; color: var(--accent); font-size: 11px; font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }
  h2 { margin: 0 0 5px; font-size: 28px; } .view-heading span, .detail-heading > span { color: var(--muted); }
  .controls { display: flex; align-items: center; gap: 12px; }
  label { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: 13px; }
  select, input { min-height: 38px; padding: 0 10px; border: 1px solid var(--line); border-radius: 10px; color: var(--text); background: var(--surface-solid); }
  .notice, .error { padding: 11px 14px; border-radius: 10px; font-size: 13px; } .notice { color: #126137; background: #e8f7ee; } .error { color: var(--danger); background: #fff0ee; }
  .people-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 20px; }
  .person-card { min-width: 0; padding: 10px 10px 14px; border: 1px solid var(--line); border-radius: 18px; background: var(--surface-solid); }
  .portrait { display: grid; width: 100%; aspect-ratio: 1; padding: 0; overflow: hidden; place-items: center; border: 0; border-radius: 14px; color: white; font-size: 44px; background: linear-gradient(145deg, #8eb9e9, #826fbd); cursor: pointer; }
  .portrait img { width: 100%; height: 100%; object-fit: cover; }
  .identity { display: grid; gap: 4px; padding: 12px 4px 8px; }
  .identity strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .identity span { color: var(--muted); font-size: 12px; }
  .actions { display: flex; gap: 4px; }
  .actions button { padding: 6px 8px; border: 0; border-radius: 8px; color: var(--accent); background: transparent; cursor: pointer; }
  .actions button:hover { background: var(--fill-subtle); }
  form { display: grid; grid-template-columns: 1fr auto; gap: 6px; padding-top: 10px; }
  form input { min-width: 0; }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 5px; }
  .photo-grid img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 5px; background: var(--fill-subtle); }
  .load-more { display: flex; justify-content: center; padding: 18px 0 6px; }
  .load-error { margin: 14px 0 0; color: var(--danger); text-align: center; font-size: 13px; }
  @media (max-width: 680px) { .view-heading { align-items: stretch; flex-direction: column; } .controls { justify-content: space-between; } .people-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; } .detail-heading { flex-wrap: wrap; } }
</style>
