<script lang="ts">
  interface Props {
    kind: 'loading' | 'empty' | 'error';
    title: string;
    message?: string;
  }
  let { kind, title, message }: Props = $props();
</script>

<div class="page-state" role={kind === 'error' ? 'alert' : 'status'}>
  {#if kind === 'loading'}<span class="spinner" aria-hidden="true"></span>{/if}
  <strong>{title}</strong>
  {#if message}<p>{message}</p>{/if}
</div>

<style>
  .page-state { display: grid; min-height: 260px; place-items: center; align-content: center; gap: 12px; color: var(--muted); text-align: center; }
  .page-state strong { color: var(--text); font-size: 17px; }
  .page-state p { max-width: 440px; margin: 0; line-height: 1.5; }
  .spinner { width: 26px; height: 26px; border: 3px solid var(--line); border-top-color: var(--accent); border-radius: 50%; animation: spin 700ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation-duration: 1.6s; } }
</style>
