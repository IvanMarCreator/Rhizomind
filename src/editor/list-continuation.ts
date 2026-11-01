// Продолжение списков, задач и цитат по Enter и вложенность пунктов по
// Tab / Shift+Tab (решение владельца 92, задача 2.15) — в обоих режимах
// редактора, как в Obsidian:
// - Enter в пункте (`- `, `* `, `+ `, `1. `, `1) `, задача `- [ ] `) или в
//   цитате (`> `, `> > `, `> - `) переносит строку и повторяет префикс:
//   задача — не отмечена, номер — следующий, номера ниже того же уровня
//   сдвигаются; курсор посреди текста разрывает пункт;
// - Enter на пустом пункте: вложенный — на уровень выше, верхнего уровня —
//   маркер удаляется (выход из списка); пустая цитата теряет внутренний `>`;
// - Tab / Shift+Tab в пункте — вложенность пункта вместе с его вложенными
//   строками; шаг — по стилю документа (табуляция или пробелы ближайшего
//   вложенного пункта), по умолчанию табуляция, как в Obsidian;
// - перенумерация (задача 2.21): после Tab / Shift+Tab и после удаления строк
//   (Backspace, Delete, вырезание) нумерованные списки вокруг правки
//   пересчитываются в той же транзакции — каждый уровень своей нумерацией, с
//   номера первого пункта (пункт, только что ставший первым на уровне после
//   Tab / Shift+Tab, — с 1); пустая строка — граница списка;
// - вне списка, в блоке кода и в блоке свойств правила не действуют: Enter
//   обычный, Tab не перехватывается (переход фокуса, как прежде).
// Разбор префиксов — по тем же правилам, что у живого предпросмотра
// (live-preview/markdown.ts). Правка — одна транзакция: Ctrl+Z отменяет
// продолжение одним шагом. Enter при открытом списке подсказок `[[` остаётся
// за автодополнением: его сочетания важнее (Prec.highest).
import {
  EditorSelection,
  EditorState,
  Prec,
  type ChangeSpec,
  type Extension,
  type Line,
  type Transaction,
  type TransactionSpec,
} from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { inCodeAt } from './live-preview/index.ts';
import { fenceStep, type CodeBlock, type OpenFence } from './live-preview/markdown.ts';

/** Цитата с позиции `lastIndex` — как в разборе предпросмотра. */
const QUOTE_AT = / {0,3}> ?/y;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const HEADING = /^ {0,3}#{1,6}(?:[ \t]|$)/;
const LIST = /^([ \t]*)([-*+]|(\d{1,9})([.)]))(?=[ \t]|$)/;
const TASK = /^([ \t]+)\[[ xX]\](?=[ \t]|$)/;
const BLANKS = /^[ \t]*/;

/** Ширина табуляции при сравнении отступов (как `tabSize` редактора по умолчанию). */
const TAB_WIDTH = 4;

/** Сколько строк вокруг смотреть, выбирая шаг отступа. */
const STYLE_SCAN_LINES = 200;

interface ListItem {
  /** Отступ перед маркером. */
  indent: string;
  /** `-`, `*`, `+` или номер с `.`/`)`. */
  bullet: string;
  number: number | null;
  /** Цифры номера как написаны (с ведущими нулями). */
  digits: string;
  delimiter: string;
  /** Пробелы между маркером и `[ ]` (у задачи) или текстом. */
  gap: string;
  /** У задачи — пробелы после `[ ]` (пустые, если строка на ней кончается). */
  taskGap: string | null;
}

interface Prefix {
  /** Начала префиксов цитаты (с их отступом) — для удаления внутреннего уровня. */
  quotes: number[];
  /** Конец префиксов цитаты. */
  quoteEnd: number;
  list: ListItem | null;
  /** Начало текста строки после всех префиксов. */
  contentFrom: number;
}

/** Префиксы строки: цитаты, затем пункт списка. `null` — ни того, ни другого. */
function parsePrefix(text: string): Prefix | null {
  const quotes: number[] = [];
  let pos = 0;
  for (;;) {
    QUOTE_AT.lastIndex = pos;
    const m = QUOTE_AT.exec(text);
    if (!m) break;
    quotes.push(pos);
    pos += m[0].length;
  }
  const quoteEnd = pos;
  const rest = text.slice(pos);
  const list = RULE.test(rest) ? null : LIST.exec(rest);
  if (!list) return quotes.length > 0 ? { quotes, quoteEnd, list: null, contentFrom: quoteEnd } : null;

  const indent = list[1] ?? '';
  const bullet = list[2] ?? '';
  let at = quoteEnd + list[0].length;
  const task = TASK.exec(text.slice(at));
  let gap: string;
  let taskGap: string | null = null;
  if (task) {
    gap = task[1] ?? ' ';
    at += task[0].length;
    taskGap = BLANKS.exec(text.slice(at))?.[0] ?? '';
    at += taskGap.length;
  } else {
    gap = BLANKS.exec(text.slice(at))?.[0] ?? '';
    at += gap.length;
  }
  const digits = list[3] ?? '';
  const number = list[3] === undefined ? null : Number(digits);
  return { quotes, quoteEnd, list: { indent, bullet, number, digits, delimiter: list[4] ?? '', gap, taskGap }, contentFrom: at };
}

/** Ширина отступа в колонках: табуляция — до следующей кратной `TAB_WIDTH`. */
function columns(indent: string): number {
  let width = 0;
  for (const ch of indent) width = ch === '\t' ? width - (width % TAB_WIDTH) + TAB_WIDTH : width + 1;
  return width;
}

/**
 * Строка — внутри блока кода или блока свойств: правила не действуют. Блок
 * кода внутри цитаты (`> ```…`) общий разбор блоков не видит: его ограды
 * ищутся здесь, среди строк выше с тем же уровнем цитаты (без префиксов).
 */
function inBlockLine(state: EditorState, line: Line, prefix: Prefix): boolean {
  if (inCodeAt(state, line.from)) return true;
  const depth = prefix.quotes.length;
  if (depth === 0) return false;
  let first = line.number;
  while (first > 1 && (parsePrefix(state.doc.line(first - 1).text)?.quotes.length ?? 0) === depth) first--;
  let open: OpenFence | null = null;
  const closed: CodeBlock[] = [];
  for (let n = first; n <= line.number; n++) {
    const text = state.doc.line(n).text;
    const body = text.slice(parsePrefix(text)?.quoteEnd ?? 0);
    const wasOpen = open !== null;
    open = fenceStep(open, body, 0, body.length, closed);
    // Сама строка — ограда (открывающая или закрывающая): тоже код.
    if (n === line.number) return wasOpen || open !== null;
  }
  return false;
}

/** Номер с ведущими нулями до ширины `digits` (`01.` → `02.`). */
function formatNumber(value: number, digits: string): string {
  // Нули — только если номер написан с ними: `10.` → `9.`, а не `09.`.
  return digits.startsWith('0') ? String(value).padStart(digits.length, '0') : String(value);
}

/**
 * Пустой пункт сразу под строкой абзаца — не пункт (CommonMark: пустой пункт
 * не прерывает абзац; `Итого` / `-` — заголовок setext или текст абзаца).
 */
function underParagraph(state: EditorState, line: Line, prefix: Prefix): boolean {
  if (line.number === 1) return false;
  const above = state.doc.line(line.number - 1).text;
  const parsed = parsePrefix(above);
  if ((parsed?.quotes.length ?? 0) !== prefix.quotes.length || parsed?.list) return false;
  const body = above.slice(parsed?.quoteEnd ?? 0);
  if (body.trim() === '' || /^[ \t]/.test(body)) return false;
  return !HEADING.test(body) && !RULE.test(body);
}

/** Префикс нового пункта после `item`: тот же маркер, задача не отмечена, номер следующий. */
function nextItemPrefix(item: ListItem): string {
  const bullet = item.number === null ? item.bullet : `${formatNumber(item.number + 1, item.digits)}${item.delimiter}`;
  const gap = item.gap === '' ? ' ' : item.gap;
  if (item.taskGap === null) return `${item.indent}${bullet}${gap}`;
  return `${item.indent}${bullet}${gap}[ ]${item.taskGap === '' ? ' ' : item.taskGap}`;
}

type Replace = { from: number; to: number; insert: string };

/**
 * Новые номера пунктов ниже строки `line` того же уровня, если после неё
 * встал пункт с номером `inserted`.
 */
function renumber(state: EditorState, line: Line, prefix: Prefix, item: ListItem, inserted: number): ChangeSpec[] {
  const changes: Replace[] = [];
  numberRun(state, line.number + 1, prefix.quotes.length, columns(item.indent), item.delimiter, inserted + 1, changes);
  return joinChanges(state, changes);
}

/**
 * Пронумеровать пункты одного уровня, начиная со строки `start`, числами с
 * `expected`. Вложенные строки пропускаются; конец — пустая строка, другой
 * уровень цитаты, строка с отступом не глубже пункта, но не нумерованный
 * пункт с тем же разделителем.
 */
function numberRun(state: EditorState, start: number, depth: number, base: number, delimiter: string, expected: number, out: Replace[]): void {
  let next = expected;
  for (let n = start; n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    const parsed = parsePrefix(line.text);
    if ((parsed?.quotes.length ?? 0) !== depth) return;
    const quoteEnd = parsed?.quoteEnd ?? 0;
    const body = line.text.slice(quoteEnd);
    if (body.trim() === '') return;
    const indent = parsed?.list?.indent ?? BLANKS.exec(body)?.[0] ?? '';
    const width = columns(indent);
    if (width > base) continue;
    const other = parsed?.list;
    if (width < base || !other || other.number === null || other.delimiter !== delimiter) return;
    if (other.number !== next) {
      const from = line.from + quoteEnd + other.indent.length;
      out.push({ from, to: from + other.digits.length, insert: formatNumber(next, other.digits) });
    }
    next++;
  }
}

/**
 * Первая строка нумерованного списка того же уровня, в котором стоит пункт
 * строки `n` (вверх, пропуская вложенные строки; границы — как у `numberRun`).
 */
function runFirst(state: EditorState, n: number, depth: number, base: number, delimiter: string): number {
  let first = n;
  for (let k = n - 1; k >= 1; k--) {
    const text = state.doc.line(k).text;
    const parsed = parsePrefix(text);
    if ((parsed?.quotes.length ?? 0) !== depth) break;
    const body = text.slice(parsed?.quoteEnd ?? 0);
    if (body.trim() === '') break;
    const width = columns(parsed?.list?.indent ?? BLANKS.exec(body)?.[0] ?? '');
    if (width > base) continue;
    const other = parsed?.list;
    if (width < base || !other || other.number === null || other.delimiter !== delimiter) break;
    first = k;
  }
  return first;
}

/**
 * Перенумеровать списки, в которых стоят строки `anchors` (задача 2.21): каждый
 * уровень — своей нумерацией, с номера первого пункта; если первый пункт — в
 * `fromOne` (пункт только что перенесён на этот уровень), — с 1. Замены —
 * одним изменением участка.
 */
function renumberRuns(state: EditorState, anchors: Iterable<number>, fromOne: ReadonlySet<number> = new Set()): Replace[] {
  const changes: Replace[] = [];
  const done = new Set<number>();
  for (const n of anchors) {
    if (n < 1 || n > state.doc.lines) continue;
    const line = state.doc.line(n);
    const prefix = parsePrefix(line.text);
    const item = prefix?.list;
    if (!prefix || !item || item.number === null || inBlockLine(state, line, prefix)) continue;
    const depth = prefix.quotes.length;
    const base = columns(item.indent);
    const first = runFirst(state, n, depth, base, item.delimiter);
    if (done.has(first)) continue;
    done.add(first);
    const start = fromOne.has(first) ? 1 : (parsePrefix(state.doc.line(first).text)?.list?.number ?? 1);
    numberRun(state, first, depth, base, item.delimiter, start, changes);
  }
  return changes.sort((a, b) => a.from - b.from);
}

/**
 * Позиция после замен номеров `changes` (по порядку). Замены сливаются в одно
 * изменение участка (`joinChanges`), и CodeMirror перенёс бы курсор внутри
 * участка к его краю — поэтому выделение переносится по каждой замене.
 */
function mapThrough(changes: readonly Replace[], pos: number): number {
  let shift = 0;
  for (const change of changes) {
    if (change.from >= pos) break;
    shift += change.to <= pos ? change.insert.length - (change.to - change.from) : Math.min(pos, change.from + change.insert.length) - pos;
  }
  return pos + shift;
}

function mapSelection(selection: EditorSelection, changes: readonly Replace[]): EditorSelection {
  return EditorSelection.create(
    selection.ranges.map((range) => EditorSelection.range(mapThrough(changes, range.anchor), mapThrough(changes, range.head))),
    selection.mainIndex,
  );
}

/**
 * Замены по порядку — одним изменением участка от первой до последней: у
 * длинного списка тысячи отдельных правок CodeMirror применяет заметно дольше.
 */
function joinChanges(state: EditorState, changes: readonly Replace[]): ChangeSpec[] {
  const first = changes[0];
  const last = changes.at(-1);
  if (!first || !last || changes.length === 1) return [...changes];
  const parts: string[] = [];
  let pos = first.from;
  for (const change of changes) {
    parts.push(state.doc.sliceString(pos, change.from), change.insert);
    pos = change.to;
  }
  return [{ from: first.from, to: last.to, insert: parts.join('') }];
}

/**
 * Enter по правилам продолжения (решение 92): правка или `null` — правило
 * здесь не действует, нужен обычный Enter.
 */
export function continueMarkup(state: EditorState): TransactionSpec | null {
  if (state.readOnly || state.selection.ranges.length !== 1) return null;
  const range = state.selection.main;
  if (!range.empty) return null;
  const line = state.doc.lineAt(range.head);
  const prefix = parsePrefix(line.text);
  if (!prefix || range.head - line.from < prefix.contentFrom) return null;
  if (inBlockLine(state, line, prefix)) return null;

  const { list } = prefix;
  if (line.text.slice(prefix.contentFrom).trim() === '') {
    return list && underParagraph(state, line, prefix) ? null : exitEmpty(state, line, prefix);
  }

  const insert = `\n${line.text.slice(0, prefix.quoteEnd)}${list ? nextItemPrefix(list) : ''}`;
  const specs: ChangeSpec[] = [{ from: range.head, insert }];
  if (list?.number != null) specs.push(...renumber(state, line, prefix, list, list.number + 1));
  const changes = state.changes(specs);
  return {
    changes,
    selection: EditorSelection.cursor(changes.mapPos(range.head, 1)),
    scrollIntoView: true,
    userEvent: 'input',
  };
}

/** Enter на пустом пункте: вложенный — на уровень выше, иначе префикс удаляется. */
function exitEmpty(state: EditorState, line: Line, prefix: Prefix): TransactionSpec {
  const { list } = prefix;
  let change: { from: number; to: number; insert: string };
  if (list && list.indent !== '') {
    const from = line.from + prefix.quoteEnd;
    change = { from, to: from + list.indent.length, insert: outdented(list.indent, indentUnit(state, line)) };
  } else if (list) {
    change = { from: line.from + prefix.quoteEnd, to: line.to, insert: '' };
  } else {
    change = { from: line.from + (prefix.quotes.at(-1) ?? 0), to: line.to, insert: '' };
  }
  const changes = state.changes(change);
  return {
    changes,
    selection: EditorSelection.cursor(changes.mapPos(line.to, 1)),
    scrollIntoView: true,
    userEvent: 'delete',
  };
}

/** Отступ на один шаг меньше: одна табуляция или до ширины шага пробелов. */
function outdented(indent: string, unit: string): string {
  if (indent.startsWith('\t')) return indent.slice(1);
  const limit = unit === '\t' ? TAB_WIDTH : unit.length;
  let cut = 0;
  while (cut < limit && indent[cut] === ' ') cut++;
  return indent.slice(cut);
}

/**
 * Шаг отступа списка по стилю документа: ближайший к строке вложенный пункт
 * (в пределах сплошного блока строк) — табуляция или пробелы; ширина
 * пробелов — наименьший ненулевой отступ пунктов блока. Нет вложенных
 * пунктов — табуляция, как в Obsidian по умолчанию.
 */
function indentUnit(state: EditorState, line: Line): string {
  const indents: string[] = [];
  const look = (n: number): boolean => {
    const text = state.doc.line(n).text;
    if (text.trim() === '') return false;
    const indent = parsePrefix(text)?.list?.indent ?? '';
    if (indent !== '') indents.push(indent);
    return true;
  };
  look(line.number);
  let up = line.number - 1;
  let down = line.number + 1;
  let upOpen = true;
  let downOpen = true;
  for (let step = 0; step < STYLE_SCAN_LINES && (upOpen || downOpen); step++) {
    if (upOpen) upOpen = up >= 1 && look(up--);
    if (downOpen) downOpen = down <= state.doc.lines && look(down++);
  }
  const nearest = indents[0];
  if (nearest === undefined || nearest.startsWith('\t')) return '\t';
  const widths = indents.filter((indent) => !indent.startsWith('\t')).map((indent) => BLANKS.exec(indent)?.[0].length ?? 0);
  const width = Math.min(...widths.filter((w) => w > 0));
  return Number.isFinite(width) ? ' '.repeat(width) : '\t';
}

/**
 * Tab (`1`) или Shift+Tab (`-1`): сдвинуть пункты списка под выделением
 * вместе с их вложенными строками. `null` — под выделением нет пунктов,
 * клавиша остаётся окну (переход фокуса).
 */
export function indentList(state: EditorState, direction: 1 | -1): TransactionSpec | null {
  if (state.readOnly) return null;
  const targets = new Set<number>();
  for (const range of state.selection.ranges) {
    const first = state.doc.lineAt(range.from).number;
    const last = state.doc.lineAt(range.to).number;
    for (let n = first; n <= last; n++) {
      const line = state.doc.line(n);
      const prefix = parsePrefix(line.text);
      if (!prefix?.list || inBlockLine(state, line, prefix)) continue;
      targets.add(n);
      addChildren(state, n, prefix, prefix.list, targets);
    }
  }
  if (targets.size === 0) return null;

  const main = state.doc.lineAt(state.selection.main.head);
  const unit = indentUnit(state, main);
  const changes: ChangeSpec[] = [];
  /** Строки, отступ которых действительно изменился. */
  const shiftedLines = new Set<number>();
  for (const n of [...targets].sort((a, b) => a - b)) {
    const line = state.doc.line(n);
    const quoteEnd = parsePrefix(line.text)?.quoteEnd ?? 0;
    const from = line.from + quoteEnd;
    if (direction > 0) {
      changes.push({ from, insert: unit });
      shiftedLines.add(n);
    } else {
      const indent = BLANKS.exec(line.text.slice(quoteEnd))?.[0] ?? '';
      const rest = outdented(indent, unit);
      if (rest.length < indent.length) {
        changes.push({ from, to: from + indent.length - rest.length });
        shiftedLines.add(n);
      }
    }
  }
  // Перенумерация (задача 2.21): списки, куда пункты встали и откуда ушли, —
  // в той же правке (Ctrl+Z отменяет одним шагом). Сдвинутый пункт, ставший
  // первым на новом уровне, и пункт под сдвинутыми, ставший первым на старом,
  // нумеруются с 1.
  const moved = state.changes(changes);
  const shifted = state.update({ changes: moved }).state;
  const sorted = [...shiftedLines].sort((a, b) => a - b);
  const anchors = new Set(sorted);
  for (const n of sorted) {
    const after = n + 1;
    if (shiftedLines.has(after) || after > state.doc.lines) continue;
    const oldWidth = listWidth(state, groupFirst(shiftedLines, n));
    if (oldWidth !== null && listWidth(state, after) === oldWidth) anchors.add(after);
  }
  const fromOne = new Set(anchors);
  const numbers = renumberRuns(shifted, anchors, fromOne);
  const renumbered = shifted.changes(joinChanges(shifted, numbers));
  return {
    changes: moved.compose(renumbered),
    selection: mapSelection(shifted.selection, numbers),
    userEvent: direction > 0 ? 'input.indent' : 'delete.dedent',
  };
}

/** Первая строка сплошной группы строк `lines`, в которой стоит `n`. */
function groupFirst(lines: ReadonlySet<number>, n: number): number {
  let k = n;
  while (lines.has(k - 1)) k--;
  return k;
}

/** Отступ пункта списка на строке `n` в колонках; `null` — не пункт. */
function listWidth(state: EditorState, n: number): number | null {
  const item = parsePrefix(state.doc.line(n).text)?.list;
  return item ? columns(item.indent) : null;
}

/**
 * Удаление строк (Backspace, Delete, вырезание — `delete.*`, кроме Shift+Tab):
 * нумерованный список вокруг места удаления перенумеровывается в той же
 * транзакции (задача 2.21). Удаление внутри одной строки не трогает номера.
 */
const renumberAfterDelete = EditorState.transactionFilter.of((tr: Transaction) => {
  if (!tr.docChanged || !tr.isUserEvent('delete') || tr.isUserEvent('delete.dedent')) return tr;
  const anchors: number[] = [];
  tr.changes.iterChanges((fromA, toA, fromB) => {
    if (toA > fromA && tr.startState.doc.sliceString(fromA, toA).includes('\n')) {
      const n = tr.newDoc.lineAt(fromB).number;
      anchors.push(n, n + 1);
    }
  });
  if (anchors.length === 0) return tr;
  const state = tr.state;
  const numbers = renumberRuns(state, anchors);
  if (numbers.length === 0) return tr;
  return [tr, { changes: joinChanges(state, numbers), selection: mapSelection(state.selection, numbers), sequential: true }];
});

/** Вложенные строки пункта: ниже него, с тем же уровнем цитаты и отступом глубже. */
function addChildren(state: EditorState, n: number, prefix: Prefix, item: ListItem, out: Set<number>): void {
  const base = columns(item.indent);
  for (let k = n + 1; k <= state.doc.lines; k++) {
    const text = state.doc.line(k).text;
    const parsed = parsePrefix(text);
    if ((parsed?.quotes.length ?? 0) !== prefix.quotes.length) return;
    const body = text.slice(parsed?.quoteEnd ?? 0);
    if (body.trim() === '' || columns(BLANKS.exec(body)?.[0] ?? '') <= base) return;
    out.add(k);
  }
}

/** Идёт набор через IME (в том числе только что начатый): не вмешиваться. */
const composing = (view: EditorView): boolean => view.composing || view.compositionStarted;

function run(view: EditorView, spec: TransactionSpec | null): boolean {
  if (!spec) return false;
  view.dispatch(spec);
  return true;
}

/**
 * Расширение: Enter, Tab, Shift+Tab и перенумерация после удаления строк.
 * Сочетания важнее сочетаний по умолчанию (`Prec.high`), но ниже списка
 * подсказок вики-ссылок (`Prec.highest`).
 */
export function listContinuation(): Extension {
  return [
    renumberAfterDelete,
    Prec.high(
      keymap.of([
        { key: 'Enter', run: (view) => !composing(view) && run(view, continueMarkup(view.state)) },
        { key: 'Tab', run: (view) => !composing(view) && run(view, indentList(view.state, 1)) },
        { key: 'Shift-Tab', run: (view) => !composing(view) && run(view, indentList(view.state, -1)) },
      ]),
    ),
  ];
}
