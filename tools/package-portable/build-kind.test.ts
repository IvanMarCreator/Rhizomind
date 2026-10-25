import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { WEBDRIVER_BUILD_MARKER, isWebdriverBuild } from './build-kind.ts';

describe('package:portable: тестовая сборка не упаковывается', () => {
  it('узнаёт тестовую сборку по пометке в заголовке окна (UTF-8 в исполняемом файле)', () => {
    const exe = Buffer.concat([
      Buffer.from([0x4d, 0x5a, 0, 0]),
      Buffer.from(`...Rhizomind — ${WEBDRIVER_BUILD_MARKER}...`, 'utf8'),
    ]);
    expect(isWebdriverBuild(exe)).toBe(true);
  });

  it('выпускную сборку пропускает', () => {
    expect(isWebdriverBuild(Buffer.from('MZ...Rhizomind...webdriver...', 'utf8'))).toBe(false);
  });

  it('пометка совпадает с константой ядра', () => {
    const lib = readFileSync(path.join(import.meta.dirname, '..', '..', 'src-tauri', 'src', 'lib.rs'), 'utf8');
    expect(lib).toContain(`pub const WEBDRIVER_BUILD_MARKER: &str = "${WEBDRIVER_BUILD_MARKER}";`);
  });
});
