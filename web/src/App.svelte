<script lang="ts">
  import { onMount } from 'svelte';
  import { createApiClient } from './lib/api/client';
  import AlbumsView from './lib/albums/AlbumsView.svelte';
  import PeopleView from './lib/people/PeopleView.svelte';
  import StatusBadge from './lib/components/StatusBadge.svelte';
  import TimelineView from './lib/timeline/TimelineView.svelte';
  import { createHashRouter, routes } from './lib/router';

  const router = createHashRouter();
  const api = createApiClient();

  onMount(() => {
    const stopRouter = router.start();
    return stopRouter;
  });
</script>

<svelte:head>
  <meta name="description" content="A private, high-density home for your photo library" />
</svelte:head>

<div class="app-shell">
  <header class="topbar">
    <a class="brand" href="#/photos" aria-label="PicManager 首页">
      <span class="brand-mark" aria-hidden="true">P</span>
      <span>PicManager</span>
    </a>
    <nav aria-label="主要导航">
      {#each routes as route}
        <button
          class:active={$router.id === route.id}
          type="button"
          aria-current={$router.id === route.id ? 'page' : undefined}
          onclick={() => router.navigate(route)}
        >{route.label}</button>
      {/each}
    </nav>
    <div class="topbar-actions">
      <StatusBadge label="本地私有" tone="success" />
      <a class="legacy-link" href="/legacy/">经典界面</a>
    </div>
  </header>

  <main>
    {#if $router.id === 'photos'}
      <section class="page-heading" aria-labelledby="page-title">
        <div>
          <p class="eyebrow">Library</p>
          <h1 id="page-title">所有照片</h1>
          <p class="subtitle">按拍摄时间浏览你的完整图库。</p>
        </div>
      </section>
      <TimelineView {api} />
    {:else if $router.id === 'albums'}
      <section class="page-heading" aria-labelledby="page-title">
        <div><p class="eyebrow">Collections</p><h1 id="page-title">相册</h1><p class="subtitle">浏览自动整理的图库，也建立自己的精选集。</p></div>
      </section>
      <AlbumsView {api} />
    {:else if $router.id === 'people'}
      <section class="page-heading" aria-labelledby="page-title">
        <div><p class="eyebrow">People</p><h1 id="page-title">人物</h1><p class="subtitle">按人物浏览照片，并整理识别结果。</p></div>
      </section>
      <PeopleView {api} />
    {:else}
      <section class="page-heading" aria-labelledby="page-title">
        <div>
          <p class="eyebrow">{$router.id}</p>
          <h1 id="page-title">{$router.label}</h1>
          <p class="subtitle">这个视图将在接下来的迁移里程碑中接入。</p>
        </div>
        <a class="legacy-link" href="/legacy/">在经典界面中打开</a>
      </section>
    {/if}
  </main>
</div>
