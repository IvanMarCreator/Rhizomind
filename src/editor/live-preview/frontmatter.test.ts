// Блок свойств в живом предпросмотре (0.2-3.2, С2-3.4, решение 112): разбор
// блока и его тегов против общих образцов tests/fixtures/frontmatter/ (те же,
// что у ядра, `markdown::tests::fixtures_frontmatter`) и состояние
// «свёрнут / раскрыт» без окна.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { EditorSelection, type EditorState } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { frontmatterTags, parseFrontmatter, propertiesView } from './frontmatter.ts';
import { MAX_BLOCK_TEXT } from './markdown.ts';

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

const NOTE = '---\ntags: [проект, идея]\ntitle: Встреча\n---\nТекст заметки\nвторая строка';
const BLOCK_END = NOTE.indexOf('---\nТекст') + 3;

function live(text: string, anchor = text.length, head = anchor): EditorState {
  return createEditorState(text, { mode: 'live_preview' }).update({ selection: EditorSelection.single(anchor, head) }).state;
}

describe('блок свойств: свёрнут или раскрыт', () => {
  it('блок свёрнут в строку Свойства с тегами', () => {
    const view = propertiesView(live(NOTE));
    expect(view).toEqual({ from: 0, to: BLOCK_END, expanded: false, tags: ['проект', 'идея'] });
  });

  it('курсор в начале текста (перед строкой «Свойства») — блок свёрнут: курсор виден у её края', () => {
    expect(propertiesView(live(NOTE, 0))?.expanded).toBe(false);
  });

  it('курсор в любом месте блока, включая конец закрывающей строки, — раскрыт', () => {
    for (const pos of [1, 4, 10, BLOCK_END - 1, BLOCK_END]) {
      expect(propertiesView(live(NOTE, pos))?.expanded, `позиция ${pos}`).toBe(true);
    }
  });

  it('курсор ушёл — свернулось', () => {
    const open = live(NOTE, 6);
    expect(propertiesView(open)?.expanded).toBe(true);
    const left = open.update({ selection: { anchor: BLOCK_END + 1 } }).state;
    expect(propertiesView(left)?.expanded).toBe(false);
  });

  it('ввод в раскрытом блоке его не сворачивает, текст — как набран', () => {
    let state = live(NOTE, NOTE.indexOf(']'));
    for (const ch of ', план') {
      const at = state.selection.main.head;
      state = state.update({ changes: { from: at, insert: ch }, selection: { anchor: at + ch.length }, userEvent: 'input.type' }).state;
      expect(propertiesView(state)?.expanded).toBe(true);
    }
    expect(state.doc.toString()).toBe(NOTE.replace('идея]', 'идея, план]'));
  });

  it('выделение через блок (якорь ниже, голова в начале текста) — блок свёрнут, выделен исходный текст', () => {
    const state = live(NOTE, NOTE.length, 0);
    expect(propertiesView(state)?.expanded).toBe(false);
    expect(state.sliceDoc(state.selection.main.from, state.selection.main.to)).toBe(NOTE);
  });

  it('> 10 МБ — блок не распознаётся', () => {
    const big = NOTE + 'x'.repeat(MAX_BLOCK_TEXT);
    expect(big.length).toBeGreaterThan(MAX_BLOCK_TEXT);
    expect(propertiesView(live(big))).toBeNull();
    // Правка, после которой текст стал больше 10 МБ, тоже убирает строку «Свойства».
    const small = live(NOTE + 'x'.repeat(MAX_BLOCK_TEXT - NOTE.length));
    expect(propertiesView(small)).not.toBeNull();
    const grown = small.update({ changes: { from: small.doc.length, insert: 'yy' } }).state;
    expect(propertiesView(grown)).toBeNull();
  });

  it('в исходном режиме виден целиком', () => {
    const state = createEditorState(NOTE, { mode: 'source' });
    expect(propertiesView(state)).toBeNull();
  });

  it('незакрытый блок и `---` не в начале — не блок', () => {
    expect(propertiesView(live('---\ntags: [проект]\nТекст'))).toBeNull();
    expect(propertiesView(live('Текст\n---\ntags: [проект]\n---\n'))).toBeNull();
  });
});
