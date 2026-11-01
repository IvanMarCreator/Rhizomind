import { defineConfig } from 'vitest/config';

// Замер производительности (задача 3.2, критерий С9.5): настоящее окно
// тестовой сборки (`npm run build:e2e`) на хранилище из 10 000 заметок от
// tools/gen-vault. В `npm run test` и `npm run test:e2e` не входит: отдельный
// шаг CI (`npm run test:perf`). Сводка (stats.test.ts) — без окна, идёт тут же.
export default defineConfig({
  test: {
    root: import.meta.dirname,
    include: ['**/*.test.ts'],
    environment: 'node',
    // Подробный вывод: строки «[perf] …» и таблица попадают в журнал CI и при успехе.
    reporters: ['verbose'],
    fileParallelism: false,
    // Пять запусков программы на 10 000 заметок и сотни нажатий клавиш:
    // одна проверка идёт минуты (Linux CI: создание сессии WebDriver ~30 с).
    testTimeout: 900_000,
    hookTimeout: 300_000,
    teardownTimeout: 30_000,
  },
});
