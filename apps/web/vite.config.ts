// SPDX-License-Identifier: AGPL-3.0-only
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  build: { outDir: 'dist', emptyOutDir: true, sourcemap: false },
  server: {
    // `pnpm dev` proxies API calls to a locally running server.
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
});
