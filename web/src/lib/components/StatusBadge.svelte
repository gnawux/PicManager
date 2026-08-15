<script lang="ts">
  interface Props {
    label: string;
    tone?: 'neutral' | 'success' | 'warning' | 'danger';
  }
  let { label, tone = 'neutral' }: Props = $props();
</script>

<span class="badge {tone}">{label}</span>

<style>
  .badge { display: inline-flex; align-items: center; min-height: 24px; padding: 0 9px; border-radius: 999px; font-size: 12px; font-weight: 680; }
  .neutral { color: var(--muted); background: var(--fill-subtle); }
  .success { color: #176b3a; background: #e5f6eb; }
  .warning { color: #8a5610; background: #fff1d6; }
  .danger { color: #a32c2c; background: #ffe8e7; }
</style>
