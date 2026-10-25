import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { applyTheme, resolveTheme, type SystemScheme } from './theme.ts';

const ROOT = path.join(import.meta.dirname, '..', '..');
const tokensMd = readFileSync(path.join(ROOT, 'docs', 'design', 'tokens.md'), 'utf8');
const tokensCss = readFileSync(path.join(import.meta.dirname, 'tokens.css'), 'utf8');

type Rgba = [number, number, number, number];

/** «#4A55C8» или «#0F1114 / 32%» из tokens.md. */
function fromDesign(value: string): Rgba {
  const match = /^#([0-9A-Fa-f]{6})(?:\s*\/\s*(\d+)%)?$/.exec(value.trim());
  if (!match?.[1]) throw new Error(`Не цвет: ${value}`);
  const hex = match[1];
  const alpha = match[2] ? Number(match[2]) / 100 : 1;
  return [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16)).concat(alpha) as Rgba;
}

/** «#4a55c8» или «rgb(15 17 20 / 32%)» из CSS. */
function fromCss(value: string): Rgba {
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  if (hex?.[1]) return fromDesign(`#${hex[1]}`);
  const rgb = /^rgb\((\d+) (\d+) (\d+) \/ (\d+)%\)$/.exec(value);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), Number(rgb[4]) / 100];
  throw new Error(`Не цвет: ${value}`);
}

/** Строки таблицы цветов: имя, светлая, тёмная. Только то, что переносится в приложение. */
function designTokens(): { name: string; light: string; dark: string }[] {
  const section = tokensMd.split('## Цвета')[1]?.split('\n## ')[0] ?? '';
  const rows: { name: string; light: string; dark: string }[] = [];
  for (const line of section.split('\n')) {
    const cells = line.split('|').map((cell) => cell.trim());
    const name = /^`([a-z/-]+)`$/.exec(cells[1] ?? '')?.[1];
    if (!name || name.startsWith('pointer/') || /^accent\/(teal|copper)\//.test(name)) continue;
    rows.push({ name, light: cells[2] ?? '', dark: cells[3] ?? '' });
  }
  return rows;
}

/** Объявления `--имя: значение` внутри блока с селектором. */
function block(selector: string): Map<string, string> {
  const start = tokensCss.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`Нет блока ${selector}`);
  const body = tokensCss.slice(start, tokensCss.indexOf('}', start));
  return new Map([...body.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/g)].map((m) => [m[1] ?? '', (m[2] ?? '').trim()]));
}

describe('переменные темы из tokens.md', () => {
  const tokens = designTokens();
  const light = block(':root');
  const dark = block(":root[data-theme='dark']");

  it('в таблице дизайна найдены все цвета', () => {
    expect(tokens.length).toBe(30);
    expect(tokens.map((token) => token.name)).toContain('accent/indigo/base');
  });

  it.each(tokens)('$name — светлая и тёмная тема', ({ name, light: lightValue, dark: darkValue }) => {
    const variable = `--${name.replaceAll('/', '-')}`;
    expect(fromCss(light.get(variable) ?? '')).toEqual(fromDesign(lightValue));
    // «то же» в тёмном столбце — значение светлой темы.
    const expectedDark = darkValue === 'то же' ? lightValue : darkValue;
    expect(fromCss(dark.get(variable) ?? '')).toEqual(fromDesign(expectedDark));
  });

  it('псевдонимы акцента ведут на индиго', () => {
    expect(light.get('--accent')).toBe('var(--accent-indigo-base)');
    expect(light.get('--accent-text')).toBe('var(--accent-indigo-text)');
    expect(light.get('--accent-subtle')).toBe('var(--accent-indigo-subtle)');
  });

  it('варианты, не вошедшие в выпуск, в приложение не перенесены', () => {
    expect(tokensCss).not.toMatch(/--accent-(teal|copper)|--pointer-/);
  });

  it('шрифты — только локальные файлы', () => {
    const fonts = readFileSync(path.join(import.meta.dirname, 'fonts.css'), 'utf8');
    const urls = [...fonts.matchAll(/url\(([^)]+)\)/g)].map((m) => m[1]);
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) expect(url).toMatch(/^'\.\/fonts\/[\w-]+\.woff2'$/);
  });
});

class FakeScheme implements SystemScheme {
  matches: boolean;
  listeners = new Set<() => void>();

  constructor(dark: boolean) {
    this.matches = dark;
  }

  addEventListener(_type: 'change', listener: () => void): void {
    this.listeners.add(listener);
  }

  removeEventListener(_type: 'change', listener: () => void): void {
    this.listeners.delete(listener);
  }

  set(dark: boolean): void {
    this.matches = dark;
    for (const listener of this.listeners) listener();
  }
}

describe('режимы темы', () => {
  it.each([
    ['system', false, 'light'],
    ['system', true, 'dark'],
    ['light', true, 'light'],
    ['dark', false, 'dark'],
  ] as const)('%s при тёмной системе=%s → %s', (mode, systemDark, expected) => {
    expect(resolveTheme(mode, systemDark)).toBe(expected);
  });

  it('«Как в системе» следует за сменой темы системы, пока не отписались', () => {
    const root = { dataset: {} as DOMStringMap };
    const scheme = new FakeScheme(false);

    const stop = applyTheme('system', root, scheme);
    expect(root.dataset.theme).toBe('light');

    scheme.set(true);
    expect(root.dataset.theme).toBe('dark');

    stop();
    scheme.set(false);
    expect(root.dataset.theme).toBe('dark');
    expect(scheme.listeners.size).toBe(0);
  });

  it('явная тема не зависит от системы', () => {
    const root = { dataset: {} as DOMStringMap };
    const scheme = new FakeScheme(true);

    applyTheme('light', root, scheme);
    scheme.set(true);

    expect(root.dataset.theme).toBe('light');
    expect(scheme.listeners.size).toBe(0);
  });
});
