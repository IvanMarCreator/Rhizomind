// Блок свойств и теги из него против общих образцов tests/fixtures/frontmatter/
// — тех же, что проверяет ядро (`markdown::tests::fixtures_frontmatter`).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { frontmatterTags, parseFrontmatter } from './frontmatter.ts';

const FIXTURES = join(import.meta.dirname, '..', '..', '..', 'tests', 'fixtures', 'frontmatter', 'tags.json');

interface FrontmatterCase {
  rule: string;
  note: string;
  text: string;
  block: { from: number; to: number } | null;
  tags: string[];
}

const cases = (JSON.parse(readFileSync(FIXTURES, 'utf8')) as { cases: FrontmatterCase[] }).cases;

describe('fixtures: frontmatter', () => {
  it('образцы прочитаны', () => {
    expect(cases.length).toBeGreaterThanOrEqual(15);
  });

  it.each(cases.map((c) => [`${c.rule} ${c.note}`, c] as const))('%s', (_name, c) => {
    const parsed = parseFrontmatter(c.text);
    expect(parsed ? { from: parsed.from, to: parsed.to } : null).toEqual(c.block ? { from: c.block.from, to: c.block.to } : null);
    expect(parsed?.tags ?? []).toEqual(c.tags);
  });
});

describe('теги блока для строки «Свойства»', () => {
  it('повторы одного тега (с точностью до регистра) — один раз, в первом написании', () => {
    expect(frontmatterTags('---\ntags: [проект, Проект, идея]\ntag: проект\n---')).toEqual(['проект', 'идея']);
  });
});
