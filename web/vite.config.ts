import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

export default defineConfig({
  base: '/modern/',
  plugins: [svelte()],
  build: {
    outDir: '../frontend/modern',
    emptyOutDir: true,
    sourcemap: false,
  },
});
