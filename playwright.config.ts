import { defineConfig } from '@playwright/test';

// Сценарные тесты запускают собранное приложение (npm run build) через Electron.
export default defineConfig({
  testDir: 'tests/e2e',
  outputDir: 'test-results/playwright',
  timeout: 60_000,
  workers: 1,
  reporter: process.env.CI ? [['list'], ['github']] : 'list',
});
