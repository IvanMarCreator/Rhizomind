import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// Интерфейс собирается в dist/ и встраивается в исполняемый файл Tauri
// (frontendDist в src-tauri/tauri.conf.json). Сервер Vite работает только в
// режиме разработки (`npm run dev`) и только на localhost.
export default defineConfig({
  root: 'src',
  base: './',
  publicDir: false,
  plugins: [svelte({ configFile: false })],
  clearScreen: false,
  server: {
    host: 'localhost',
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: ['chrome120', 'safari16'],
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
});
