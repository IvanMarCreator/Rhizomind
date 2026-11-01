// Парная квадратная скобка (решение владельца 95, задача 2.18): правила по
// отдельности, на состоянии редактора со всеми расширениями. Курсор в
// образцах — `|`, выделение — между двумя `|`.
import { isolateHistory, redo, undo } from '@codemirror/commands';
import { EditorSelection, EditorState, type TransactionSpec } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { closeBracket, deletePair, enterInPair, openBracket, taskSpace } from './bracket-pairs.ts';
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
    ['после `\\\\` — косая экранирована, скобка — нет: пара (QA 2.18)', '\\\\|', '\\\\[|]'],
    ['после `\\\\\\\\` — две экранированные косые', '\\\\\\\\|', '\\\\\\\\[|]'],
  ])('%s', (_name, before, after) => {
    expect(apply(before, openBracket)).toBe(after);
  });

  it.each([
    ['перед буквой — одна `[` (обычный ввод)', '|слово'],
    ['перед цифрой', '|42'],
    ['после `\\` — экранированная скобка', '\\|'],
    ['после `\\\\\\` — косая и экранированная скобка', '\\\\\\|'],
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

/** Выполнить команду истории над состоянием. */
function history(state: EditorState, cmd: typeof undo): EditorState {
  let next = state;
  cmd({ state, dispatch: (tr) => (next = tr.state) });
  return next;
}

describe('Ctrl+Z и Ctrl+Y — скобка снова «своя» (QA 2.18, задача 2.21)', () => {
  // После Ctrl+Y курсор CodeMirror ставит за вставленный текст: скобку
  // проверяем, вернув курсор к ней (стрелкой влево).
  const back = (state: EditorState, by: number): EditorState =>
    state.update({ selection: { anchor: state.selection.main.head - by } }).state;

  it('пара отменена и возвращена: `]` перескакивается', () => {
    let state = typed(stateOf('текст |'), '[');
    state = back(history(history(state, undo), redo), 1);
    expect(marked(state)).toBe('текст [|]');
    expect(marked(run(state, closeBracket, ']'))).toBe('текст []|');
  });

  it('пара с текстом отменена и возвращена: `]` перескакивается', () => {
    let state = typed(stateOf('|'), '[имя');
    state = history(history(state, undo), redo);
    state = state.update({ selection: { anchor: 4 } }).state;
    expect(marked(state)).toBe('[имя|]');
    expect(marked(run(state, closeBracket, ']'))).toBe('[имя]|');
  });

  it('вики-ссылка: обе `]]` перескакиваются после Ctrl+Z, Ctrl+Y', () => {
    let state = typed(stateOf('|'), '[[Имя');
    state = history(history(state, undo), redo);
    state = state.update({ selection: { anchor: 5 } }).state;
    expect(marked(typed(state, ']]'))).toBe('[[Имя]]|');
  });

  it('Backspace по пустой паре отменён: пара снова «своя»', () => {
    let state = typed(stateOf('|'), '[');
    state = state.update({ annotations: isolateHistory.of('full') }).state;
    state = state.update(deletePair(state) ?? {}).state;
    state = history(state, undo);
    expect(marked(state)).toBe('[|]');
    expect(marked(run(state, closeBracket, ']'))).toBe('[]|');
  });

  it('чужая `]` после Ctrl+Z не становится «своей»', () => {
    let state = stateOf('[а|]');
    state = state.update({ changes: { from: 2, insert: 'б' }, selection: { anchor: 3 }, userEvent: 'input.type' }).state;
    state = history(state, undo);
    expect(closeBracket(state)).toBeNull();
  });
});

/** Enter в паре: правило или обычный Enter (`null`). */
const enter = (text: string): string | null => apply(text, enterInPair);

describe('Enter в только что вставленной паре (QA 2.18, задача 2.21)', () => {
  it.each([
    ['пустая пара в пункте с текстом — пара убирается, новый пункт', '- а ', '[', '- а \n- |'],
    ['пустая пара в пустом пункте — пара убирается, выход из списка', '- ', '[', '|'],
    ['пустая пара во вложенном пустом пункте — на уровень выше', '- а\n\t- ', '[', '- а\n- |'],
    ['пустая `[[]]` — убирается целиком', '- а ', '[[', '- а \n- |'],
    ['пустая пара в абзаце — пара убирается, обычный перенос', 'текст ', '[', 'текст \n|'],
  ])('%s', (_name, before, keys, after) => {
    const state = typed(stateOf(`${before}|`), keys);
    const spec = enterInPair(state);
    expect(spec).not.toBeNull();
    expect(marked(state.update(spec ?? {}).state)).toBe(after);
  });

  it.each([
    ['текст в паре — `]` остаётся на строке, новый пункт', '- [а', '- [а]\n- |'],
    ['вики-ссылка — `]]` остаются на строке', '- см. [[Имя', '- см. [[Имя]]\n- |'],
    ['номер — следующий', '1. [а', '1. [а]\n2. |'],
    ['в абзаце — перенос после `]`', 'см. [а', 'см. [а]\n|'],
  ])('%s', (_name, before, after) => {
    const state = typed(stateOf('|'), before);
    const spec = enterInPair(state);
    expect(spec).not.toBeNull();
    expect(marked(state.update(spec ?? {}).state)).toBe(after);
  });

  it.each([
    ['чужая `]`', '- [а|]'],
    ['курсор не перед `]`', '- [а]|'],
  ])('%s — обычный Enter', (_name, before) => {
    expect(enter(before)).toBeNull();
  });

  it('Ctrl+Z отменяет Enter одним шагом, пара снова «своя»', () => {
    let state = typed(stateOf('- а |'), '[');
    state = state.update({ annotations: isolateHistory.of('full') }).state;
    state = state.update(enterInPair(state) ?? {}).state;
    state = history(state, undo);
    expect(marked(state)).toBe('- а [|]');
    expect(marked(run(state, closeBracket, ']'))).toBe('- а []|');
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
