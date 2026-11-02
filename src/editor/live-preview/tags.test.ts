// Теги в редакторе по видимой части (0.2-3.1): тот же результат, что разбор
// всего текста (src/core/tags, общие образцы с ядром), но без разбора всего
// текста на каждое нажатие.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { findTags } from '../../core/tags/index.ts';
import { createEditorState } from '../codemirror.ts';
import { editorTags } from './tags.ts';

const FIXTURES = join(import.meta.dirname, '..', '..', '..', 'tests', 'fixtures', 'tags', 'text.json');
const cases = (JSON.parse(readFileSync(FIXTURES, 'utf8')) as { cases: Array<{ note: string; rule: string; text: string }> }).cases;

const whole = (text: string) => {
  const state = createEditorState(text, { mode: 'live_preview' });
  return editorTags(state, [{ from: 0, to: state.doc.length }]);
};

describe('теги редактора', () => {
  it.each(cases.map((c) => [`${c.rule} ${c.note}`, c.text] as const))('как разбор ядра: %s', (_name, text) => {
    expect(whole(text)).toEqual(findTags(text));
  });

  it('тег плашкой, `#` виден: участок плашки — от `#` до конца имени', () => {
    expect(whole('текст #проект, ещё')).toEqual([{ name: 'проект', from: 6, to: 13 }]);
  });

  it('тег в коде и в ссылке — не плашка', () => {
    const text = '`#код`\n\n```\n#ограда\n```\n\n[[Заметка#Заголовок]] [т](адрес#якорь) #да\n\n    #отступ';
    expect(whole(text).map((tag) => tag.name)).toEqual(['да']);
  });

  it('видимая часть посреди длинной заметки — те же теги, что во всём тексте', () => {
    const chunk = (i: number) =>
      `## Раздел ${i} #заголовок\n\nтекст #тег${i} и \`#код\`\n- пункт #пункт\n\n    #отступ_в_коде\n\n\`\`\`\n#ограда\n\`\`\`\n\n`;
    const text = Array.from({ length: 400 }, (_v, i) => chunk(i)).join('');
    const state = createEditorState(text, { mode: 'live_preview' });
    const from = state.doc.line(2001).from;
    const to = state.doc.line(2060).to;
    const expected = findTags(text).filter((tag) => tag.from >= from && tag.to <= to);
    expect(expected.length).toBeGreaterThan(10);
    expect(editorTags(state, [{ from, to }])).toEqual(expected);
  });

  it('видимая часть начинается внутри длинного блока кода — тегов в нём нет', () => {
    const text = `до #а\n\n\`\`\`\n${'#в_коде\n'.repeat(1000)}\`\`\`\n#после`;
    const state = createEditorState(text, { mode: 'live_preview' });
    const from = state.doc.line(600).from;
    expect(editorTags(state, [{ from, to: state.doc.length }])).toEqual(findTags(text).filter((tag) => tag.from >= from));
  });
});
