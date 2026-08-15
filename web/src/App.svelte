<script lang="ts">
  import { onMount } from 'svelte';
  import { createApiClient } from './lib/api/client';
  import Button from './lib/components/Button.svelte';
  import PageState from './lib/components/PageState.svelte';
  import StatusBadge from './lib/components/StatusBadge.svelte';
  import { createHashRouter, routes } from './lib/router';
  import { createLibraryStore } from './lib/state/library';

  const router = createHashRouter();
  const library = createLibraryStore(createApiClient());

  onMount(() => {
    const stopRouter = router.start();
    void library.load();
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
        <Button variant="ghost" onclick={() => library.load()}>刷新</Button>
      </section>

      {#if $library.status === 'loading' && !$library.data}
        <PageState kind="loading" title="正在读取图库" message="照片仍保留在你的设备上。" />
      {:else if $library.status === 'error'}
        <PageState kind="error" title="暂时无法读取图库" message={$library.error.message} />
      {:else if $library.status === 'ready' && $library.data.total === 0}
        <PageState kind="empty" title="图库还是空的" message="完成一次导入后，照片会按日期出现在这里。" />
      {:else if $library.data}
        <div class="platform-preview">
          <span>{$library.data.total.toLocaleString()} 张照片</span>
          <span>API、路由和状态层已连接</span>
        </div>
      {/if}
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
