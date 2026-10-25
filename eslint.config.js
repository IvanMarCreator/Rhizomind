import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

export default defineConfig(
  { ignores: ['dist/', 'release/', 'coverage/', 'test-results/', 'prototype/', 'src-tauri/', 'vendor/'] },
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
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    },
  },
);
