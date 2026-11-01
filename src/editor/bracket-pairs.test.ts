// Парная квадратная скобка (решение владельца 95, задача 2.18): правила по
// отдельности, на состоянии редактора со всеми расширениями. Курсор в
// образцах — `|`, выделение — между двумя `|`.
import { undo } from '@codemirror/commands';
import { EditorSelection, EditorState, type TransactionSpec } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { closeBracket, deletePair, openBracket, taskSpace } from './bracket-pairs.ts';
import { createEditorState } from './codemirror.ts';

function stateOf(marked: string, readOnly = false): EditorState {
  const cursor = marked.indexOf('|');
  const head = marked.lastIndexOf('|');
  const text = marked.replaceAll('|', '');
  const selection = cursor === head ? EditorSelection.cursor(cursor) : EditorSelection.range(cursor, head - 1);
  return createEditorState(text, { readOnly }).update({ selection }).state;
}

function marked(state: EditorState): string {
  const { from, to } = state.selection.main;
  const doc = state.doc.toString();
  if (from === to) return `${doc.slice(0, from)}|${doc.slice(from)}`;
  return `${doc.slice(0, from)}|${doc.slice(from, to)}|${doc.slice(to)}`;
}

type Rule = (state: EditorState) => TransactionSpec | null;

/** Применить правила по очереди; `null` у правила — обычный ввод символа `fallback`. */
function run(state: EditorState, rule: Rule, fallback: string | null): EditorState {
  const spec = rule(state);
  if (spec) return state.update(spec).state;
  if (fallback === null) return state;
  const { from, to } = state.selection.main;
  return state.update({
    changes: { from, to, insert: fallback },
    selection: { anchor: from + fallback.length },
    userEvent: 'input.type',
  }).state;
}

/** Набрать текст посимвольно, как с клавиатуры: `[`, `]`, пробел — через правила. */
function typed(state: EditorState, text: string): EditorState {
  let current = state;
  for (const ch of text) {
    if (ch === '[') current = run(current, openBracket, ch);
    else if (ch === ']') current = run(current, closeBracket, ch);
    else if (ch === ' ') current = run(current, taskSpace, ch);
    else current = run(current, () => null, ch);
  }
  return current;
}

const apply = (text: string, rule: Rule): string | null => {
  const state = stateOf(text);
  const spec = rule(state);
  return spec ? marked(state.update(spec).state) : null;
};

describe('`[` вставляет пару', () => {
  it.each([
    ['пустая строка', '|', '[|]'],
    ['конец строки', 'текст |', 'текст [|]'],
    ['перед пробелом', '|  хвост', '[|]  хвост'],
    ['перед закрывающей скобкой — `[[]]`', '[|]', '[[|]]'],
    ['перед `)`', '(|)', '([|])'],
    ['перед знаком препинания', 'а|.', 'а[|].'],
    ['после маркера задачи', '- |', '- [|]'],
  ])('%s', (_name, before, after) => {
    expect(apply(before, openBracket)).toBe(after);
  });

  it.each([
    ['перед буквой — одна `[` (обычный ввод)', '|слово'],
    ['перед цифрой', '|42'],
    ['после `\\` — экранированная скобка', '\\|'],
    ['в коде в строке', 'см. `код |`'],
    ['в блоке кода', '```\nкод |\n```'],
    ['на строке ограды', '```|'],
    ['в блоке свойств', '---\ntags: |\n---\nтекст'],
  ])('%s', (_name, before) => {
    expect(apply(before, openBracket)).toBeNull();
  });

  it('после кода в строке — пара', () => {
    expect(apply('`код` |', openBracket)).toBe('`код` [|]');
  });

  it('при выделении — оборачивает, выделение внутри', () => {
    expect(apply('см. |заметку| тут', openBracket)).toBe('см. [|заметку|] тут');
  });

  it('выделение в несколько строк — оборачивает целиком', () => {
    expect(apply('|а\nб|', openBracket)).toBe('[|а\nб|]');
  });

  it('только чтение — ничего', () => {
    expect(openBracket(stateOf('|', true))).toBeNull();
  });

  it('Ctrl+Z отменяет пару одним шагом', () => {
    const state = typed(stateOf('текст |'), '[');
    expect(marked(state)).toBe('текст [|]');
    let restored = state;
    undo({ state, dispatch: (tr) => (restored = tr.state) });
    expect(restored.doc.toString()).toBe('текст ');
  });
});

describe('`]` перед вставленной `]` — переход за неё', () => {
  it('пара: `[|]` → `[]|`', () => {
    expect(marked(typed(stateOf('|'), '[]'))).toBe('[]|');
  });

  it('вики-ссылка: `[[имя]]` набирается без лишних скобок', () => {
    expect(marked(typed(stateOf('|'), '[[Имя]]'))).toBe('[[Имя]]|');
  });

  it('ссылка Markdown: `[текст](адрес)`', () => {
    expect(marked(typed(stateOf('|'), '[текст](адрес)'))).toBe('[текст](адрес)|');
  });

  it('чужая `]` (не вставленная парой) не перескакивается', () => {
    expect(apply('[а|]', closeBracket)).toBeNull();
  });

  it('после ухода с строки вставленная `]` забывается', () => {
    let state = typed(stateOf('|\nниже'), '[');
    state = state.update({ selection: { anchor: state.doc.length } }).state;
    state = state.update({ selection: { anchor: 1 } }).state;
    expect(closeBracket(state)).toBeNull();
  });

  it('`]`, удалённая и вставленная заново, — уже не «своя»', () => {
    let state = typed(stateOf('|'), '[');
    state = state.update({ changes: { from: 1, to: 2, insert: ']' } }).state;
    expect(closeBracket(state.update({ selection: { anchor: 1 } }).state)).toBeNull();
  });
});

describe('Backspace между пустыми скобками', () => {
  it.each([
    ['`[|]` — обе', '[|]', '|'],
    ['`[[|]]` — внутренняя пара', '[[|]]', '[|]'],
    ['в тексте', 'а [|] б', 'а | б'],
  ])('%s', (_name, before, after) => {
    expect(apply(before, deletePair)).toBe(after);
  });

  it.each([
    ['не пустые', '[а|]'],
    ['курсор не между', '[]|'],
    ['выделение', '[|а|]'],
    ['в коде в строке', '`[|]`'],
  ])('%s — обычный Backspace', (_name, before) => {
    expect(apply(before, deletePair)).toBeNull();
  });

  it('двойной Backspace в `[[|]]` — пусто', () => {
    let state = stateOf('[[|]]');
    state = state.update(deletePair(state) ?? {}).state;
    state = state.update(deletePair(state) ?? {}).state;
    expect(marked(state)).toBe('|');
  });
});

describe('пробел в `[]` у маркера списка — готовая задача', () => {
  it.each([
    ['маркер -', '- [|]', '- [ ] |'],
    ['маркер *', '* [|]', '* [ ] |'],
    ['маркер +', '+ [|]', '+ [ ] |'],
    ['нумерованный', '1. [|]', '1. [ ] |'],
    ['нумерованный со скобкой', '2) [|]', '2) [ ] |'],
    ['вложенный', '\t- [|]', '\t- [ ] |'],
    ['в цитате', '> - [|]', '> - [ ] |'],
    ['текст после — без второго пробела', '- [|] дело', '- [ ] |дело'],
    ['текст вплотную — пробел добавляется', '- [|]дело', '- [ ] |дело'],
  ])('%s', (_name, before, after) => {
    expect(apply(before, taskSpace)).toBe(after);
  });

  it.each([
    ['не у маркера', 'текст [|]'],
    ['пара не пустая', '- [а|]'],
    ['без маркера', '[|]'],
    ['маркер без пробела', '-[|]'],
    ['в блоке кода', '```\n- [|]\n```'],
  ])('%s — обычный пробел', (_name, before) => {
    expect(apply(before, taskSpace)).toBeNull();
  });

  it('`- [` + пробел + текст → `- [ ] текст`', () => {
    expect(marked(typed(stateOf('|'), '- [ купить'))).toBe('- [ ] купить|');
  });

  it('отметка `x`: `- [x]` набирается, `]` перескакивает', () => {
    expect(marked(typed(stateOf('|'), '- [x] готово'))).toBe('- [x] готово|');
  });
});
