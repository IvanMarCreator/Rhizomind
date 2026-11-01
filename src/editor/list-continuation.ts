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
// - вне списка, в блоке кода и в блоке свойств правила не действуют: Enter
//   обычный, Tab не перехватывается (переход фокуса, как прежде).
// Разбор префиксов — по тем же правилам, что у живого предпросмотра
// (live-preview/markdown.ts). Правка — одна транзакция: Ctrl+Z отменяет
// продолжение одним шагом. Enter при открытом списке подсказок `[[` остаётся
// за автодополнением: его сочетания важнее (Prec.highest).
import { EditorSelection, Prec, type ChangeSpec, type EditorState, type Extension, type Line, type TransactionSpec } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { inCodeAt } from './live-preview/index.ts';

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
  const list = RULE.test(rest) || HEADING.test(rest) ? null : LIST.exec(rest);
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
  const number = list[3] === undefined ? null : Number(list[3]);
  return { quotes, quoteEnd, list: { indent, bullet, number, delimiter: list[4] ?? '', gap, taskGap }, contentFrom: at };
}

/** Ширина отступа в колонках: табуляция — до следующей кратной `TAB_WIDTH`. */
function columns(indent: string): number {
  let width = 0;
  for (const ch of indent) width = ch === '\t' ? width - (width % TAB_WIDTH) + TAB_WIDTH : width + 1;
  return width;
}

/** Строка — внутри блока кода или блока свойств: правила не действуют. */
function inBlockLine(state: EditorState, line: Line): boolean {
  return inCodeAt(state, line.from);
}

/** Префикс нового пункта после `item`: тот же маркер, задача не отмечена, номер следующий. */
function nextItemPrefix(item: ListItem): string {
  const bullet = item.number === null ? item.bullet : `${item.number + 1}${item.delimiter}`;
  const gap = item.gap === '' ? ' ' : item.gap;
  if (item.taskGap === null) return `${item.indent}${bullet}${gap}`;
  return `${item.indent}${bullet}${gap}[ ]${item.taskGap === '' ? ' ' : item.taskGap}`;
}

/**
 * Новые номера пунктов ниже строки `line` того же уровня, если после неё
 * встал пункт с номером `inserted`. Вложенные строки пропускаются; конец —
 * пустая строка, другой уровень цитаты или строка с отступом не глубже пункта.
 */
function renumber(state: EditorState, line: Line, prefix: Prefix, item: ListItem, inserted: number): ChangeSpec[] {
  const changes: ChangeSpec[] = [];
  const base = columns(item.indent);
  let expected = inserted + 1;
  for (let n = line.number + 1; n <= state.doc.lines; n++) {
    const next = state.doc.line(n);
    const parsed = parsePrefix(next.text);
    const depth = parsed?.quotes.length ?? 0;
    if (depth !== prefix.quotes.length) break;
    const quoteEnd = parsed?.quoteEnd ?? 0;
    const body = next.text.slice(quoteEnd);
    if (body.trim() === '') break;
    const indent = parsed?.list?.indent ?? BLANKS.exec(body)?.[0] ?? '';
    const width = columns(indent);
    if (width > base) continue;
    const other = parsed?.list;
    if (width < base || !other || other.number === null || other.delimiter !== item.delimiter) break;
    if (other.number !== expected) {
      const from = next.from + quoteEnd + other.indent.length;
      changes.push({ from, to: from + String(other.number).length, insert: String(expected) });
    }
    expected++;
  }
  return changes;
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
  if (inBlockLine(state, line)) return null;

  const { list } = prefix;
  if (prefix.contentFrom >= line.length) return exitEmpty(state, line, prefix);

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
      if (!prefix?.list || inBlockLine(state, line)) continue;
      targets.add(n);
      addChildren(state, n, prefix, prefix.list, targets);
    }
  }
  if (targets.size === 0) return null;

  const main = state.doc.lineAt(state.selection.main.head);
  const unit = indentUnit(state, main);
  const changes: ChangeSpec[] = [];
  for (const n of [...targets].sort((a, b) => a - b)) {
    const line = state.doc.line(n);
    const quoteEnd = parsePrefix(line.text)?.quoteEnd ?? 0;
    const from = line.from + quoteEnd;
    if (direction > 0) {
      changes.push({ from, insert: unit });
    } else {
      const indent = BLANKS.exec(line.text.slice(quoteEnd))?.[0] ?? '';
      const rest = outdented(indent, unit);
      if (rest.length < indent.length) changes.push({ from, to: from + indent.length - rest.length });
    }
  }
  return { changes, userEvent: direction > 0 ? 'input.indent' : 'delete.dedent' };
}

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
 * Расширение: Enter, Tab, Shift+Tab. Важнее сочетаний по умолчанию
 * (`Prec.high`), но ниже списка подсказок вики-ссылок (`Prec.highest`).
 */
export function listContinuation(): Extension {
  return Prec.high(
    keymap.of([
      { key: 'Enter', run: (view) => !composing(view) && run(view, continueMarkup(view.state)) },
      { key: 'Tab', run: (view) => !composing(view) && run(view, indentList(view.state, 1)) },
      { key: 'Shift-Tab', run: (view) => !composing(view) && run(view, indentList(view.state, -1)) },
    ]),
  );
}
