<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
    variant?: 'primary' | 'secondary' | 'ghost';
    disabled?: boolean;
    pressed?: boolean;
    onclick?: (event: MouseEvent) => void;
  }

  let {
    children,
    variant = 'secondary',
    disabled = false,
    pressed,
    onclick,
  }: Props = $props();
</script>

<button
  class="button {variant}"
  type="button"
  {disabled}
  aria-pressed={pressed}
  {onclick}
>
  {@render children()}
</button>

<style>
  .button {
    min-height: 36px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    font-weight: 620;
    cursor: pointer;
    transition: transform 120ms ease, background 120ms ease, border-color 120ms ease;
  }

  .button:active:not(:disabled) { transform: scale(0.98); }
  .button:focus-visible { outline: 3px solid var(--focus); outline-offset: 2px; }
  .button:disabled { cursor: default; opacity: 0.5; }
  .primary { color: white; background: var(--accent); }
  .secondary { color: var(--text); border-color: var(--line); background: var(--surface-solid); }
  .ghost { color: var(--muted); background: transparent; }
  .ghost:hover:not(:disabled) { color: var(--text); background: var(--fill-subtle); }
</style>
