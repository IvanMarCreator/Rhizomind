// Продолжение списков, задач и цитат по Enter и вложенность по Tab
// (решение владельца 92, задача 2.15). Курсор в образцах — `|`.
import { undo } from '@codemirror/commands';
import { EditorSelection, EditorState, type Transaction } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState, editorExtensions } from './codemirror.ts';
import { checkChord, chordFromEvent, type KeyLike } from '../settings/hotkeys.ts';
import { continueMarkup, indentList } from './list-continuation.ts';

function stateOf(marked: string): EditorState {
  const cursor = marked.indexOf('|');
  const head = marked.lastIndexOf('|');
  const text = marked.replaceAll('|', '');
  const selection = cursor === head ? EditorSelection.cursor(cursor) : EditorSelection.range(cursor, head - 1);
  return createEditorState(text).update({ selection }).state;
}

function marked(state: EditorState): string {
  const { from, to } = state.selection.main;
  const doc = state.doc.toString();
  if (from === to) return `${doc.slice(0, from)}|${doc.slice(from)}`;
  return `${doc.slice(0, from)}|${doc.slice(from, to)}|${doc.slice(to)}`;
}

/** Enter по правилу продолжения; `null` — правило не действует (обычный Enter). */
function enter(text: string): string | null {
  const state = stateOf(text);
  const spec = continueMarkup(state);
  return spec ? marked(state.update(spec).state) : null;
}

function tab(text: string, direction: 1 | -1 = 1): string | null {
  const state = stateOf(text);
  const spec = indentList(state, direction);
  return spec ? marked(state.update(spec).state) : null;
}

describe('Enter в конце пункта — новый пункт', () => {
  it.each([
    ['маркер -', '- текст|', '- текст\n- |'],
    ['маркер *', '* текст|', '* текст\n* |'],
    ['маркер +', '+ текст|', '+ текст\n+ |'],
    ['отступ пробелами сохраняется', '    - вложенный|', '    - вложенный\n    - |'],
    ['отступ табуляцией сохраняется', '\t\t- вложенный|', '\t\t- вложенный\n\t\t- |'],
    ['пробелы после маркера сохраняются', '-   текст|', '-   текст\n-   |'],
    ['задача не отмечена', '- [ ] купить|', '- [ ] купить\n- [ ] |'],
    ['отмеченная задача — новая не отмечена', '- [x] сделано|', '- [x] сделано\n- [ ] |'],
    ['отмеченная задача X', '* [X] сделано|', '* [X] сделано\n* [ ] |'],
    ['нумерованный с точкой', '1. первый|', '1. первый\n2. |'],
    ['нумерованный со скобкой', '9) девятый|', '9) девятый\n10) |'],
    ['нумерованный с отступом', '\t3. третий|', '\t3. третий\n\t4. |'],
    ['нумерованная задача', '1. [x] готово|', '1. [x] готово\n2. [ ] |'],
    ['цитата', '> мысль|', '> мысль\n> |'],
    ['вложенная цитата', '> > глубже|', '> > глубже\n> > |'],
    ['цитата без пробела', '>мысль|', '>мысль\n>|'],
    ['цитата со списком', '> - пункт|', '> - пункт\n> - |'],
    ['цитата с задачей', '> - [x] пункт|', '> - [x] пункт\n> - [ ] |'],
    ['Unicode и эмодзи', '- Привет 👩🏽‍💻 мир|', '- Привет 👩🏽‍💻 мир\n- |'],
  ])('%s', (_name, before, after) => {
    expect(enter(before)).toBe(after);
  });

  it('строки ниже не трогаются', () => {
    expect(enter('- а|\nтекст ниже')).toBe('- а\n- |\nтекст ниже');
  });
});

describe('Enter посреди пункта — строка разрывается, продолжение с маркером', () => {
  it.each([
    ['список', '- раз|два', '- раз\n- |два'],
    ['задача', '- [x] раз|два', '- [x] раз\n- [ ] |два'],
    ['нумерованный', '1. раз|два', '1. раз\n2. |два'],
    ['цитата', '> раз|два', '> раз\n> |два'],
    ['между суррогатами не режется — курсор за эмодзи', '- 👍|ок', '- 👍\n- |ок'],
  ])('%s', (_name, before, after) => {
    expect(enter(before)).toBe(after);
  });
});

describe('нумерованный список: следующие номера сдвигаются', () => {
  it('вставка в середину перенумеровывает пункты ниже того же уровня', () => {
    expect(enter('1. а|\n2. б\n   - вложенный\n3. в\n\nабзац\n4. другой')).toBe(
      '1. а\n2. |\n3. б\n   - вложенный\n4. в\n\nабзац\n4. другой',
    );
  });

  it('вложенный нумерованный список ниже не меняется', () => {
    expect(enter('1. а|\n\t1. x\n2. б')).toBe('1. а\n2. |\n\t1. x\n3. б');
  });
});

describe('перенумерация: QA 2.15', () => {
  it.each([
    ['ведущие нули сохраняются', '01. a|\n02. b\n03. c', '01. a\n02. |\n03. b\n04. c'],
    ['ширина с нулями растёт только при переполнении', '09. a|\n10. b', '09. a\n10. |\n11. b'],
    ['другой разделитель — другой список, не трогается', '1. a|\n2) b', '1. a\n2. |\n2) b'],
    ['другой уровень цитаты — конец перенумерации', '> 1. a|\n> 2. b\n2. c', '> 1. a\n> 2. |\n> 3. b\n2. c'],
    ['в цитате перенумеровывается своя цитата', '> 1. a|\n> 2. b\n> 3. c', '> 1. a\n> 2. |\n> 3. b\n> 4. c'],
  ])('%s', (_name, before, after) => {
    expect(enter(before)).toBe(after);
  });

  it('10 000 пунктов — одно изменение одного участка, быстро; отмена одним шагом', () => {
    const items = Array.from({ length: 10_000 }, (_v, k) => `${k + 1}. пункт`);
    let state = stateOf(`${items[0]}|\n${items.slice(1).join('\n')}`);
    const started = performance.now();
    const spec = continueMarkup(state);
    const tr = state.update(spec ?? {});
    const elapsed = performance.now() - started;
    let ranges = 0;
    tr.changes.iterChanges(() => ranges++);
    expect(ranges).toBeLessThanOrEqual(2);
    expect(elapsed).toBeLessThan(200);
    state = tr.state;
    expect(state.doc.line(10_001).text).toBe('10001. пункт');
    expect(state.doc.line(2).text).toBe('2. ');
    undo({ state, dispatch: (t: Transaction) => { state = t.state; } });
    expect(state.doc.toString()).toBe(items.join('\n'));
  });
});

describe('Enter на пустом пункте — выход из списка', () => {
  it.each([
    ['пустой пункт верхнего уровня — маркер удаляется', '- а\n- |', '- а\n|'],
    ['маркер без пробела', '- а\n-|', '- а\n|'],
    ['пустая задача', '- [ ] а\n- [ ] |', '- [ ] а\n|'],
    ['пустой нумерованный', '1. а\n2. |', '1. а\n|'],
    ['пустая цитата', '> а\n> |', '> а\n|'],
    ['пустая вложенная цитата — на уровень выше', '> > а\n> > |', '> > а\n> |'],
    ['пустой пункт в цитате — остаётся цитата', '> - а\n> - |', '> - а\n> |'],
    ['вложенный пустой пункт (табуляция) — уровень выше', '- а\n\t- |', '- а\n- |'],
    ['вложенный пустой пункт (4 пробела) — уровень выше', '- а\n    - |', '- а\n- |'],
    ['вложенный на два уровня — на один уровень выше', '\t\t- |', '\t- |'],
    ['вложенный пустой в цитате', '> - а\n>     - |', '> - а\n> - |'],
    ['пустой пункт с пробелами после маркера', '- а\n-   |', '- а\n|'],
  ])('%s', (_name, before, after) => {
    expect(enter(before)).toBe(after);
  });
});

describe('одиночный маркер под абзацем — не пункт (QA 2.15)', () => {
  it.each([
    ['«-» под текстом', 'Заголовок\n-|'],
    ['«2020.» под текстом', 'Итого\n2020.|'],
    ['«*» под текстом', 'текст\n*|'],
    ['в цитате под текстом цитаты', '> текст\n> -|'],
  ])('%s — обычный Enter', (_name, before) => {
    expect(enter(before)).toBeNull();
  });

  it.each([
    ['под пунктом списка', '- а\n-|', '- а\n|'],
    ['под заголовком #', '# З\n-|', '# З\n|'],
    ['под пустой строкой', 'текст\n\n-|', 'текст\n\n|'],
    ['под продолжением пункта с отступом', '- а\n  ещё\n-|', '- а\n  ещё\n|'],
  ])('%s — пустой пункт', (_name, before, after) => {
    expect(enter(before)).toBe(after);
  });
});

describe('мелочи QA 2.15', () => {
  it('цитата из одних пробелов — выход, как из пустой', () => {
    expect(enter('> а\n>   |')).toBe('> а\n|');
  });

  it('пробелы после [ ] у задачи сохраняются', () => {
    expect(enter('- [ ]   а|')).toBe('- [ ]   а\n- [ ]   |');
  });

  it.each([
    ['блок кода в цитате — Enter обычный', '> ```\n> - а|\n> ```'],
    ['незакрытый блок кода в цитате', '> ~~~\n> - а|'],
  ])('%s', (_name, before) => {
    expect(enter(before)).toBeNull();
    expect(tab(before)).toBeNull();
  });

  it('после блока кода в цитате список продолжается', () => {
    expect(enter('> ```\n> код\n> ```\n> - а|')).toBe('> ```\n> код\n> ```\n> - а\n> - |');
  });

  it('Tab: строка другого уровня цитаты — не вложенная строка пункта', () => {
    expect(tab('> - а|\n>   - б\n  - в')).toBe('>   - а|\n>     - б\n  - в');
  });
});

describe('где правило не действует (обычный Enter)', () => {
  it.each([
    ['обычный текст', 'текст|'],
    ['заголовок', '# Заголовок|'],
    ['курсор до маркера', '|- текст'],
    ['курсор в отступе', '  |- текст'],
    ['линия «- - -»', '- - -|'],
    ['не список: «-текст»', '-текст|'],
    ['не список: «1.текст»', '1.текст|'],
    ['номер из 10 цифр — не список', '1234567890. текст|'],
    ['в блоке кода', '```\n- текст|\n```'],
    ['в незакрытом блоке кода', '```js\n- текст|'],
    ['в блоке свойств', '---\n- тег|\n---\nтекст'],
    ['выделение — обычный Enter', '- |текст|'],
  ])('%s', (_name, before) => {
    expect(enter(before)).toBeNull();
  });

  it('несколько курсоров — обычный Enter', () => {
    const state = EditorState.create({
      doc: '- а\n- б',
      extensions: [editorExtensions(), EditorState.allowMultipleSelections.of(true)],
      selection: EditorSelection.create([EditorSelection.cursor(3), EditorSelection.cursor(7)]),
    });
    expect(state.selection.ranges).toHaveLength(2);
    expect(continueMarkup(state)).toBeNull();
  });

  it('только чтение — правило не действует', () => {
    const state = createEditorState('- а', { readOnly: true }).update({ selection: { anchor: 3 } }).state;
    expect(continueMarkup(state)).toBeNull();
    expect(indentList(state, 1)).toBeNull();
  });

  it('после блока кода список снова продолжается', () => {
    expect(enter('```\nкод\n```\n- а|')).toBe('```\nкод\n```\n- а\n- |');
  });
});

describe('отмена — одним шагом', () => {
  it('Ctrl+Z после продолжения возвращает текст до Enter', () => {
    let state = stateOf('1. а|\n2. б');
    const spec = continueMarkup(state);
    expect(spec).not.toBeNull();
    state = state.update(spec ?? {}).state;
    expect(state.doc.toString()).toBe('1. а\n2. \n3. б');
    undo({
      state,
      dispatch: (tr: Transaction) => {
        state = tr.state;
      },
    });
    expect(marked(state)).toBe('1. а|\n2. б');
  });
});

describe('Tab и Shift+Tab — вложенность пункта', () => {
  it.each([
    ['по умолчанию — табуляция (как в Obsidian)', '- а\n- б|', '- а\n\t- б|'],
    ['стиль документа: табуляция', '- а\n\t- б\n- в|', '- а\n\t- б\n\t- в|'],
    ['стиль документа: 2 пробела', '- а\n  - б\n- в|', '- а\n  - б\n  - в|'],
    ['стиль документа: 4 пробела', '- а\n    - б\n- в|', '- а\n    - б\n    - в|'],
    ['курсор посреди текста — остаётся на месте в тексте', '- а\n- б|в', '- а\n\t- б|в'],
    ['пустой пункт', '- а\n- |', '- а\n\t- |'],
    ['задача', '- [ ] а\n- [ ] б|', '- [ ] а\n\t- [ ] б|'],
    ['в цитате — после префикса цитаты', '> - а\n> - б|', '> - а\n> \t- б|'],
    ['вложенные пункты сдвигаются вместе с родителем', '- а|\n\t- б\n- в', '\t- а|\n\t\t- б\n- в'],
  ])('%s', (_name, before, after) => {
    expect(tab(before)).toBe(after);
  });

  it.each([
    ['табуляция', '- а\n\t- б|', '- а\n- б|'],
    ['4 пробела', '- а\n    - б|', '- а\n- б|'],
    ['2 пробела', '- а\n  - б\n  - в|', '- а\n  - б\n- в|'],
    ['вложенные пункты — вместе с родителем', '- а\n\t- б|\n\t\t- в\n\t- г', '- а\n- б|\n\t- в\n\t- г'],
    ['верхний уровень — без изменений, нажатие не уходит из редактора', '- а|', '- а|'],
  ])('Shift+Tab: %s', (_name, before, after) => {
    expect(tab(before, -1)).toBe(after);
  });

  it('выделение на несколько пунктов — сдвигаются все', () => {
    expect(tab('- а\n- |б\n- в|')).toBe('- а\n\t- |б\n\t- в|');
  });

  it.each([
    ['обычный текст', 'текст|'],
    ['в блоке кода', '```\n- а|\n```'],
    ['заголовок', '# а|'],
  ])('вне списка Tab не перехватывается: %s', (_name, before) => {
    expect(tab(before)).toBeNull();
    expect(tab(before, -1)).toBeNull();
  });
});

// Правило 81: Enter и Tab — клавиши правки текста; окно не может забрать их
// у редактора назначением в реестре горячих клавиш (src/settings/hotkeys.ts).
describe('реестр горячих клавиш не перехватывает Enter и Tab', () => {
  const key = (k: string, shiftKey = false): KeyLike => ({ key: k, code: k, ctrlKey: false, altKey: false, shiftKey, metaKey: false });

  it('Tab и Shift+Tab не назначаются: сочетания нет', () => {
    expect(chordFromEvent(key('Tab'))).toBeNull();
    expect(chordFromEvent(key('Tab', true))).toBeNull();
  });

  it('Enter без модификатора назначить нельзя', () => {
    expect(checkChord('Enter')).not.toBeNull();
  });
});
