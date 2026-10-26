// package:msi и package:nsis: перед упаковкой (build.beforeBundleCommand в
// src-tauri/tauri.conf.json) проверка отказывается упаковывать тестовую сборку.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { WEBDRIVER_BUILD_MARKER } from './build-kind.ts';

const root = path.join(import.meta.dirname, '..', '..');
const cli = path.join(import.meta.dirname, 'check-release-exe.ts');
const exeName = process.platform === 'win32' ? 'rhizomind.exe' : 'rhizomind';

function run(targetDir: string): number {
  try {
    execFileSync(process.execPath, [cli], { env: { ...process.env, CARGO_TARGET_DIR: targetDir }, stdio: 'pipe' });
    return 0;
  } catch (error) {
    return (error as { status: number }).status;
  }
}

describe('проверка перед упаковкой установщиков', () => {
  let dir = '';
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  const withExe = (content: string): string => {
    dir = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-check-exe-'));
    mkdirSync(path.join(dir, 'release'), { recursive: true });
    writeFileSync(path.join(dir, 'release', exeName), Buffer.from(content, 'utf8'));
    return dir;
  };

  it('тестовую сборку (пометка webdriver) не пропускает', () => {
    expect(run(withExe(`MZ...Rhizomind — ${WEBDRIVER_BUILD_MARKER}...`))).toBe(1);
  });

  it('выпускную сборку пропускает', () => {
    expect(run(withExe('MZ...Rhizomind...'))).toBe(0);
  });

  it('без собранного файла — ошибка', () => {
    dir = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-check-exe-'));
    expect(run(dir)).toBe(1);
  });

  it('подключена в tauri.conf.json как build.beforeBundleCommand', () => {
    const config = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')) as {
      build: { beforeBundleCommand?: string };
    };
    expect(config.build.beforeBundleCommand).toBe('node tools/package-portable/check-release-exe.ts');
  });
});
