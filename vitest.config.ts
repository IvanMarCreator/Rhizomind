import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

// Модульные тесты: проверки из tools/ и код интерфейса без окна (src/), в Node.
// Компонентные тесты (*.component.test.ts): компоненты Svelte в jsdom на
// подмене ядра (src/api/testing.ts). Сценарные тесты с настоящим окном —
// в vitest.e2e.config.ts.
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          include: ['tools/**/*.test.ts', 'src/**/*.test.ts'],
          exclude: ['src/**/*.component.test.ts'],
          environment: 'node',
        },
      },
      {
        plugins: [svelte({ configFile: false }), svelteTesting()],
        test: {
          name: 'components',
          include: ['src/**/*.component.test.ts'],
          environment: 'jsdom',
        },
      },
    ],
  },
});
