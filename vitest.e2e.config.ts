import { defineConfig } from 'vitest/config';

// Сценарные тесты: настоящее окно собранного приложения через tauri-driver
// (WebDriver). Перед запуском нужны сборки: `npm run build:e2e` (тестовая, с
// функцией webdriver) и `npm run build` (выпускная — для теста следов на Windows).
export default defineConfig({
  test: {
    include: ['tests/e2e/**/*.test.ts'],
    environment: 'node',
    fileParallelism: false,
    testTimeout: 60_000,
    hookTimeout: 120_000,
    teardownTimeout: 30_000,
  },
});
