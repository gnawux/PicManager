<script lang="ts">
  import Button from '../components/Button.svelte';

  interface Props {
    count: number;
    busy?: boolean;
    error?: string | null;
    onaction: (action: 'rotate_left' | 'rotate_right' | 'flip_h' | 'flip_v') => void;
    onclear: () => void;
  }

  let { count, busy = false, error = null, onaction, onclear }: Props = $props();
</script>

<div class="selection-bar" aria-label="批量操作">
  <strong aria-live="polite">已选 {count} 张</strong>
  <div class="actions">
    <Button disabled={busy} onclick={() => onaction('rotate_left')}>左转</Button>
    <Button disabled={busy} onclick={() => onaction('rotate_right')}>右转</Button>
    <Button disabled={busy} onclick={() => onaction('flip_h')}>水平翻转</Button>
    <Button disabled={busy} onclick={() => onaction('flip_v')}>垂直翻转</Button>
    <Button variant="ghost" disabled={busy} onclick={onclear}>取消选择</Button>
  </div>
  {#if busy}<span class="status" role="status">正在应用…</span>{/if}
  {#if error}<span class="error" role="alert">{error}</span>{/if}
</div>

<style>
  .selection-bar { position: fixed; right: 24px; bottom: 24px; left: 24px; z-index: 20; display: flex; align-items: center; gap: 18px; width: max-content; max-width: calc(100% - 48px); min-height: 58px; margin: auto; padding: 10px 12px 10px 20px; border: 1px solid rgba(255,255,255,.24); border-radius: 18px; color: white; background: rgba(27, 28, 31, .92); box-shadow: 0 18px 55px rgba(0,0,0,.28); backdrop-filter: blur(22px); }
  .actions { display: flex; gap: 6px; }
  .status { color: #d6e7ff; font-size: 13px; }
  .error { max-width: 260px; color: #ffb4ae; font-size: 13px; }
  @media (max-width: 760px) { .selection-bar { right: 10px; bottom: 78px; left: 10px; flex-wrap: wrap; width: auto; padding: 12px; } .actions { overflow-x: auto; } strong { width: 100%; } }
</style>
