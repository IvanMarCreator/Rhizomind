import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// Интерфейс собирается в dist/renderer и отдаётся приложению через протокол
// app:// (src/main/app-protocol.ts). Dev-сервер не используется: приложение
// никогда не грузит код с localhost.
export default defineConfig({
  root: 'src/renderer',
  base: './',
  plugins: [svelte({ configFile: false })],
  build: {
    outDir: '../../dist/renderer',
    emptyOutDir: true,
    target: 'chrome140',
    modulePreload: { polyfill: false },
  },
});
