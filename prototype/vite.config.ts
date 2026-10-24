import { defineConfig } from 'vite';

// Интерфейс собирается в dist/renderer и отдаётся приложению
// через собственный протокол app:// (см. src/main/app-protocol.ts).
// Dev-сервер не используется: приложение никогда не грузит код с localhost.
export default defineConfig({
  root: 'src/renderer',
  base: './',
  build: {
    outDir: '../../dist/renderer',
    emptyOutDir: true,
    target: 'chrome120',
    modulePreload: { polyfill: false },
  },
});
