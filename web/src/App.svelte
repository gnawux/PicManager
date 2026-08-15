<script lang="ts">
  const sections = ['照片', '相册', '人物', '地点'];
</script>

<svelte:head>
  <meta
    name="description"
    content="A private, high-density home for your photo library"
  />
</svelte:head>

<div class="app-shell">
  <header class="topbar">
    <a class="brand" href="/modern/" aria-label="PicManager 首页">
      <span class="brand-mark" aria-hidden="true">P</span>
      <span>PicManager</span>
    </a>
    <nav aria-label="主要导航">
      {#each sections as section, index}
        <button class:active={index === 0} type="button">{section}</button>
      {/each}
    </nav>
    <a class="legacy-link" href="/">经典界面</a>
  </header>

  <main>
    <section class="intro" aria-labelledby="modern-title">
      <p class="eyebrow">Modern photo library</p>
      <h1 id="modern-title">你的照片，终于有足够的空间。</h1>
      <p>新的高密度浏览体验正在搭建中，现有功能仍可从经典界面完整使用。</p>
    </section>
  </main>
</div>
