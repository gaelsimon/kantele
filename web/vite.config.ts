import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteSingleFile } from 'vite-plugin-singlefile';

// One file, because the binary embeds it and a NAS may have no route to the internet.
export default defineConfig(({ mode }) => ({
  plugins: [svelte(), viteSingleFile()],
  // A component test mounts in jsdom, so Svelte must resolve to its browser build there.
  resolve: mode === 'test' ? { conditions: ['browser'] } : undefined,
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
}));
