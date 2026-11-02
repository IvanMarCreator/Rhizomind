// Строки и код: блоки с оградой ``` и ~~~, код с отступом, код в строке
// (решения 84–87, Р17 п.5) — как `Blocks` и `code_spans` в
// src-tauri/src/markdown/code.rs. Правила те же, что у `paragraphs` и
// `codeSpans` в src/core/wikilinks (образцы tests/fixtures/wikilinks/text.json);
// здесь — по строке за раз, чтобы редактор мог начать разбор с любой строки
// с известным состоянием.

/** Строка текста: начало и конец без перевода строки. */
export interface LineSpan {
  from: number;
  to: number;
}

/** К чему относится строка: пустая, код (ограда, её содержимое, отступ) или текст абзаца. */
export type LineKind = 'blank' | 'code' | 'text';

const BLANK = /^[ \t]*$/;
/** До трёх пробелов, `-`/`*`/`+` или 1–9 цифр с `.`/`)`, затем пробел, табуляция или конец. */
const LIST_ITEM = /^ {0,3}(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)/;

/** Ширина отступа в колонках: табуляция — до следующей кратной 4. */
export function indentColumns(line: string): number {
  let columns = 0;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === ' ') columns += 1;
    else if (ch === '\t') columns += 4 - (columns % 4);
    else break;
  }
  return columns;
}

function skipThreeSpaces(line: string): number {
  let i = 0;
  while (i < 3 && line[i] === ' ') i++;
  return i;
}

/**
 * Открывающая ограда: до трёх пробелов, затем три и больше `` ` `` или `~`;
 * после `` ` `` строка не содержит `` ` ``. Строка с U+2028 или U+2029 — не
 * ограда (как `FENCE_OPEN` и `opens_fence` ядра).
 */
function opensFence(line: string): { mark: string; length: number } | null {
  const start = skipThreeSpaces(line);
  const mark = line[start];
  if (mark !== '`' && mark !== '~') return null;
  let end = start;
  while (line[end] === mark) end++;
  const info = line.slice(end);
  if (end - start < 3 || info.includes(' ') || info.includes(' ') || (mark === '`' && info.includes('`'))) return null;
  return { mark, length: end - start };
}

/** Закрывающая ограда: до трёх пробелов, те же символы не короче, дальше только пробелы и табуляции. */
function closesFence(line: string, mark: string, length: number): boolean {
  const start = skipThreeSpaces(line);
  let end = start;
  while (line[end] === mark) end++;
  return end - start >= length && BLANK.test(line.slice(end));
}

/**
 * Вид строк по порядку: состояние — открытая ограда, вид предыдущей строки,
 * идёт ли список. Строка с отступом в 4 колонки — код, если перед ней пустая
 * строка, код или начало текста и это не продолжение пункта списка.
 */
export class LineClassifier {
  #fence: { mark: string; length: number } | null = null;
  #previous: LineKind | 'start' = 'start';
  #inList = false;

  /** Вид очередной строки (без перевода строки). */
  classify(line: string): LineKind {
    let kind: LineKind;
    if (this.#fence) {
      if (closesFence(line, this.#fence.mark, this.#fence.length)) this.#fence = null;
      kind = 'code';
    } else if (BLANK.test(line)) {
      kind = 'blank';
    } else {
      const indent = indentColumns(line);
      const fence = indent >= 4 && this.#previous !== 'text' && !this.#inList ? 'indented' : opensFence(line);
      if (fence === 'indented') {
        kind = 'code';
      } else if (fence) {
        this.#fence = fence;
        kind = 'code';
      } else {
        if (LIST_ITEM.test(line)) this.#inList = true;
        else if (this.#previous === 'blank' && indent < 2) this.#inList = false;
        kind = 'text';
      }
    }
    this.#previous = kind;
    return kind;
  }

  /**
   * Строка — код блока с оградой, известный снаружи (редактор берёт ограды
   * из своего разбора блоков): состояние — как после строки кода, ограда не
   * открыта.
   */
  fencedCode(): void {
    this.#fence = null;
    this.#previous = 'code';
  }
}

/**
 * Код в строке на участке абзаца `[from, to)`: серия обратных апострофов и
 * следующая серия той же длины (серии берутся целиком); серия без пары — не
 * код. За один проход: для каждой длины — очередь серий этой длины.
 */
export function codeSpans(text: string, from: number, to: number): LineSpan[] {
  let first = from;
  while (first < to && text.charCodeAt(first) !== 0x60) first++;
  if (first >= to) return [];
  const runs: Array<{ at: number; length: number }> = [];
  for (let i = first; i < to; ) {
    if (text[i] !== '`') {
      i++;
      continue;
    }
    let end = i;
    while (end < to && text[end] === '`') end++;
    runs.push({ at: i, length: end - i });
    i = end;
  }
  const byLength = new Map<number, { runs: number[]; next: number }>();
  runs.forEach((run, n) => {
    const queue = byLength.get(run.length);
    if (queue) queue.runs.push(n);
    else byLength.set(run.length, { runs: [n], next: 0 });
  });
  const spans: LineSpan[] = [];
  for (let r = 0; r < runs.length; ) {
    const run = runs[r];
    const queue = run && byLength.get(run.length);
    if (!run || !queue) break;
    while (queue.next < queue.runs.length && (queue.runs[queue.next] ?? 0) <= r) queue.next++;
    const close = queue.runs[queue.next];
    if (close === undefined) {
      r++;
      continue;
    }
    spans.push({ from: run.at, to: (runs[close]?.at ?? 0) + run.length });
    r = close + 1;
  }
  return spans;
}
