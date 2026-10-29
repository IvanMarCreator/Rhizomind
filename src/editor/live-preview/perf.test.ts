// Скорость живого предпросмотра на больших заметках и неудобных строках
// (задача 2.5а, замечания QA). Цель — ≤ 5 мс на нажатие или движение курсора;
// предел в тестах втрое больше, с запасом на медленные машины CI. Прежние
// значения — сотни миллисекунд, так что предел ловит возврат к ним.
import { EditorSelection, EditorState, Text } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { codeBlocks, previewDecorations } from './index.ts';
import { MAX_BLOCK_TEXT } from './markdown.ts';

const LIMIT_MS = 15;
/** Рост времени при вчетверо более длинной строке: линейно ≈ ×4, квадратично ≈ ×16. */
const MAX_GROWTH = 8;
/** Абсолютный предел для неудобных строк — с запасом на нагрузку параллельных тестов. */
const SLOW_LIMIT_MS = 50;
const MB = 1024 * 1024;

/** Плотный Markdown: заголовок, выделение, ссылка, списки, цитата и блок кода на каждые ~150 символов. */
function dense(size: number, fence = '```js'): string {
  const parts: string[] = [];
  let length = 0;
  for (let i = 0; length < size; i++) {
    const chunk = `## Раздел ${i}\n\nТекст с **жирным** и *курсивом*, \`код\` и [ссылка](https://example.org/${i}).\n- пункт\n- [ ] задача\n> цитата\n\n${fence}\nconst x = ${i};\n\`\`\`\n\n`;
    parts.push(chunk);
    length += chunk.length;
  }
  return parts.join('');
}

/**
 * Лучший из нескольких замеров, мс; первые два прогона — разогрев (в окне код
 * уже разогрет). Лучший, а не средний: при параллельных тестах отдельный
 * прогон бывает в разы дольше из-за нагрузки, а квадратичный разбор медленен
 * в каждом прогоне — его предел ловит. 15 прогонов по нескольку миллисекунд:
 * при 7 под сильной нагрузкой (три полных прогона тестов на 2 ядрах) лучший
 * однажды вышел 17,8 мс при обычных ≤ 4,4.
 */
function best(fn: () => void, runs = 15): number {
  fn();
  fn();
  let time = Infinity;
  for (let k = 0; k < runs; k++) {
    const start = performance.now();
    fn();
    time = Math.min(time, performance.now() - start);
  }
  return time;
}

/** Правка и оформление видимой части (~60 строк от места правки) — то, что делает окно на нажатие. */
function keystroke(state: EditorState, from: number, to: number, insert: string): () => void {
  return () => {
    const next = state.update({ changes: { from, to, insert } }).state;
    const line = next.doc.lineAt(from);
    previewDecorations(next, [{ from: line.from, to: next.doc.line(Math.min(next.doc.lines, line.number + 60)).to }]);
  };
}

describe('ввод в заметке почти 10 МБ (блоки кода размечаются)', { timeout: 60_000 }, () => {
  const text = dense(MAX_BLOCK_TEXT - 64 * 1024);
  const state = createEditorState(text, { mode: 'live_preview' });

  it('блоки кода размечены (иначе замеры ниже ничего не проверяют)', () => {
    expect(text.length).toBeLessThanOrEqual(MAX_BLOCK_TEXT);
    expect(codeBlocks(state).code.length).toBeGreaterThan(10_000);
  });

  it('в первой строке — без пересчёта блоков всего текста', () => {
    expect(best(keystroke(state, 5, 5, 'x'))).toBeLessThan(LIMIT_MS);
  });

  it('название языка в строке-ограде посередине — без пересчёта всего текста', () => {
    const at = text.indexOf('```js\n', text.length / 2) + 5;
    expect(best(keystroke(state, at, at, 'x'))).toBeLessThan(LIMIT_MS);
  });

  it('новая ограда посередине и удалённая ограда — пересчёт до первого совпадения с прежним разбором', () => {
    const line = text.indexOf('```js\n', text.length / 2);
    expect(best(keystroke(state, line, line, '```\n'))).toBeLessThan(LIMIT_MS);
    expect(best(keystroke(state, line, line + 1, ''))).toBeLessThan(LIMIT_MS);
  });
});

describe('заметка больше 10 МБ (решение 77)', { timeout: 120_000 }, () => {
  it('блоки кода не размечаются; ввод через границу 10 МБ выключает и снова включает их', () => {
    const big = createEditorState(dense(MAX_BLOCK_TEXT + 1000), { mode: 'live_preview' });
    expect(codeBlocks(big).code).toEqual([]);

    const text = dense(MAX_BLOCK_TEXT - 64 * 1024);
    const small = createEditorState(text, { mode: 'live_preview' });
    const blocks = codeBlocks(small).code.length;
    expect(blocks).toBeGreaterThan(0);
    const pad = 'а'.repeat(MAX_BLOCK_TEXT - text.length + 1);
    const over = small.update({ changes: { from: 0, insert: pad } }).state;
    expect(codeBlocks(over).code).toEqual([]);
    const back = over.update({ changes: { from: 0, to: pad.length } }).state;
    expect(codeBlocks(back).code).toHaveLength(blocks);
  });

  // Открытие заметки 30 МБ: прежний код (main 12c7f4f) и код без порога 10 МБ
  // разбирают весь текст.
  const text = dense(30 * MB);

  it('открывается в живом предпросмотре без чтения всего текста', () => {
    // Счётчик работы, а не время: не зависит от нагрузки на машину. Сейчас
    // читается видимая часть (единицы КБ), при разборе всего текста — 30 МБ.
    // Проверки временем нет: счётчик видит любое чтение текста, а с решения
    // 87 поле блоков работает и в исходном тексте — сравнивать не с чем.
    const read = charsRead(() => {
      const state = createEditorState(text, { mode: 'live_preview' });
      previewDecorations(state, [{ from: 0, to: state.doc.line(60).to }]);
    });
    expect(read).toBeGreaterThan(0);
    expect(read).toBeLessThan(text.length / 100);
  });
});

/**
 * Сколько символов текста прочитано за время `fn` — любым путём: `line`,
 * `sliceString`, `toString`, `iter`, `iterRange`, `iterLines` и прочее в
 * CodeMirror в итоге берут строки из массивов `text` листьев документа.
 * Документ, созданный за это время (`Text.of`), получает листья, у которых
 * массив строк считает каждое чтение строки по номеру. Общий пустой документ
 * `Text.empty` (его возвращает `Text.of([''])`) не трогается: обёртка осталась
 * бы в нём и после замера.
 */
function charsRead(fn: () => void): number {
  const original = Text.of;
  const of = original.bind(Text);
  let read = 0;
  const counting = (lines: string[]) =>
    new Proxy(lines, {
      get(target, key, receiver) {
        const value: unknown = Reflect.get(target, key, receiver);
        if (typeof key === 'string' && typeof value === 'string' && /^\d+$/.test(key)) read += value.length + 1;
        return value;
      },
    });
  const wrap = (node: Text) => {
    const tree = node as unknown as { children: readonly Text[] | null; text?: string[] };
    if (tree.children) tree.children.forEach(wrap);
    else if (tree.text) tree.text = counting(tree.text);
  };
  Text.of = (lines: readonly string[]) => {
    const doc = of(lines);
    if (doc !== Text.empty) wrap(doc);
    return doc;
  };
  try {
    fn();
  } finally {
    Text.of = original;
  }
  return read;
}

describe('неудобная строка ~10 000 символов: оформление при движении курсора линейно', () => {
  // Строка из повторов `piece` длиной не больше `n`.
  const fill = (piece: string) => (n: number) => piece.repeat(Math.floor(n / piece.length));
  const cases: [string, (n: number) => string][] = [
    ['[a без ]', fill('[a')],
    ['[a и одна ] в конце', (n) => `${'[a'.repeat(Math.floor(n / 2))}]`],
    ['[a](', fill('[a](')],
    ['[a](\\(', fill('[a](\\(')],
    ['[a](b (', fill('[a](b (')],
    ['[a](<', fill('[a](<')],
    ['``a`', fill('``a`')],
    ['*a', fill('*a')],
    ['a* ', fill('a* ')],
    ['~~a', fill('~~a')],
    ['# и пробелы', (n) => `# a${' '.repeat(n)}a`],
    ['> ', fill('> ')],
    // Вики-ссылки (задача 2.6): пары `[[` → `]]` — один проход по строке.
    ['[[a без ]]', fill('[[a')],
    ['[[a и одна ]] в конце', (n) => `${'[[a'.repeat(Math.floor(n / 3))}]]`],
    ['[[a]', fill('[[a]')],
    ['[[a`]]', fill('[[a`]]')],
    ['[[`a]] с парой ` в конце', (n) => `${'[[`a]]'.repeat(Math.floor(n / 6))}\``],
    ['[[a]]', fill('[[a]]')],
    ['[[a#b|c]]', fill('[[a#b|c]]')],
    ['![[a', fill('![[a')],
  ];
  it.each(cases)('%s', (_name, make) => {
    // Рост: та же строка вчетверо короче, замеры попеременно (нагрузка на
    // машину одна и та же). Линейный разбор — около ×4 (сейчас ×2,5–3,9),
    // квадратичный — около ×16 (код 12c7f4f — ×13–17). Сравнение с самой собой,
    // а не с другой строкой: квадратичный разбор в общем пути (вики-ссылки)
    // замедлил бы и опорную строку, отношение осталось бы ≈ 1. Абсолютный
    // предел — дополнительный, с запасом на нагрузку: под ней лучший замер
    // бывал 16–26 мс при обычных ≤ 4,4.
    const { time, ratio } = bestVersus(cursorMoves(make(9_990)), cursorMoves(make(9_990 / 4)), 25);
    const note = `${time.toFixed(2)} мс, рост ×${ratio.toFixed(1)} к строке вчетверо короче`;
    expect(ratio, note).toBeLessThan(MAX_GROWTH);
    expect(time, note).toBeLessThan(SLOW_LIMIT_MS);
  });
});

/** Движение курсора между первой и второй строкой и оформление всей заметки. */
function cursorMoves(line: string): () => void {
  const state = createEditorState(`курсор\n${line}\n`, { mode: 'live_preview' });
  const moves = [0, state.doc.length - 3];
  let k = 0;
  return () => {
    const next = state.update({ selection: EditorSelection.cursor(moves[k++ % 2] ?? 0) }).state;
    previewDecorations(next);
  };
}

/** Лучшие из попеременных замеров `fn` и `base`: время `fn` и его отношение к `base`. */
function bestVersus(fn: () => void, base: () => void, runs = 15): { time: number; ratio: number } {
  fn();
  fn();
  base();
  base();
  let time = Infinity;
  let baseTime = Infinity;
  for (let k = 0; k < runs; k++) {
    let start = performance.now();
    fn();
    time = Math.min(time, performance.now() - start);
    start = performance.now();
    base();
    baseTime = Math.min(baseTime, performance.now() - start);
  }
  return { time, ratio: time / Math.max(baseTime, 0.01) };
}
