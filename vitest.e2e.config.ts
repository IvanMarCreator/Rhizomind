import { defineConfig } from 'vitest/config';

// Сценарные тесты: настоящее окно собранного приложения через tauri-driver
// (WebDriver). Перед запуском нужна сборка: `npm run build`.
export default defineConfig({
  test: {
    include: ['tests/e2e/**/*.test.ts'],
    environment: 'node',
    fileParallelism: false,
    testTimeout: 60_000,
    hookTimeout: 120_000,
  },
});
