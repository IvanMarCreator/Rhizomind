// Разбор тегов интерфейса против общих образцов tests/fixtures/tags/ — тех же,
// что проверяет ядро (`markdown::tests::fixtures_tags` и `fixtures_tag_keys`,
// src-tauri/src/markdown/tests.rs): обе стороны обязаны давать одинаковый
// результат (С2-3.3, С2-5.3, Р17).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { findTags, isTagName, tagKey, tagsInParagraph, LineClassifier, type TextTag } from './index.ts';

const FIXTURES = join(import.meta.dirname, '..', '..', '..', 'tests', 'fixtures', 'tags');

interface Fixture<C> {
  format_version: number;
  cases: C[];
}

function readFixture<C>(file: string): Fixture<C> {
  return JSON.parse(readFileSync(join(FIXTURES, file), 'utf8')) as Fixture<C>;
}

interface TextCase {
  rule: string;
  kind: 'tag' | 'not_tag';
  note: string;
  text: string;
  tags: Array<{ raw: string; name: string; from: number; to: number }>;
}

const textCases = readFixture<TextCase>('text.json');
const keyCases = readFixture<{ note: string; a: string; b: string; same: boolean }>('key.json');

describe('fixtures: tags', () => {
  it('формат образцов — версия 1, не меньше 60 случаев', () => {
    expect(textCases.format_version).toBe(1);
    expect(textCases.cases.length).toBeGreaterThanOrEqual(60);
  });

  it.each(textCases.cases.map((c) => [`${c.rule} ${c.note}`, c] as const))('%s', (_name, c) => {
    const expected: TextTag[] = c.tags.map(({ name, from, to }) => ({ name, from, to }));
    const got = findTags(c.text);
    expect(got).toEqual(expected);
    for (const tag of got) expect(c.text.slice(tag.from, tag.to)).toBe(`#${tag.name}`);
  });
});

describe('fixtures: tag keys', () => {
  it.each(keyCases.cases.map((c) => [c.note, c] as const))('%s', (_note, c) => {
    expect(tagKey(c.a) === tagKey(c.b)).toBe(c.same);
  });

  it('ключ без «#» и с ним — один', () => {
    expect(tagKey('#Проект')).toBe(tagKey('проект'));
  });
});

describe('имя тега целиком (значения блока свойств — тем же правилом)', () => {
  it.each([
    ['проект', true],
    ['работа/отчёт', true],
    ['2026-11-01', true],
    ['_', true],
    ['2026', false],
    ['/работа', false],
    ['работа/', false],
    ['a//b', false],
    ['проект.', false],
    ['', false],
    ['a'.repeat(200), true],
    ['a'.repeat(201), false],
    // 200 «й» в разложенной форме (400 кодовых точек) — после NFC 200.
    ['й'.normalize('NFD').repeat(200), true],
    // U+0344 раскладывается при NFC в две точки: 101 знак — 202 после NFC.
    ['̈́'.repeat(101), false],
  ])('%s → %s', (name, valid) => {
    expect(isTagName(name)).toBe(valid);
  });
});

describe('разбор участка текста (для редактора)', () => {
  it('теги абзаца по строкам: код в строке через перенос не даёт тега', () => {
    const text = 'начало `код\n#не_тег` и #тег';
    const lines = [
      { from: 0, to: 11 },
      { from: 12, to: text.length },
    ];
    expect(tagsInParagraph(text, lines)).toEqual([{ name: 'тег', from: 23, to: 27 }]);
  });

  it('классификатор строк: код с отступом после пустой строки, не в списке', () => {
    const lines = new LineClassifier();
    expect(lines.classify('Абзац')).toBe('text');
    expect(lines.classify('    продолжение')).toBe('text');
    expect(lines.classify('')).toBe('blank');
    expect(lines.classify('    код')).toBe('code');
    expect(lines.classify('```')).toBe('code');
    expect(lines.classify('#в_ограде')).toBe('code');
    expect(lines.classify('```')).toBe('code');
    expect(lines.classify('- пункт')).toBe('text');
    expect(lines.classify('')).toBe('blank');
    expect(lines.classify('    продолжение пункта')).toBe('text');
  });

  it('строка ~10 000 символов из `[#a](` — разбор линейный', () => {
    // Лучший из замеров: вчетверо длиннее — около ×4 (квадратичный — ×16).
    const time = (n: number) => {
      const text = '[#a]('.repeat(n);
      findTags(text);
      let best = Infinity;
      for (let k = 0; k < 7; k++) {
        const start = performance.now();
        findTags(text);
        best = Math.min(best, performance.now() - start);
      }
      return best;
    };
    expect(time(8000) / Math.max(time(2000), 0.05)).toBeLessThan(10);
  });
});
