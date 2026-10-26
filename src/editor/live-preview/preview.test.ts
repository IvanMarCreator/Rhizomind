// Живой предпросмотр (С3.3): разметка прячется вне строки с курсором и
// показывается на ней. Проверяется по оформлению состояния редактора: какие
// участки текста скрыты или заменены значками, какие строки и куски текста
// оформлены.
import { EditorSelection, EditorState } from '@codemirror/state';
import { Decoration } from '@codemirror/view';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { previewDecorations } from './index.ts';

/** Курсор — после символа `|` (сам символ из текста убирается). */
function stateWithCursor(marked: string): EditorState {
  const at = marked.indexOf('|');
  if (at < 0) throw new Error('нет курсора');
  const text = marked.slice(0, at) + marked.slice(at + 1);
  return createEditorState(text, { mode: 'live_preview' }).update({ selection: EditorSelection.cursor(at) }).state;
}

/** Скрытые и заменённые значками участки — как текст. */
function hidden(state: EditorState): string[] {
  const out: [number, string][] = [];
  previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
    if (from < to && isReplace(value)) out.push([from, state.sliceDoc(from, to)]);
  });
  return out.sort((a, b) => a[0] - b[0]).map(([, text]) => text);
}

function isReplace(value: Decoration): boolean {
  return (value.spec as { rz?: string }).rz === 'hide' || (value.spec as { rz?: string }).rz === 'widget';
}

/** Классы оформления у участков текста: `класс: текст`. */
function marks(state: EditorState): string[] {
  const out: string[] = [];
  previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
    const cls = (value.spec as { class?: string }).class;
    if (from < to && cls && !isReplace(value)) out.push(`${cls}: ${state.sliceDoc(from, to)}`);
  });
  return out;
}

/** Классы строк: `номер строки: класс`. */
function lineClasses(state: EditorState): string[] {
  const out: string[] = [];
  previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
    const attrs = (value.spec as { attributes?: { class?: string } }).attributes;
    if (from === to && attrs?.class) out.push(`${state.doc.lineAt(from).number}: ${attrs.class}`);
  });
  return out;
}

/** Элемент на второй строке; курсор то в первой строке, то во второй. */
function away(line: string): EditorState {
  return stateWithCursor(`|курсор\n${line}`);
}
function on(line: string): EditorState {
  return stateWithCursor(`курсор\n${line}|`);
}

describe('разметка прячется вне строки с курсором и видна на ней', () => {
  it.each([
    ['заголовок 1', '# Заголовок', ['# ']],
    ['заголовок 6', '###### Шестой', ['###### ']],
    ['заголовок с закрывающими решётками', '## Второй ##', ['## ', ' ##']],
    ['жирный **', 'а **жирный** б', ['**', '**']],
    ['жирный __', 'а __жирный__ б', ['__', '__']],
    ['курсив *', 'а *курсив* б', ['*', '*']],
    ['курсив _', 'а _курсив_ б', ['_', '_']],
    ['жирный курсив', '***оба***', ['*', '**', '**', '*']],
    ['зачёркнутый', 'а ~~зачёркнутый~~ б', ['~~', '~~']],
    ['код в строке', 'а `код` б', ['`', '`']],
    ['код в строке двойными', 'а ``ко`д`` б', ['``', '``']],
    ['цитата', '> цитата', ['> ']],
    ['вложенная цитата', '> > глубже', ['> ', '> ']],
    ['маркированный список', '- пункт', ['-']],
    ['маркированный список *', '* пункт', ['*']],
    ['чекбокс', '- [ ] задача', ['- ', '[ ]']],
    ['отмеченный чекбокс', '- [x] сделано', ['- ', '[x]']],
    ['горизонтальная линия', '\n---', ['---']],
    ['горизонтальная линия ***', '\n* * *', ['* * *']],
    ['ссылка', 'см. [текст](https://example.org) тут', ['[', '](https://example.org)']],
    ['ссылка с заголовком', '[т](https://a.b "Подсказка")', ['[', '](https://a.b "Подсказка")']],
    ['ссылка с жирным текстом', '[**т**](https://a.b)', ['[', '**', '**', '](https://a.b)']],
  ])('%s', (_name, line, expected) => {
    expect(hidden(away(line))).toEqual(expected);
    expect(hidden(on(line))).toEqual([]);
  });

  it('блок кода: ограды скрыты, пока курсор вне блока, и видны, когда курсор в любой строке блока', () => {
    const block = '```js\nlet a = 1;\n```';
    expect(hidden(away(block))).toEqual(['```js', '```']);
    expect(hidden(stateWithCursor(`курсор\n\`\`\`js\nlet| a = 1;\n\`\`\``))).toEqual([]);
    expect(hidden(on(block))).toEqual([]);
  });

  it('выделение на несколько строк показывает разметку всех этих строк', () => {
    const state = createEditorState('# А\n# Б\n# В', { mode: 'live_preview' }).update({ selection: EditorSelection.range(0, 5) }).state;
    expect(hidden(state)).toEqual(['# ']);
  });

  it('в режиме исходного текста разметка не прячется и не оформляется', () => {
    const state = createEditorState('курсор\n# Заголовок **ж**', { mode: 'source' });
    expect(previewDecorations(state).size).toBe(0);
  });
});

describe('оформление', () => {
  it('заголовки: класс строки по уровню', () => {
    expect(lineClasses(away('### Третий'))).toEqual(['2: cm-rz-heading cm-rz-h3']);
  });

  it('жирный, курсив, зачёркнутый, код — классы у текста', () => {
    expect(marks(away('**ж** *к* ~~з~~ `к`'))).toEqual([
      'cm-rz-strong: ж',
      'cm-rz-em: к',
      'cm-rz-strike: з',
      'cm-rz-code: к',
    ]);
  });

  it('разметка на строке с курсором приглушена', () => {
    expect(marks(on('**ж**'))).toEqual(['cm-rz-markup: **', 'cm-rz-strong: ж', 'cm-rz-markup: **']);
  });

  it('блок кода: строки моноширинные, текст внутри не разбирается', () => {
    const state = away('```\n**не жирный**\n```');
    expect(lineClasses(state)).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock cm-rz-codeblock-last',
    ]);
    expect(marks(state)).toEqual([]);
  });

  it('незакрытый блок кода идёт до конца текста', () => {
    expect(lineClasses(away('```\n# не заголовок'))).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first',
      '3: cm-rz-codeblock',
    ]);
  });

  it('цитата и список — классы строк', () => {
    expect(lineClasses(away('> ц'))).toEqual(['2: cm-rz-quote']);
    expect(lineClasses(away('- [x] сделано'))).toEqual(['2: cm-rz-task-done']);
  });

  it('нумерованный список: номер не прячется, а приглушается', () => {
    expect(hidden(away('1. пункт'))).toEqual([]);
    expect(marks(away('1. пункт'))).toEqual(['cm-rz-list-number: 1.']);
    expect(marks(away('12) пункт'))).toEqual(['cm-rz-list-number: 12)']);
  });

  it('ссылка: текст оформлен как ссылка, адрес — в атрибуте', () => {
    const state = away('[текст](https://example.org)');
    const found: string[] = [];
    previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
      const spec = value.spec as { class?: string; attributes?: Record<string, string> };
      if (spec.class === 'cm-rz-link') found.push(`${state.sliceDoc(from, to)} → ${spec.attributes?.['data-rz-url']}`);
    });
    expect(found).toEqual(['текст → https://example.org']);
  });
});

describe('что не оформляется (показывается как исходный текст)', () => {
  it.each([
    ['звёздочки внутри слова с пробелами вокруг', 'a * b * c'],
    ['подчёркивания внутри слова', 'snake_case_name'],
    ['экранированные звёздочки', '\\*не курсив\\*'],
    ['незакрытый жирный', '**нет пары'],
    ['одиночная тильда', '~не зачёркнутый~'],
    ['вики-ссылка (задача 2.6)', '[[Заметка]]'],
    ['картинка (выпуск 0.3)', '![alt](a.png)'],
    ['решётка без пробела', '#тег'],
    ['семь решёток', '####### нет'],
    ['звёздочки в коде', '`*не курсив*`'],
  ])('%s', (_name, line) => {
    const state = away(line);
    const kinds = marks(state).filter((m) => !m.startsWith('cm-rz-code'));
    expect(hidden(state)).toEqual(line.startsWith('`') ? ['`', '`'] : []);
    expect(kinds).toEqual([]);
  });

  it('--- сразу под строкой текста — не линия (в CommonMark это заголовок второго уровня; в 0.1 — исходный текст)', () => {
    expect(hidden(away('текст\n---'))).toEqual([]);
  });

  it('блок свойств в начале заметки не оформляется (скрытие — выпуск 0.2)', () => {
    const state = stateWithCursor('---\ntitle: **x**\n---\n|\n---');
    expect(hidden(state)).toEqual(['---']);
    expect(marks(state)).toEqual([]);
  });

  it('очень длинная строка не разбирается внутри (как исходный текст)', () => {
    expect(marks(away('**ж** ' + 'а'.repeat(20_000)))).toEqual([]);
  });
});

describe('выделение по правилам CommonMark', () => {
  it.each([
    ['курсив внутри жирного', '**а *б* в**', ['**', '*', '*', '**']],
    ['жирный внутри курсива', '*а **б** в*', ['*', '**', '**', '*']],
    ['лишняя звёздочка остаётся текстом', '**а*', ['*', '*']],
    ['подчёркивания у слова', '__init__', ['__', '__']],
    ['звёздочки внутри слова', 'пре*фикс*ный', ['*', '*']],
  ])('%s', (_name, line, expected) => {
    expect(hidden(away(line))).toEqual(expected);
  });
});

describe('блоки кода после правок', () => {
  function edit(state: EditorState, from: number, to: number, insert: string): EditorState {
    return state.update({ changes: { from, to, insert } }).state;
  }

  it('набранная ограда открывает блок, удалённая — закрывает', () => {
    let state = away('a\n# б');
    const end = state.doc.length;
    state = edit(state, 7, 7, '```\n');
    expect(lineClasses(state)).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock',
    ]);
    state = edit(state, 7, 11, '');
    expect(state.doc.length).toBe(end);
    expect(lineClasses(state)).toEqual(['3: cm-rz-heading cm-rz-h1']);
  });

  it('правка внутри блока и над ним сдвигает блок вместе с текстом', () => {
    let state = stateWithCursor('|абв\n```\nкод\n```\n# з');
    state = edit(state, 0, 0, 'новая строка\n');
    state = edit(state, state.doc.toString().indexOf('код'), state.doc.toString().indexOf('код'), 'ещё ');
    expect(lineClasses(state)).toEqual([
      '3: cm-rz-codeblock cm-rz-codeblock-first',
      '4: cm-rz-codeblock',
      '5: cm-rz-codeblock cm-rz-codeblock-last',
      '6: cm-rz-heading cm-rz-h1',
    ]);
  });
});

describe('незакрытый блок кода после правок', () => {
  it('текст, дописанный в конец, остаётся в блоке', () => {
    let state = away('```\nкод');
    state = state.update({ changes: { from: state.doc.length, insert: '\nещё\n# тоже код' } }).state;
    expect(lineClasses(state)).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock',
      '5: cm-rz-codeblock',
    ]);
  });
});
