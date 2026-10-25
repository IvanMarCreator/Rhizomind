import { defineConfig } from 'vitest/config';

// Модульные тесты: проверки из tools/ и код интерфейса без окна (src/).
// Сценарные тесты с настоящим окном — в vitest.e2e.config.ts.
export default defineConfig({
  test: {
    include: ['tools/**/*.test.ts', 'src/**/*.test.ts', 'src-isolation/**/*.test.ts'],
    environment: 'node',
  },
});
