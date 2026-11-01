// Живой предпросмотр (С3.3, решение 93): разметка видна только у элемента,
// в котором стоит курсор или с которым он соприкасается; остальная строка
// оформлена. Проверяется по оформлению состояния редактора: какие участки
// текста скрыты или заменены значками, какие строки и куски текста оформлены.
import { EditorSelection, EditorState, StateEffect } from '@codemirror/state';
import { Decoration } from '@codemirror/view';
import { redo, undo } from '@codemirror/commands';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { codeBlocks, previewDecorations, taskToggle } from './index.ts';
import { scanBlocks } from './markdown.ts';

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
    if (from === to && attrs?.class && !isGap(value)) out.push(`${state.doc.lineAt(from).number}: ${attrs.class}`);
  });
  return out;
}

/** Промежуток между блоками (задача 2.20) — отдельное оформление строки; его проверяет `gaps`. */
function isGap(value: Decoration): boolean {
  return (value.spec as { rz?: string }).rz === 'gap';
}

/** Промежутки между блоками: `номер строки: after` (снизу) или `before` (сверху). */
function gaps(state: EditorState, ranges?: { from: number; to: number }[]): string[] {
  const out: string[] = [];
  previewDecorations(state, ranges).between(0, state.doc.length, (from, _to, value) => {
    if (!isGap(value)) return;
    const cls = (value.spec as { attributes: { class: string } }).attributes.class;
    out.push(`${state.doc.lineAt(from).number}: ${cls === 'cm-rz-gap-before' ? 'before' : 'after'}`);
  });
  return out;
}

/** Элемент на второй строке; курсор в первой строке. */
function away(line: string): EditorState {
  return stateWithCursor(`|курсор\n${line}`);
}
/** Курсор в конце строки с элементом (касается элемента, который кончается в конце строки). */
function on(line: string): EditorState {
  return stateWithCursor(`курсор\n${line}|`);
}
/** Строка с элементом; курсор — `|` внутри неё. */
function at(marked: string): EditorState {
  return stateWithCursor(`курсор\n${marked}`);
}

describe('разметка прячется, пока курсор не в элементе, и видна, когда он в нём (решение 93)', () => {
  // Решение 93 (задача 2.16): прежде проверка шла курсором в конце строки —
  // «на строке» значило «раскрыто». Теперь курсор ставится внутрь элемента.
  it.each([
    ['заголовок 1', '# Заголовок', ['# '], '#| Заголовок'],
    ['заголовок 6', '###### Шестой', ['###### '], '###### |Шестой'],
    ['жирный **', 'а **жирный** б', ['**', '**'], 'а **жир|ный** б'],
    ['жирный __', 'а __жирный__ б', ['__', '__'], 'а __жир|ный__ б'],
    ['курсив *', 'а *курсив* б', ['*', '*'], 'а *кур|сив* б'],
    ['курсив _', 'а _курсив_ б', ['_', '_'], 'а _кур|сив_ б'],
    ['жирный курсив', '***оба***', ['*', '**', '**', '*'], '***о|ба***'],
    ['зачёркнутый', 'а ~~зачёркнутый~~ б', ['~~', '~~'], 'а ~~зачёрк|нутый~~ б'],
    ['код в строке', 'а `код` б', ['`', '`'], 'а `к|од` б'],
    ['код в строке двойными', 'а ``ко`д`` б', ['``', '``'], 'а ``ко|`д`` б'],
    ['цитата', '> цитата', ['> '], '>| цитата'],
    ['вложенная цитата', '> > глубже', ['> ', '> '], '> >| глубже'],
    ['маркированный список: значок вместе с пробелом за маркером', '- пункт', ['- '], '-| пункт'],
    ['маркированный список *', '* пункт', ['* '], '* |пункт'],
    ['маркированный список с табом', '-\tпункт', ['-\t'], '|-\tпункт'],
    ['пустой пункт', '-', ['-'], '-|'],
    ['чекбокс: значок вместе с пробелом за ним', '- [ ] задача', ['- ', '[ ] '], '- [| ] задача'],
    ['отмеченный чекбокс', '- [x] сделано', ['- ', '[x] '], '- [x] |сделано'],
    ['чекбокс без текста', '- [ ]', ['- ', '[ ]'], '- [ ]|'],
    ['горизонтальная линия', '\n---', ['---'], '\n-|--'],
    ['горизонтальная линия ***', '\n* * *', ['* * *'], '\n* * *|'],
    ['ссылка', 'см. [текст](https://example.org) тут', ['[', '](https://example.org)'], 'см. [текст](https://exa|mple.org) тут'],
    ['ссылка с заголовком', '[т](https://a.b "Подсказка")', ['[', '](https://a.b "Подсказка")'], '[т|](https://a.b "Подсказка")'],
  ])('%s', (_name, line, expected, inside) => {
    expect(hidden(away(line))).toEqual(expected);
    expect(hidden(at(inside))).toEqual([]);
  });

  it('ссылка с жирным текстом: курсор в жирном раскрывает и жирный, и ссылку', () => {
    expect(hidden(away('[**т**](https://a.b)'))).toEqual(['[', '**', '**', '](https://a.b)']);
    expect(hidden(at('[**т|**](https://a.b)'))).toEqual([]);
  });

  it('блок кода: ограды скрыты, пока курсор вне блока, и видны, когда курсор в любой строке блока', () => {
    const block = '```js\nlet a = 1;\n```';
    expect(hidden(away(block))).toEqual(['```js', '```']);
    expect(hidden(stateWithCursor(`курсор\n\`\`\`js\nlet| a = 1;\n\`\`\``))).toEqual([]);
    expect(hidden(on(block))).toEqual([]);
  });

  it('выделение на несколько строк показывает разметку задетых элементов этих строк', () => {
    const state = createEditorState('# А\n# Б\n# В', { mode: 'live_preview' }).update({ selection: EditorSelection.range(0, 5) }).state;
    expect(hidden(state)).toEqual(['# ']);
  });

  it('в режиме исходного текста разметка не прячется и не оформляется', () => {
    const state = createEditorState('курсор\n# Заголовок **ж**', { mode: 'source' });
    expect(previewDecorations(state).size).toBe(0);
  });
});

describe('решение 93: разметка только у элемента с курсором, а не у всей строки', () => {
  /** Выделения: `⟨…⟩` — диапазоны (несколько — несколько выделений). */
  function selected(marked: string): EditorState {
    const ranges: { from: number; to: number }[] = [];
    let text = '';
    let open = -1;
    for (const ch of marked) {
      if (ch === '⟨') open = text.length;
      else if (ch === '⟩') ranges.push({ from: open, to: text.length });
      else text += ch;
    }
    const base = createEditorState(text, { mode: 'live_preview' });
    const multi = base.update({ effects: StateEffect.appendConfig.of(EditorState.allowMultipleSelections.of(true)) }).state;
    return multi.update({ selection: EditorSelection.create(ranges.map((r) => EditorSelection.range(r.from, r.to))) }).state;
  }

  /** Приглушённая (видимая) разметка — как текст. */
  function faint(state: EditorState): string[] {
    return marks(state)
      .filter((m) => m.startsWith('cm-rz-markup: '))
      .map((m) => m.slice('cm-rz-markup: '.length));
  }

  it('несколько элементов на строке: раскрыт только тот, в котором курсор', () => {
    const state = at('а **ж|ир** и ~~з~~ `к` [с](https://a.b) [[В]]');
    expect(faint(state)).toEqual(['**', '**']);
    expect(hidden(state)).toEqual(['~~', '~~', '`', '`', '[', '](https://a.b)', '[[', ']]']);
  });

  it.each([
    ['курсив', '*а* *б|* *в*', ['*', '*', '*', '*'], ['*', '*']],
    ['зачёркнутый', '~~а~~ ~|~б~~', ['~~', '~~'], ['~~', '~~']],
    ['код в строке', '`а` `б|`', ['`', '`'], ['`', '`']],
    ['ссылка: курсор в адресе', '[а](https://a.b) [б](https://c.d|)', ['[', '](https://a.b)'], ['[', '](https://c.d)']],
    ['вики-ссылка', '[[А]] [[Б|]]', ['[[', ']]'], ['[[', ']]']],
  ])('%s: соседний такой же элемент остаётся оформленным', (_name, line, stillHidden, shown) => {
    const state = at(line);
    expect(hidden(state)).toEqual(stillHidden);
    expect(faint(state)).toEqual(shown);
  });

  it('вики-ссылка с путём и псевдонимом: соседняя остаётся оформленной', () => {
    // `|` занят псевдонимом: курсор — пустое выделение `⟨⟩`.
    const state = selected('курсор\n[[Альфа/Встреча|встр⟨⟩ечи]] и [[План]]');
    expect(hidden(state)).toEqual(['[[', ']]']);
    expect(faint(state)).toEqual(['[[Альфа/Встреча|', ']]']);
  });

  it('граница: курсор сразу перед открывающими или сразу после закрывающих символов раскрывает элемент', () => {
    expect(hidden(at('а |**ж** б'))).toEqual([]);
    expect(hidden(at('а **ж**| б'))).toEqual([]);
    expect(hidden(at('а| **ж** б'))).toEqual(['**', '**']);
    expect(hidden(at('а **ж** |б'))).toEqual(['**', '**']);
    // Два элемента вплотную: курсор между ними касается обоих.
    expect(hidden(at('**а**|~~б~~ `в`'))).toEqual(['`', '`']);
  });

  it('вложенные элементы: раскрываются все, что содержат курсор', () => {
    expect(hidden(at('~~**а|**~~ *б*'))).toEqual(['*', '*']);
    // В `~~` внешнего, но не у внутреннего: раскрыт только внешний.
    expect(hidden(at('~|~**а**~~'))).toEqual(['**', '**']);
    // В тексте ссылки, но не в жирном внутри неё.
    expect(hidden(at('[т| **ж**](https://a.b)'))).toEqual(['**', '**']);
  });

  it('у раскрытой ссылки «↗» нет, у соседней внешней — есть', () => {
    const state = at('[а](https://a.b) [б|](https://c.d)');
    const arrows: string[] = [];
    previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
      const widget = (value.spec as { widget?: { url?: string } }).widget;
      if (from === to && widget?.url) arrows.push(widget.url);
    });
    expect(arrows).toEqual(['https://a.b']);
  });

  // Маркер блока — один элемент на всю строку (уточнение владельца к решению 93):
  // виден, где бы на строке ни стоял курсор; строчные элементы строки — по своему курсору.
  it.each([
    ['заголовок: курсор в начале текста', '# |Заголовок', ['# ']],
    ['заголовок: курсор в середине', '# Заго|ловок', ['# ']],
    ['заголовок: курсор в конце', '# Заголовок|', ['# ']],
    ['заголовок с закрывающими решётками', '## Вто|рой ##', ['## ', ' ##']],
    ['цитата', '> ци|тата', ['> ']],
    ['вложенная цитата: видны все `>`', '> > те|кст', ['> ', '> ']],
    ['пункт списка', '- пункт|', ['- ']],
    ['задача: маркер и чекбокс', '- [ ] за|дача', ['- ', '[ ] ']],
    ['нумерованная задача', '1. [ ] за|дача', ['[ ] ']],
    ['пункт в цитате', '> - пу|нкт', ['> ', '- ']],
    ['линия', '\n* *| *', ['* * *']],
  ])('маркер блока виден при курсоре в любом месте строки — %s', (_name, line, marker) => {
    const plain = line.replace('|', '');
    expect(hidden(away(plain))).toEqual(marker);
    expect(hidden(at(line))).toEqual([]);
    expect(faint(at(line))).toEqual(marker);
  });

  it('строка цитаты со ссылкой и зачёркнутым: курсор в тексте — виден только `>`; курсор в ссылке — `>` и ссылка', () => {
    const quote = '> текст [ссылка](https://a) и ~~зачёркнутое~~';
    expect(hidden(at(quote.replace('текст', 'те|кст')))).toEqual(['[', '](https://a)', '~~', '~~']);
    expect(faint(at(quote.replace('текст', 'те|кст')))).toEqual(['> ']);
    expect(hidden(at(quote.replace('ссылка', 'ссы|лка')))).toEqual(['~~', '~~']);
    expect(faint(at(quote.replace('ссылка', 'ссы|лка')))).toEqual(['> ', '[', '](https://a)']);
  });

  it('строчные элементы в заголовке и в пункте раскрываются только по своему курсору', () => {
    expect(hidden(at('# За|г **ж** `к`'))).toEqual(['**', '**', '`', '`']);
    expect(hidden(at('# Заг **ж|** `к`'))).toEqual(['`', '`']);
    expect(hidden(at('- [x] сде|лано **ж**'))).toEqual(['**', '**']);
  });

  it('курсор на другой строке — маркеры блоков оформлены: класс строки, точка, чекбокс', () => {
    const state = away('- [x] сделано **ж**');
    expect(lineClasses(state)).toEqual(['2: cm-rz-task-done']);
    expect(hidden(state)).toEqual(['- ', '[x] ', '**', '**']);
  });

  it('выделение раскрывает все задетые им элементы, остальные — нет', () => {
    const state = selected('курсор\nа **ж** ~~з⟨~~ `к`⟩ **е**');
    expect(hidden(state)).toEqual(['**', '**', '**', '**']);
    expect(faint(state)).toEqual(['~~', '~~', '`', '`']);
  });

  it('несколько выделений: раскрыты элементы под каждым', () => {
    const state = selected('⟨⟩курсор\n*а* **б⟨⟩** `в` ~~г⟨~~⟩');
    expect(hidden(state)).toEqual(['*', '*', '`', '`']);
  });

  it('стрелки: курсор ни в одной позиции не оказывается внутри скрытой разметки — шаг ←/→ никогда не «пропадает» в невидимых символах', () => {
    // atomicRanges не используются: курсор ходит по символам текста. Элемент
    // раскрывается, как только курсор встал у его края, поэтому следующий шаг
    // идёт по уже видимой разметке.
    const lines = [
      'а **ж** ~~з~~ `к` [с](https://a.b "т") [[А/Б#В|г]] *е*',
      '> > - [ ] **ж** и [[Б#В]]',
      '## Заг ~~**о**~~ ##',
      '1. [x] ***оба*** `к`',
    ];
    for (const line of lines) {
      const base = createEditorState(`курсор\n${line}`, { mode: 'live_preview' });
      for (let pos = 7; pos <= base.doc.length; pos++) {
        const state = base.update({ selection: EditorSelection.cursor(pos) }).state;
        previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
          if (isReplace(value)) expect(from < pos && pos < to, `${JSON.stringify(line)}: курсор ${pos - 7} внутри ${state.sliceDoc(from, to)}`).toBe(false);
        });
      }
    }
  });

  it('набор: незакрытый элемент — разметка видна как текст; закрыли — раскрыт (курсор у границы); ушли — оформлен', () => {
    let state = at('а **жир|');
    expect(hidden(state)).toEqual([]);
    expect(marks(state)).toEqual([]);
    const end = state.doc.length;
    state = state.update({ changes: { from: end, insert: '**' }, selection: { anchor: end + 2 } }).state;
    expect(hidden(state)).toEqual([]);
    expect(faint(state)).toEqual(['**', '**']);
    state = state.update({ selection: { anchor: 0 } }).state;
    expect(hidden(state)).toEqual(['**', '**']);
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

  it('разметка элемента с курсором приглушена', () => {
    expect(marks(on('**ж**'))).toEqual(['cm-rz-markup: **', 'cm-rz-strong: ж', 'cm-rz-markup: **']);
  });

  it('блок кода: строки моноширинные, текст внутри не разбирается', () => {
    const state = away('```\n**не жирный**\n```');
    expect(lineClasses(state)).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock cm-rz-codeblock-last cm-rz-codeblock-fence',
    ]);
    expect(marks(state)).toEqual([]);
  });

  it('незакрытый блок кода идёт до конца текста', () => {
    expect(lineClasses(away('```\n# не заголовок'))).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
      '3: cm-rz-codeblock',
    ]);
  });

  it('цитата и список — классы строк', () => {
    expect(lineClasses(away('> ц'))).toEqual(['2: cm-rz-quote cm-rz-quote-1']);
    expect(lineClasses(away('- [x] сделано'))).toEqual(['2: cm-rz-task-done']);
  });

  it('нумерованный список: номер не прячется, а приглушается', () => {
    expect(hidden(away('1. пункт'))).toEqual([]);
    expect(marks(away('1. пункт'))).toEqual(['cm-rz-list-number: 1.', 'cm-rz-list-gap:  ']);
    expect(marks(away('12) пункт'))).toEqual(['cm-rz-list-number: 12)', 'cm-rz-list-gap:  ']);
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

describe('вид по макету Э1 (задача 2.13, находка Н8)', () => {
  /** Значки-вставки (без замены текста): `текст строки до значка| ↗ → адрес`; вид значка — LivePreview.look.component.test.ts. */
  function inserted(state: EditorState): string[] {
    const out: string[] = [];
    previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
      if (from !== to || !(value.spec as { widget?: unknown }).widget) return;
      const widget = (value.spec as { widget: { url?: string } }).widget;
      out.push(`${state.sliceDoc(0, from).split('\n').pop() ?? ''}| ↗ → ${widget.url}`);
    });
    return out;
  }

  it('вложенная цитата — класс по уровню: по полосе на каждый `>`', () => {
    expect(lineClasses(away('> > глубже'))).toEqual(['2: cm-rz-quote cm-rz-quote-2']);
    expect(lineClasses(away('> > > три'))).toEqual(['2: cm-rz-quote cm-rz-quote-3']);
    expect(lineClasses(away('>>>>>>>> восемь'))).toEqual(['2: cm-rz-quote cm-rz-quote-6']);
  });

  it('внешняя ссылка (http, https, mailto) — «↗» за текстом; у раскрытой ссылки (курсор в ней) его нет', () => {
    expect(inserted(away('см. [портал](https://example.com/r) тут'))).toEqual([
      'см. [портал| ↗ → https://example.com/r',
    ]);
    expect(inserted(away('[почта](mailto:a@b.c)'))).toEqual(['[почта| ↗ → mailto:a@b.c']);
    expect(inserted(away('[**т**](http://a.b)'))).toEqual(['[**т**| ↗ → http://a.b']);
    expect(inserted(on('[портал](https://example.com/r)'))).toEqual([]);
  });

  it('у вики-ссылок и ссылок не наружу «↗» нет', () => {
    expect(inserted(away('[[Встреча]] и [[Альфа/Встреча|встречи]]'))).toEqual([]);
    expect(inserted(away('[файл](file:///C:/x) [заметка](Заметка.md)'))).toEqual([]);
  });

  it('скрытые ограды блока кода не занимают строку; курсор в блоке — ограды обычные', () => {
    const block = '```js\nlet a = 1;\n```';
    expect(lineClasses(away(block))).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock cm-rz-codeblock-last cm-rz-codeblock-fence',
    ]);
    expect(lineClasses(on(block))).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock cm-rz-codeblock-last',
    ]);
  });
});

describe('что не оформляется (показывается как исходный текст)', () => {
  it.each([
    ['звёздочки внутри слова с пробелами вокруг', 'a * b * c'],
    ['подчёркивания внутри слова', 'snake_case_name'],
    ['экранированные звёздочки', '\\*не курсив\\*'],
    ['незакрытый жирный', '**нет пары'],
    ['одиночная тильда', '~не зачёркнутый~'],
    ['якорь [[#…]] (С3.4)', '[[#Заголовок]]'],
    ['встраивание (выпуск 0.3)', '![[Схема.png]]'],
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
      '2: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
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
      '3: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
      '4: cm-rz-codeblock',
      '5: cm-rz-codeblock cm-rz-codeblock-last cm-rz-codeblock-fence',
      '6: cm-rz-heading cm-rz-h1',
    ]);
  });
});

describe('незакрытый блок кода после правок', () => {
  it('текст, дописанный в конец, остаётся в блоке', () => {
    let state = away('```\nкод');
    state = state.update({ changes: { from: state.doc.length, insert: '\nещё\n# тоже код' } }).state;
    expect(lineClasses(state)).toEqual([
      '2: cm-rz-codeblock cm-rz-codeblock-first cm-rz-codeblock-fence',
      '3: cm-rz-codeblock',
      '4: cm-rz-codeblock',
      '5: cm-rz-codeblock',
    ]);
  });
});

describe('блоки после правок совпадают с разбором всего текста заново', () => {
  // Пересчёт идёт с места правки до совпадения с прежним разбором (2.5а).
  // Случайные правки строк из оград, свойств и текста: результат обязан
  // совпасть с полным разбором.
  const LINES = ['```', '```js', '~~~', '````', '``` a`b', '   ```', '    ```', '---', '...', 'текст', '', '# з', '`код`'];
  const PIECES = ['`', '``', '```', '~', '-', '.', 'js', ' ', '\n', '\n```\n', '\n---\n', 'а'];

  function random(seed: number): () => number {
    let x = seed;
    return () => {
      x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
      return x / 2_147_483_648;
    };
  }

  /** Отмена или повтор (Ctrl+Z / Ctrl+Y) через историю CodeMirror. */
  function historyStep(state: EditorState, command: typeof undo): EditorState {
    let next = state;
    command({ state, dispatch: (tr) => (next = tr.state) });
    return next;
  }

  it('правка у конца блока свойств задевает ограду под ним', () => {
    const cases: [string, number, number, string?][] = [
      ['---\na\n---\n```\ncode\n', 9, 13],
      ['---\na\n---\n```\ncode\n```\n# h', 9, 13],
      ['---\na\n---\n```\ncode\n```\n# h', 8, 14],
      ['---\nk\n...\n````\n```js\n```\n``` a`b', 9, 16, '\n---\n'],
    ];
    for (const [text, from, to, insert = ''] of cases) {
      let state = createEditorState(text, { mode: 'live_preview' });
      state = state.update({ changes: { from, to, insert } }).state;
      expect(codeBlocks(state), JSON.stringify(state.doc.toString())).toEqual(scanBlocks(state.doc));
      state = historyStep(state, undo);
      expect(state.doc.toString()).toBe(text);
      expect(codeBlocks(state), 'после отмены').toEqual(scanBlocks(state.doc));
      state = historyStep(state, redo);
      expect(codeBlocks(state), 'после повтора').toEqual(scanBlocks(state.doc));
    }
  });

  it('2000 случайных правок, отмен и повторов, в том числе с блоком свойств в начале', () => {
    const next = random(7);
    const pick = <T>(list: readonly T[]): T => list[Math.floor(next() * list.length)] as T;
    for (let doc = 0; doc < 40; doc++) {
      const lines = Array.from({ length: 4 + Math.floor(next() * 20) }, () => pick(LINES));
      if (doc % 2 === 0) lines.unshift('---', 'ключ: значение', pick(['---', '...']));
      let state = createEditorState(lines.join('\n'), { mode: 'live_preview' });
      for (let k = 0; k < 50; k++) {
        const roll = next();
        if (roll < 0.1) {
          state = historyStep(state, undo);
        } else if (roll < 0.15) {
          state = historyStep(state, redo);
        } else {
          const length = state.doc.length;
          const from = Math.floor(next() * (length + 1));
          const to = Math.min(length, from + (next() < 0.5 ? 0 : Math.floor(next() * 6)));
          const insert = next() < 0.3 ? '' : pick(PIECES);
          // Иногда вторая правка в той же транзакции (несколько курсоров).
          const second = Math.floor(next() * (length + 1));
          const extra = next() < 0.3 && (second < from || second > to) ? [{ from: second, insert: pick(PIECES) }] : [];
          state = state.update({ changes: [{ from, to, insert }, ...extra] }).state;
        }
        expect(codeBlocks(state), `текст: ${JSON.stringify(state.doc.toString())}`).toEqual(scanBlocks(state.doc));
      }
    }
  });
});

describe('чекбокс', () => {
  it('переключение меняет ровно один символ: `[ ]` ↔ `[x]`', () => {
    const state = createEditorState('- [ ] задача\n- [X] сделано', { mode: 'live_preview' });
    for (const [pos, from, insert] of [[2, 3, 'x'], [15, 16, ' ']] as const) {
      const spec = taskToggle(state, pos);
      const changes = state.update(spec ?? {}).changes;
      const touched: [number, number, string][] = [];
      changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => touched.push([fromA, toA, inserted.toString()]));
      expect(touched).toEqual([[from, from + 1, insert]]);
    }
  });
});

describe('задача 2.20: промежутки между блоками, вложенные пункты, номер в колонке маркера', () => {
  const live = (text: string) => createEditorState(text, { mode: 'live_preview' });

  it.each([
    ['строки одного абзаца — без промежутка; перед пустой строкой — промежуток', 'а\nб\n\nв', ['2: after']],
    ['заголовок — блок сам по себе', '# З\nТекст', ['1: after']],
    ['заголовок за заголовком', '# а\n## б', ['1: after']],
    ['абзац перед списком', 'Текст:\n- пункт', ['1: after']],
    ['пункты одного списка (и вложенные) — без промежутка, после списка — промежуток', '- а\n- б\n  - в\nТекст', ['3: after']],
    ['продолжение пункта с отступом — тот же пункт', '- а\n  продолжение\n- б', []],
    ['строки цитаты (и вложенной) — одна цитата', '> а\n> > б\nТекст', ['2: after']],
    ['линия — блок сама по себе', 'а\n\n---\nб', ['1: after', '3: after']],
    ['блок кода: промежуток над ним — у строки выше, под ним — сверху у строки ниже (фон блока не растягивается)', 'Текст\n```\nкод\n```\nТекст', ['1: after', '5: before']],
    ['последняя строка и пустые строки — без промежутка', 'а\n\n\n', ['1: after']],
  ])('%s', (_name, text, expected) => {
    expect(gaps(live(text))).toEqual(expected);
  });

  it('в режиме исходного текста промежутков нет', () => {
    expect(gaps(createEditorState('# З\nТекст\n\n- а', { mode: 'source' }))).toEqual([]);
  });

  it('текст строк не меняется: промежуток — только класс строки', () => {
    const state = stateWithCursor('# З\nТекст|');
    expect(hidden(state)).toEqual(['# ']);
    expect(state.doc.toString()).toBe('# З\nТекст');
  });

  /** Отступы вложенных пунктов: `номер строки: уровень`. */
  function levels(state: EditorState, ranges?: { from: number; to: number }[]): string[] {
    const out: string[] = [];
    previewDecorations(state, ranges).between(0, state.doc.length, (from, to, value) => {
      const cls = (value.spec as { class?: string }).class ?? '';
      const level = /cm-rz-list-indent-(\d)/.exec(cls);
      if (level && from < to) out.push(`${state.doc.lineAt(from).number}: ${level[1]} «${state.sliceDoc(from, to)}»`);
    });
    return out;
  }

  it('вложенный пункт — отступ по уровню, а не по ширине пробелов', () => {
    expect(levels(live('- а\n  - б\n    - в\n- г'))).toEqual(['2: 1 «  »', '3: 2 «    »']);
    expect(levels(live('1. а\n   1. б'))).toEqual(['2: 1 «   »']);
    expect(levels(live('- а\n\t- б\n\t\t- в'))).toEqual(['2: 1 «\t»', '3: 2 «\t\t»']);
  });

  it('пустая строка и продолжение пункта не обрывают список; абзац без отступа — обрывает', () => {
    expect(levels(live('- а\n\n  - б'))).toEqual(['3: 1 «  »']);
    expect(levels(live('- а\n  текст\n  - б'))).toEqual(['3: 1 «  »']);
    expect(levels(live('- а\nТекст\n  - б'))).toEqual([]);
  });

  it('пункт с отступом без родителя — первый уровень не сдвигается', () => {
    expect(levels(live('  - а'))).toEqual([]);
  });

  it('видимая часть начинается внутри списка: уровень считается по пунктам выше', () => {
    const state = live('Текст\n- а\n  - б\n    - в');
    const third = state.doc.line(4);
    expect(levels(state, [{ from: third.from, to: third.to }])).toEqual(['4: 2 «    »']);
  });
});
