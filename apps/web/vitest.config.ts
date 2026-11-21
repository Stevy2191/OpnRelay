// SPDX-License-Identifier: AGPL-3.0-only
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  test: { include: ['src/**/*.test.ts'] },
});
