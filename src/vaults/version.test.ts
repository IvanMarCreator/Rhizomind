import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { APP_VERSION } from './version.ts';

describe('версия в окне «Управление хранилищами»', () => {
  it('совпадает с версией в package.json и tauri.conf.json', () => {
    const root = path.join(import.meta.dirname, '..', '..');
    const pkg = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')) as { version: string };
    const tauri = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')) as {
      version: string;
    };
    expect(APP_VERSION).toBe(pkg.version);
    expect(APP_VERSION).toBe(tauri.version);
  });
});
