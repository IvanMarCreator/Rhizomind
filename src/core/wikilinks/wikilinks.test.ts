// Разбор вики-ссылок интерфейса против общих образцов tests/fixtures/wikilinks/:
// parse.json проверяет и Rust (src-tauri/src/links), text.json — только этот модуль.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { findWikilinks, parseTarget, type ParsedTarget, type WikiLink } from './index.ts';

const FIXTURES = join(import.meta.dirname, '..', '..', '..', 'tests', 'fixtures', 'wikilinks');

interface Fixture<C> {
  format_version: number;
  cases: C[];
}

function readFixture<C>(file: string): Fixture<C> {
  return JSON.parse(readFileSync(join(FIXTURES, file), 'utf8')) as Fixture<C>;
}

const parseCases = readFixture<{ input: string; note: string; expected: ParsedTarget }>('parse.json');
const textCases = readFixture<{ text: string; note: string; links: WikiLink[] }>('text.json');

describe('parseTarget: общие образцы parse.json', () => {
  it('формат образцов — версия 1', () => {
    expect(parseCases.format_version).toBe(1);
    expect(parseCases.cases.length).toBeGreaterThanOrEqual(40);
  });

  it.each(parseCases.cases.map((c) => [c.note, c] as const))('%s', (_note, c) => {
    expect(parseTarget(c.input)).toEqual(c.expected);
  });
});

describe('findWikilinks: образцы text.json', () => {
  it('формат образцов — версия 1', () => {
    expect(textCases.format_version).toBe(1);
    expect(textCases.cases.length).toBeGreaterThanOrEqual(25);
  });

  it.each(textCases.cases.map((c) => [c.note, c] as const))('%s', (_note, c) => {
    expect(findWikilinks(c.text)).toEqual(c.links);
  });

  it('raw совпадает с текстом по смещениям', () => {
    for (const c of textCases.cases) {
      for (const link of findWikilinks(c.text)) {
        expect(c.text.slice(link.from, link.to)).toBe(link.raw);
      }
    }
  });

  it('не падает и не зацикливается на произвольном тексте', () => {
    const alphabet = ['[', ']', '`', '~', '|', '#', '/', '!', '\n', '\r', ' ', '\t', '-', '1', '.', 'а', '😀'];
    let seed = 12345;
    const next = (): number => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed;
    };
    for (let n = 0; n < 2000; n += 1) {
      const length = next() % 60;
      let text = '';
      for (let i = 0; i < length; i += 1) text += alphabet[next() % alphabet.length];
      for (const link of findWikilinks(text)) {
        expect(text.slice(link.from, link.to)).toBe(link.raw);
        expect(link.raw.endsWith(']]')).toBe(true);
      }
    }
  });
});
