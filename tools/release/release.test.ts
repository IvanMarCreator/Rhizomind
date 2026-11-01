import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { checksums, DATE_PLACEHOLDER, parseTag, projectVersions, releaseNotes } from './release.ts';

const CHANGELOG = `# Изменения

Вступление.

## [Не выпущено]

## [0.2.0] — ${DATE_PLACEHOLDER}

### Добавлено
- Вторая.

## [0.1.0] — 2026-11-05

### Добавлено
- Первая.

### Исправлено
- Мелочь.

[0.1.0]: https://example.invalid
`;

describe('описание выпуска из CHANGELOG.md', () => {
  it('берёт раздел версии до следующего раздела или ссылок', () => {
    expect(releaseNotes(CHANGELOG, '0.1.0', { requireDate: true })).toBe(
      '### Добавлено\n- Первая.\n\n### Исправлено\n- Мелочь.\n',
    );
  });

  it('раздел с датой-заглушкой годится для пробной сборки, но не для выпуска по тегу', () => {
    expect(releaseNotes(CHANGELOG, '0.2.0', { requireDate: false })).toBe('### Добавлено\n- Вторая.\n');
    expect(() => releaseNotes(CHANGELOG, '0.2.0', { requireDate: true })).toThrow(/дата/);
  });

  it('нет раздела версии — ошибка, а не пустое описание', () => {
    expect(() => releaseNotes(CHANGELOG, '0.3.0', { requireDate: false })).toThrow(/0\.3\.0/);
  });

  it('пустой раздел — ошибка', () => {
    expect(() => releaseNotes('## [0.1.0] — 2026-11-05\n\n## [0.0.1] — 2026-01-01\n- x\n', '0.1.0', { requireDate: true })).toThrow(
      /пуст/,
    );
  });

  it('в настоящем CHANGELOG.md есть раздел текущей версии', () => {
    const root = path.join(import.meta.dirname, '..', '..');
    const changelog = readFileSync(path.join(root, 'CHANGELOG.md'), 'utf8');
    const { version } = projectVersions(root);
    expect(releaseNotes(changelog, version, { requireDate: false }).length).toBeGreaterThan(200);
  });
});

describe('номер версии', () => {
  it('тег v0.1.0 — выпуск, v0.1.0-rc.N — кандидат той же версии; другое — ошибка', () => {
    expect(parseTag('v0.1.0')).toEqual({ version: '0.1.0', candidate: false });
    expect(parseTag('v0.1.0-rc.1')).toEqual({ version: '0.1.0', candidate: true });
    expect(parseTag('v0.1.0-rc.12')).toEqual({ version: '0.1.0', candidate: true });
    for (const bad of ['0.1.0', 'v0.1', 'v0.1.0-rc', 'v0.1.0-rc.0', 'v0.1.0-beta.1', 'v0.1.0-rc.1x']) {
      expect(() => parseTag(bad), bad).toThrow();
    }
  });

  it('package.json, tauri.conf.json и src-tauri/Cargo.toml — одна версия', () => {
    const root = path.join(import.meta.dirname, '..', '..');
    const versions = projectVersions(root);
    expect(versions.version).toBe('0.1.0');
  });

  it('разные версии в файлах проекта — ошибка с перечнем', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-release-'));
    writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ version: '0.1.0' }));
    mkdirSync(path.join(dir, 'src-tauri'));
    writeFileSync(path.join(dir, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ version: '0.1.0' }));
    writeFileSync(path.join(dir, 'src-tauri', 'Cargo.toml'), '[package]\nname = "rhizomind"\nversion = "0.2.0"\n');
    expect(() => projectVersions(dir)).toThrow(/Cargo\.toml: 0\.2\.0/);
  });
});

describe('контрольные суммы', () => {
  it('SHA-256 в формате sha256sum, по имени файла', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-sums-'));
    writeFileSync(path.join(dir, 'b.txt'), 'abc');
    writeFileSync(path.join(dir, 'a.txt'), '');
    expect(checksums(dir, ['b.txt', 'a.txt'])).toBe(
      'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.txt\n' +
        'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  b.txt\n',
    );
  });
});
