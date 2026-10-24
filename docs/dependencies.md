# Зависимости

Перечень прямых зависимостей Rhizomind: версия (фиксируется точно, без `^` и `~`, вместе с `package-lock.json`), лицензия, зачем нужна и есть ли сетевой код в работающем приложении. Полный перечень со всеми вложенными пакетами — `npm run sbom` (`release/sbom.json`, формат CycloneDX).

Правила (план 0.1, раздел 1.2): лицензия совместима с GPL-3.0, нет сетевого кода и телеметрии в том, что попадает в сборку. Лицензии production-зависимостей (включая вложенные) проверяет `npm run check:licenses` (`scripts/check-licenses.mjs`), сетевой код в исходниках — `npm run check:network` (`scripts/check-no-network.mjs`).

## Попадают в приложение

| Пакет | Версия | Лицензия | Зачем | Сетевой код в рантайме |
|---|---|---|---|---|
| `svelte` | 5.57.2 | MIT | Фреймворк интерфейса (ADR 0002); встраивается сборщиком в `dist/renderer` | Нет: в собранном интерфейсе `check:network` ничего не находит |
| `electron` | 45.0.0 | MIT (Chromium и Node.js — свои лицензии, `LICENSES.chromium.html` в сборке) | Платформа приложения (ADR 0005). Числится в devDependencies: в сборку попадает его исполняемый файл, а не npm-пакет | Сетевые возможности Chromium есть; их выключение — задача 0.3 (ADR 0007). Сам npm-пакет скачивает бинарник Electron с GitHub только при разработке |

Вложенные зависимости `svelte` (`acorn`, `clsx`, `esm-env`, `devalue`, `esrap` и др.) — MIT и Apache-2.0; полный список выводит `npm run check:licenses`, точные версии — в `package-lock.json`. В `app.asar` попадает только собранный `dist/`, папка `node_modules` в сборку не копируется (`electron-builder.yml`).

## Только для разработки

| Пакет | Версия | Лицензия | Зачем | Сетевой код в рантайме |
|---|---|---|---|---|
| `typescript` | 6.0.3 | Apache-2.0 | Проверка типов (strict). Не 7.x: `typescript-eslint` 8.72 и `svelte-check` поддерживают TypeScript до 6.x | только разработка |
| `vite` | 8.3.5 | MIT | Сборка интерфейса | только разработка |
| `@sveltejs/vite-plugin-svelte` | 7.3.1 | MIT | Компиляция Svelte в Vite | только разработка |
| `esbuild` | 0.28.2 | MIT | Сборка ядра (main) и моста (preload) | только разработка |
| `svelte-check` | 4.7.7 | MIT | Проверка типов в `.svelte` | только разработка |
| `vitest` | 5.0.5 | MIT | Модульные тесты | только разработка |
| `@playwright/test` | 1.64.1 | Apache-2.0 | Сценарные тесты: запуск Electron через `_electron` | только разработка |
| `electron-builder` | 26.18.0 | MIT | Портативная сборка и установщик; при сборке скачивает Electron и инструменты NSIS с GitHub | только разработка |
| `eslint` | 10.13.0 | MIT | Линтер (flat config) | только разработка |
| `@eslint/js` | 10.0.1 | MIT | Базовые правила ESLint | только разработка |
| `typescript-eslint` | 8.72.0 | MIT | Правила и парсер TypeScript для ESLint | только разработка |
| `eslint-plugin-svelte` | 3.23.1 | MIT | Правила ESLint для `.svelte` | только разработка |
| `globals` | 17.13.0 | MIT | Списки глобальных имён (Node.js, браузер) для ESLint | только разработка |
| `@types/node` | 24.19.2 | MIT | Типы Node.js 24 (версия Node в Electron 45) | только разработка |
