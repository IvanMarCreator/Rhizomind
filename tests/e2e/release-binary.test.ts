// Выпускной исполняемый файл (npm run build) не содержит кода тестовой сборки
// (функция cargo `webdriver`): ни строки флага порта отладки WebView2, ни
// пометки тестовой сборки. Код порта отладки собирается только в тестах и в
// тестовой сборке (`cfg(any(test, feature = "webdriver"))`, src-tauri/src/security/).
import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { applicationPath } from './driver.ts';

describe('выпускной исполняемый файл', () => {
  const exe = applicationPath('release');

  it('собран (npm run build)', () => {
    expect(existsSync(exe), `${exe}: нужна выпускная сборка`).toBe(true);
  });

  it('без флага порта отладки и без пометки тестовой сборки', () => {
    const binary = readFileSync(exe);
    expect(binary.includes(Buffer.from('--remote-debugging-port=', 'utf8'))).toBe(false);
    expect(binary.includes(Buffer.from('ТЕСТОВАЯ СБОРКА (webdriver)', 'utf8'))).toBe(false);
  });

  // Задача 1.3: открытие тестового хранилища без окна выбора папки и свой
  // список хранилищ есть только в тестовой сборке (src-tauri/src/windows/mod.rs).
  it('без переменных окружения сценарных тестов', () => {
    const binary = readFileSync(exe);
    for (const name of ['RHIZOMIND_E2E_VAULT', 'RHIZOMIND_E2E_PROFILE_DIR']) {
      expect(binary.includes(Buffer.from(name, 'utf8')), name).toBe(false);
    }
  });
});
