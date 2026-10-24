import { defineConfig } from 'vitest/config';

// Модульные тесты: скрипты проверок и код без Electron (src/core, src/shared).
export default defineConfig({
  test: {
    include: ['tests/unit/**/*.test.ts'],
    environment: 'node',
  },
});
