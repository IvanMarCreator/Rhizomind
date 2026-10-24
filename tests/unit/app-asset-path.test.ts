import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { resolveAppAsset } from '../../src/main/app-asset-path';

const ROOT = path.resolve('/opt/rhizomind/dist/renderer');

describe('resolveAppAsset', () => {
  it('корень отдаёт index.html', () => {
    expect(resolveAppAsset(ROOT, 'app://rhizomind/')).toBe(path.join(ROOT, 'index.html'));
  });

  it('файл интерфейса отдаётся из папки интерфейса', () => {
    expect(resolveAppAsset(ROOT, 'app://rhizomind/assets/index-abc.js')).toBe(
      path.join(ROOT, 'assets', 'index-abc.js'),
    );
  });

  it('параметры запроса и якорь не влияют на путь', () => {
    expect(resolveAppAsset(ROOT, 'app://rhizomind/index.html?x=1#top')).toBe(path.join(ROOT, 'index.html'));
  });

  it.each([
    ['чужой хост', 'app://evil/index.html'],
    ['другая схема', 'file:///etc/passwd'],
    ['битая кодировка', 'app://rhizomind/%E0%A4%A'],
  ])('отказывает: %s', (_name, url) => {
    expect(resolveAppAsset(ROOT, url)).toBeNull();
  });

  it.each([
    ['..', 'app://rhizomind/../../etc/passwd'],
    ['%2e%2e', 'app://rhizomind/%2e%2e/%2e%2e/etc/passwd'],
    ['закодированный /', 'app://rhizomind/..%2f..%2f..%2fsecret.txt'],
    ['закодированный \\', 'app://rhizomind/..%5c..%5c..%5csecret.txt'],
  ])('не выпускает за папку интерфейса: %s', (_name, url) => {
    const target = resolveAppAsset(ROOT, url);
    if (target !== null) expect(target.startsWith(ROOT + path.sep)).toBe(true);
  });
});
