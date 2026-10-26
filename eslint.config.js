import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

export default defineConfig(
  { ignores: ['dist/', '.network-check/', 'release/', 'coverage/', 'test-results/', 'prototype/', 'src-tauri/', 'vendor/'] },
  js.configs.recommended,
  ts.configs.strict,
  svelte.configs.recommended,
  {
    // Проверки и сценарные тесты работают в Node.
    files: ['tools/**', 'tests/**', '*.config.ts', '*.config.js'],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    // Интерфейс работает в WebView: только браузерные глобалы, без Node.
    files: ['src/**'],
    languageOptions: { globals: { ...globals.browser } },
  },
  {
    // Изолированный iframe: обычный скрипт без модулей (isolation pattern).
    files: ['src-isolation/**/*.js'],
    languageOptions: { sourceType: 'script', globals: { ...globals.browser } },
  },
  {
    files: ['src-isolation/**/*.test.ts'],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser, extraFileExtensions: ['.svelte'] },
    },
  },
  {
    // События ядра — только для своего окна: ядро отправляет их окну
    // хранилища (emit_to), а глобальный listen получил бы и события,
    // отправленные другим окнам. Подписка — onCoreEvent (src/api/events.ts,
    // getCurrentWebviewWindow().listen); задача 1.7, docs/SECURITY.md.
    files: ['src/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/event',
              importNames: ['listen', 'once'],
              message: 'Глобальная подписка запрещена: используйте onCoreEvent из src/api (события своего окна).',
            },
          ],
        },
      ],
    },
  },
  {
    // Окно через свойство (`el.ownerDocument.defaultView`, `view.win` у CodeMirror)
    // — тот же глобальный объект, что и window: через него доступно любое API по
    // имени. В своём коде не используем; если понадобится — узкое исключение
    // `// eslint-disable-next-line no-restricted-syntax -- <почему>` с разбором
    // в ревью безопасности (ревью 2.4, В1; docs/SECURITY.md).
    files: ['src/**', 'src-isolation/**'],
    rules: {
      'no-restricted-syntax': [
        'error',
        ...['defaultView', 'win'].flatMap((name) => [
          {
            selector: `MemberExpression[property.name='${name}'][computed=false]`,
            message: `Окно через .${name} запрещено: используйте нужное API напрямую (docs/SECURITY.md, проверка сети).`,
          },
          {
            selector: `MemberExpression[computed=true][property.value='${name}']`,
            message: `Окно через ['${name}'] запрещено: используйте нужное API напрямую (docs/SECURITY.md, проверка сети).`,
          },
        ]),
      ],
    },
  },
  {
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    },
  },
);
