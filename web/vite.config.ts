import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteSingleFile } from 'vite-plugin-singlefile';

// One file, because the binary embeds it and a NAS may have no route to the internet.
export default defineConfig({
  plugins: [svelte(), viteSingleFile()],
  build: {
    outDir: '../assets/web',
    emptyOutDir: true,
    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    cssCodeSplit: false,
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8200',
    },
  },
});
