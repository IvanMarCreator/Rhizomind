// Скорость живого предпросмотра на больших заметках и неудобных строках
// (задача 2.5а, замечания QA). Цель — ≤ 5 мс на нажатие или движение курсора;
// предел в тестах втрое больше, с запасом на медленные машины CI. Прежние
// значения — сотни миллисекунд, так что предел ловит возврат к ним.
import { EditorSelection, EditorState } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { codeBlocks, previewDecorations } from './index.ts';
import { MAX_BLOCK_TEXT } from './markdown.ts';

const LIMIT_MS = 15;
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
 * в каждом прогоне — его предел ловит.
 */
function best(fn: () => void, runs = 7): number {
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

  it('открывается в живом предпросмотре не дольше исходного текста + 20 %', () => {
    const text = dense(30 * MB);
    const open = (mode: 'source' | 'live_preview') => () => {
      const state = createEditorState(text, { mode });
      previewDecorations(state, [{ from: 0, to: state.doc.line(60).to }]);
    };
    // Попеременно и по лучшему из прогонов: один замер на CI шумит на ±20 %.
    // Прежний разбор всего текста давал ×2,5 — предел его ловит.
    let source = Infinity;
    let live = Infinity;
    const once = (fn: () => void) => {
      const start = performance.now();
      fn();
      return performance.now() - start;
    };
    for (let k = 0; k < 5; k++) {
      source = Math.min(source, once(open('source')));
      live = Math.min(live, once(open('live_preview')));
    }
    expect(live).toBeLessThan(source * 1.2 + 20);
  });
});

describe('неудобная строка ~10 000 символов: оформление при движении курсора линейно', () => {
  const fill = (piece: string) => piece.repeat(Math.floor(9_990 / piece.length));
  it.each([
    ['[a без ]', fill('[a')],
    ['[a и одна ] в конце', `${'[a'.repeat(4_990)}]`],
    ['[a](', fill('[a](')],
    ['[a](\\(', fill('[a](\\(')],
    ['[a](b (', fill('[a](b (')],
    ['[a](<', fill('[a](<')],
    ['``a`', fill('``a`')],
    ['*a', fill('*a')],
    ['a* ', fill('a* ')],
    ['~~a', fill('~~a')],
    ['# и пробелы', `# a${' '.repeat(9_990)}a`],
    ['> ', fill('> ')],
    // Вики-ссылки (задача 2.6): пары `[[` → `]]` — один проход по строке.
    ['[[a без ]]', fill('[[a')],
    ['[[a и одна ]] в конце', `${'[[a'.repeat(3_320)}]]`],
    ['[[a]', fill('[[a]')],
    ['[[a`]]', fill('[[a`]]')],
    ['[[`a]] с парой ` в конце', `${'[[`a]]'.repeat(1_660)}\``],
    ['[[a]]', fill('[[a]]')],
    ['[[a#b|c]]', fill('[[a#b|c]]')],
    ['![[a', fill('![[a')],
  ])('%s', (_name, line) => {
    const state = createEditorState(`курсор\n${line}\n`, { mode: 'live_preview' });
    const moves = [0, state.doc.length - 3];
    let k = 0;
    const time = best(() => {
      const next = state.update({ selection: EditorSelection.cursor(moves[k++ % 2] ?? 0) }).state;
      previewDecorations(next);
    });
    expect(time).toBeLessThan(LIMIT_MS);
  });
});
