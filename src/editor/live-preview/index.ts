// Живой предпросмотр (С3.3, ADR 0001): оформление поверх текста. Документ
// редактора не меняется — разметка только прячется или приглушается. По
// решению 93 раскрыт (разметка видна приглушённо) только элемент, который
// задевает курсор или выделение, включая его края: строчный элемент — сам по
// себе, маркер блока (`#`, `>`, `-`, `1.`, `[ ]`) — когда курсор где угодно
// на его строке. Режим исходного текста — без этого расширения: вся разметка
// видна как есть.
//
// Ссылки `[текст](адрес)` — без элементов `<a>` и переходов в окне: щелчок
// передаёт адрес наверх, окно открывает его командой ядра `open_external`
// с подтверждением (С3.4, docs/SECURITY.md). Вики-ссылки (задача 2.6) —
// только переход к заметке хранилища через `WikiHost`, никогда не
// `open_external`. Щелчок открывает нераскрытую ссылку, раскрытую (курсор в
// ней или у её края) — Ctrl+щелчок (решения 76 и 93); в режиме исходного
// текста — Ctrl+щелчок (`sourceLinks`).
import {
  Facet,
  RangeSet,
  RangeValue,
  StateField,
  type ChangeSet,
  type EditorState,
  type Extension,
  type Range,
  type Text,
  type TransactionSpec,
} from '@codemirror/state';
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import { t } from '../../i18n/index.ts';
import { wikiHost } from '../wikilinks/host.ts';
import { isUnresolved, resolutionField, resolvedEffect, staleEffect, type Resolution } from '../wikilinks/resolve.ts';
import {
  MAX_BLOCK_TEXT,
  PROPERTIES_CLOSE,
  PROPERTIES_OPEN,
  fenceStep,
  findProperties,
  isFenceLike,
  lineShape,
  openFenceAt,
  parseLine,
  scanBlocks,
  type Blocks,
  type CodeBlock,
  type LineKind,
  type LineSource,
  type MarkPiece,
  type OpenFence,
  type Span,
  type WidgetKind,
} from './markdown.ts';
import { sourceTags, tagPreview } from './tags.ts';

// ---- Блоки кода и блок свойств ----------------------------------------------

interface BlockState {
  /** Закрытые блоки кода: набор диапазонов CodeMirror сдвигается за правкой без перебора всех блоков. */
  code: RangeSet<RangeValue>;
  /** Начало незакрытого блока: он идёт до конца текста. */
  unclosed: number | null;
  properties: Span | null;
}

const CODE_BLOCK = new (class extends RangeValue {})();
const NO_BLOCKS: BlockState = { code: RangeSet.empty, unclosed: null, properties: null };

function scan(doc: Text): BlockState {
  const blocks = scanBlocks(doc);
  const closed = blocks.code.filter((block) => block.closeLine !== null);
  const open = blocks.code.find((block) => block.closeLine === null);
  return {
    properties: blocks.properties,
    code: RangeSet.of(closed.map((block) => CODE_BLOCK.range(block.from, block.to))),
    unclosed: open ? open.from : null,
  };
}

/** Пересчитать всё заново: правка задела границу блока свойств. */
const RESCAN = Symbol('rescan');

/**
 * Блоки, зависящие от строк выше. Обычная правка только сдвигает позиции.
 * Правка строки-ограды пересчитывает ограды с места правки до первой строки,
 * где новый разбор совпал с прежним (`rescanFences`). Весь текст заново —
 * только если сдвинулась граница блока свойств. Текст больше
 * `MAX_BLOCK_TEXT` — без блоков (решение 77).
 */
const blocksField = StateField.define<BlockState>({
  create: (state) => scan(state.doc),
  update(blocks, tr) {
    if (!tr.docChanged) return blocks;
    const before = tr.startState.doc;
    const doc = tr.state.doc;
    if (doc.length > MAX_BLOCK_TEXT) return NO_BLOCKS;
    if (before.length > MAX_BLOCK_TEXT) return scan(doc);
    const properties = updateProperties(blocks.properties, tr.changes, before, doc);
    if (properties === RESCAN) return scan(doc);
    const mapped: BlockState = {
      properties,
      code: blocks.code.map(tr.changes),
      unclosed: blocks.unclosed === null ? null : tr.changes.mapPos(blocks.unclosed, -1),
    };
    // Ограды разбираются после блока свойств: правки целиком внутри него (и в
    // старом тексте, и в новом) их не меняют. Правка, начатая в нём и
    // задевшая строки ниже, учитывается с первой строки после него.
    const fencesFrom = properties ? properties.to + 1 : 0;
    const fencesFromBefore = blocks.properties ? blocks.properties.to + 1 : 0;
    let from = -1;
    let to = -1;
    tr.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
      if (toB < fencesFrom && toA < fencesFromBefore) return;
      if (anyLine(before, fromA, toA, isFenceLike) || anyLine(doc, fromB, toB, isFenceLike)) {
        if (from < 0) from = Math.max(fromB, fencesFrom);
        to = Math.max(toB, fencesFrom);
      }
    });
    return from < 0 ? mapped : rescanFences(doc, mapped, Math.min(from, doc.length), Math.min(to, doc.length), fencesFrom);
  },
});

/** Включён ли живой предпросмотр (его расширение в состоянии). */
const livePreviewOn = Facet.define<boolean, boolean>({ combine: (values) => values.some(Boolean) });

/**
 * Пошаговый разбор блоков кода и блока свойств без оформления. Подключён в
 * обоих режимах редактора (codemirror.ts): в исходном тексте по нему
 * автодополнение и Ctrl+щелчок узнают, не в коде ли ссылка, — без разбора
 * оград от начала текста на каждое нажатие (задача 2.6а). Живой предпросмотр
 * подключает то же поле — CodeMirror не дублирует одно расширение.
 */
export function blockTracking(): Extension {
  return blocksField;
}

function anyLine(doc: Text, from: number, to: number, test: (text: string) => boolean): boolean {
  for (let n = doc.lineAt(from).number, last = doc.lineAt(to).number; n <= last; n++) {
    if (test(doc.line(n).text)) return true;
  }
  return false;
}

/** Блок свойств после правки; `RESCAN` — сдвинулась его граница, блоки кода надо искать заново. */
function updateProperties(old: Span | null, changes: ChangeSet, before: Text, doc: Text): Span | null | typeof RESCAN {
  const wasOpen = before.lines > 1 && PROPERTIES_OPEN.test(before.line(1).text);
  const isOpen = doc.lines > 1 && PROPERTIES_OPEN.test(doc.line(1).text);
  if (!wasOpen && !isOpen) return null;
  if (wasOpen !== isOpen) return RESCAN;
  let touched = false;
  changes.iterChangedRanges((fromA, _toA, fromB, toB) => {
    if (touched) return;
    // Нет закрывающей строки: блок появится, только если правка её добавила.
    touched = old ? fromA <= old.to : anyLine(doc, fromB, toB, (text) => PROPERTIES_CLOSE.test(text));
  });
  if (!touched) return old;
  if (!old) return RESCAN;
  const next = findProperties(doc);
  return next && next.to === changes.mapPos(old.to, 1) ? next : RESCAN;
}

/**
 * Начало закрытого блока, внутри которого (не на первой строке) лежит строка
 * с началом `pos`. Блоки с началом до `floor` (сдвинутые правкой в блок
 * свойств — их ограда удалена) не в счёт.
 */
function enclosingBlock(code: RangeSet<RangeValue>, pos: number, floor: number): number | null {
  let found: number | null = null;
  code.between(pos, pos, (from, to) => {
    if (from >= floor && from < pos && to >= pos) {
      found = from;
      return false;
    }
    return undefined;
  });
  return found;
}

/**
 * Ограды после правки строк-оград в `[changeFrom, changeTo]`. Разбор идёт с
 * начала первой задетой строки (или с открывающей ограды блока, в котором
 * она лежит) и останавливается на первой строке ниже правки, перед которой и
 * новый, и прежний разбор — вне блока: дальше текст и разбор те же, блоки
 * берутся из прежнего, сдвинутые. Если ограду открыли или закрыли, а дальше
 * блоки без названия языка, разбор может дойти до конца текста.
 */
function rescanFences(doc: Text, mapped: BlockState, changeFrom: number, changeTo: number, floor: number): BlockState {
  const first = doc.lineAt(changeFrom);
  const regionEnd = doc.lineAt(changeTo).to;
  // Незакрытый блок, сдвинутый правкой в блок свойств, — остаток удалённой ограды.
  const unclosed = mapped.unclosed !== null && mapped.unclosed >= floor ? mapped.unclosed : null;
  // Прежний разбор ниже правки: курсор идёт вперёд вместе с разбором,
  // без поиска по набору блоков на каждой проверке.
  const old = mapped.code.iter(regionEnd);
  const insideOld = (pos: number): boolean => {
    // Здесь — и блоки, чья ограда ушла правкой в блок свойств: это прежнее состояние.
    if (mapped.unclosed !== null && mapped.unclosed < pos) return true;
    while (old.value && old.to < pos) old.next();
    return old.value !== null && old.from < pos;
  };

  // Разбор с первой задетой строки; если она внутри блока — с состоянием
  // «блок открыт» его оградой (строка ограды выше правки не менялась).
  let start = first.from;
  let pos = first.from;
  let open: OpenFence | null = null;
  const enclosing = unclosed !== null && unclosed < start ? unclosed : enclosingBlock(mapped.code, start, floor);
  if (enclosing !== null) {
    start = enclosing;
    open = openFenceAt(doc.lineAt(enclosing).text, enclosing);
  }

  const closed: CodeBlock[] = [];
  let resync = -1;
  let check = true;
  const lines = doc.iterLines(first.number);
  for (lines.next(); !lines.done; lines.next()) {
    if (check && open === null && pos > regionEnd) {
      // Разборы могут совпасть только после строки, где новый закрыл блок.
      check = false;
      if (!insideOld(pos)) {
        resync = pos;
        break;
      }
    }
    const text = lines.value;
    const wasOpen = open !== null;
    open = fenceStep(open, text, pos, pos + text.length, closed);
    if (wasOpen && open === null) check = true;
    pos += text.length + 1;
  }

  const end = resync < 0 ? doc.length + 1 : resync;
  return {
    properties: mapped.properties,
    code: mapped.code.update({
      filter: (from) => from >= floor && (from < start || from >= end),
      filterFrom: Math.min(start, floor),
      filterTo: end,
      add: closed.map((block) => CODE_BLOCK.range(block.from, block.to)),
    }),
    unclosed: resync < 0 ? (open?.from ?? null) : unclosed !== null && unclosed >= end ? unclosed : null,
  };
}

/** Блоки кода и блок свойств из состояния редактора (для тестов: сверка с `scanBlocks`). */
export function codeBlocks(state: EditorState): Blocks {
  const blocks = state.field(blocksField, false) ?? NO_BLOCKS;
  const code: CodeBlock[] = [];
  blocks.code.between(0, state.doc.length, (from, to) => {
    code.push({ from, to, closeLine: state.doc.lineAt(to).from });
  });
  if (blocks.unclosed !== null) code.push({ from: blocks.unclosed, to: state.doc.length, closeLine: null });
  return { code, properties: blocks.properties };
}

/** Блок кода, в котором лежит строка с началом `pos`. */
function blockAt(state: EditorState, blocks: BlockState, pos: number): CodeBlock | null {
  if (blocks.unclosed !== null && pos >= blocks.unclosed) {
    return { from: blocks.unclosed, to: state.doc.length, closeLine: null };
  }
  let found: CodeBlock | null = null;
  blocks.code.between(pos, pos, (from, to) => {
    found = { from, to, closeLine: state.doc.lineAt(to).from };
    return false;
  });
  return found;
}

// ---- Значки ------------------------------------------------------------------

/** Разделитель имени и заголовка вики-ссылки: «Встреча › Итоги». */
const WIKI_HEADING_SEPARATOR = ' \u203a ';

class MarkWidget extends WidgetType {
  readonly kind: WidgetKind;
  /** Текст значка (для «Имя › Заголовок»); у остальных — пусто. */
  readonly text: string;

  constructor(kind: WidgetKind) {
    super();
    this.kind = kind;
    this.text = kind === 'wiki-heading' ? WIKI_HEADING_SEPARATOR : '';
  }

  override eq(other: MarkWidget): boolean {
    return other.kind === this.kind;
  }

  toDOM(view: EditorView): HTMLElement {
    const element = document.createElement('span');
    if (this.kind === 'wiki-heading') {
      element.className = 'cm-rz-wikilink-separator';
      element.textContent = this.text;
      return element;
    }
    if (this.kind === 'bullet') {
      element.className = 'cm-rz-bullet';
    } else if (this.kind === 'rule') {
      element.className = 'cm-rz-rule';
    } else {
      const checked = this.kind === 'checkbox-checked';
      element.className = checked ? 'cm-rz-checkbox cm-rz-checkbox-checked' : 'cm-rz-checkbox';
      element.setAttribute('role', 'checkbox');
      element.setAttribute('aria-checked', String(checked));
      element.setAttribute('aria-label', t('editor.task.toggle'));
      if (view.state.readOnly) element.setAttribute('aria-disabled', 'true');
    }
    element.setAttribute('aria-hidden', this.kind === 'bullet' || this.kind === 'rule' ? 'true' : 'false');
    return element;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

/**
 * Стрелка «↗» за текстом внешней ссылки (http, https, mailto; вид 5Б, Э1):
 * ссылка уводит из приложения. У вики-ссылок её нет. Щелчок по стрелке —
 * как по ссылке: у неё тот же адрес в `data-rz-url`.
 */
class ExternalMarkWidget extends WidgetType {
  readonly url: string;

  constructor(url: string) {
    super();
    this.url = url;
  }

  override eq(other: ExternalMarkWidget): boolean {
    return other.url === this.url;
  }

  toDOM(): HTMLElement {
    const element = document.createElement('span');
    element.className = 'cm-rz-link-external';
    element.textContent = EXTERNAL_ARROW;
    element.setAttribute('aria-hidden', 'true');
    element.setAttribute('data-rz-url', this.url);
    return element;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

/** Знак внешней ссылки (tokens.md: `accent/indigo/text`, «стрелка ↗»). */
export const EXTERNAL_ARROW = '\u2197';

const WIDGETS: Record<WidgetKind, MarkWidget> = {
  bullet: new MarkWidget('bullet'),
  checkbox: new MarkWidget('checkbox'),
  'checkbox-checked': new MarkWidget('checkbox-checked'),
  rule: new MarkWidget('rule'),
  'wiki-heading': new MarkWidget('wiki-heading'),
};

const HIDE = Decoration.replace({ rz: 'hide' });
const MARKUP = Decoration.mark({ class: 'cm-rz-markup' });
const widgetDecorations = Object.fromEntries(
  Object.entries(WIDGETS).map(([kind, widget]) => [kind, Decoration.replace({ widget, rz: 'widget' })]),
) as Record<WidgetKind, Decoration>;
const markDecorations = new Map<string, Decoration>();

function markDecoration(cls: string): Decoration {
  let decoration = markDecorations.get(cls);
  if (!decoration) {
    decoration = Decoration.mark({ class: cls });
    markDecorations.set(cls, decoration);
  }
  return decoration;
}

const lineDecorations = new Map<string, Decoration>();

function lineDecoration(cls: string): Decoration {
  let decoration = lineDecorations.get(cls);
  if (!decoration) {
    decoration = Decoration.line({ attributes: { class: cls } });
    lineDecorations.set(cls, decoration);
  }
  return decoration;
}

// ---- Промежутки между блоками и вложенность пунктов (задача 2.20) ----------

/** Вид строки: блок кода и блок свойств — по полю блоков, остальное — по началу строки. */
type BlockKind = LineKind | 'code' | 'properties';

interface LineInfo {
  kind: BlockKind;
  /** Блок кода, в котором строка. */
  block: CodeBlock | null;
  /** Отступ перед маркером пункта (символов) и уровень вложенности пункта: 0 — верхний. */
  indent: number;
  level: number;
}

/** Классы отступа вложенного пункта есть до этого уровня; глубже — как у него. */
const MAX_LIST_LEVEL = 6;

/** Строк выше видимой части, по которым ищутся родительские пункты. */
const LIST_LOOKBACK = 200;

/**
 * Промежуток между блоками — граница снизу у последней строки блока (tokens.md
 * «промежуток между блоками 10»), у строки под блоком кода — сверху: фон
 * блока не растягивается. Отдельное оформление (`rz: 'gap'`), текст не меняется.
 */
const GAP_AFTER = Decoration.line({ attributes: { class: 'cm-rz-gap' }, rz: 'gap' });
const GAP_BEFORE = Decoration.line({ attributes: { class: 'cm-rz-gap-before' }, rz: 'gap' });

/** Вид строки `n` без учёта пунктов выше. */
function classify(state: EditorState, blocks: BlockState, n: number): LineInfo & { columns: number } {
  const { doc } = state;
  const line = doc.line(n);
  const { properties } = blocks;
  if (properties && line.from >= properties.from && line.to <= properties.to) {
    return { kind: 'properties', block: null, indent: 0, level: 0, columns: 0 };
  }
  const block = blockAt(state, blocks, line.from);
  if (block) return { kind: 'code', block, indent: 0, level: 0, columns: 0 };
  const previous = n > 1 && !blockAt(state, blocks, doc.line(n - 1).from) ? doc.line(n - 1).text : null;
  const shape = lineShape(line.text, previous);
  return { kind: shape.kind, block: null, indent: shape.indent, level: 0, columns: shape.columns };
}

/**
 * Вложенность пунктов сверху вниз: стек отступов открытых пунктов. Пункт с
 * отступом больше, чем у пункта выше, — вложен в него. Пустая строка и
 * строка с отступом (продолжение пункта) список не обрывают; любой другой
 * блок — обрывает.
 */
function nest(stack: number[], info: LineInfo & { columns: number }): LineInfo {
  if (info.kind === 'blank') return info;
  if (info.kind === 'list') {
    while (stack.length > 0 && (stack[stack.length - 1] ?? 0) >= info.columns) stack.pop();
    const level = stack.length;
    stack.push(info.columns);
    return { ...info, level };
  }
  if (info.kind === 'para' && info.indent > 0 && stack.length > 0) return { ...info, kind: 'list' };
  stack.length = 0;
  return info;
}

/** Стек пунктов перед строкой `first`: подъём до начала списка (не дальше `LIST_LOOKBACK` строк). */
function listStackBefore(state: EditorState, blocks: BlockState, first: number): number[] {
  let start = first;
  while (start > 1 && first - start < LIST_LOOKBACK) {
    const info = classify(state, blocks, start - 1);
    if (info.kind !== 'blank' && info.kind !== 'list' && !(info.kind === 'para' && info.indent > 0)) break;
    start--;
  }
  const stack: number[] = [];
  for (let n = start; n < first; n++) nest(stack, classify(state, blocks, n));
  return stack;
}

/** Нужен ли промежуток между строкой `a` и строкой `b` под ней (вне блоков кода). */
function gapBetween(a: LineInfo, b: LineInfo): boolean {
  if (a.kind === 'blank' || a.kind === 'properties' || a.kind === 'code') return false;
  if (b.kind === 'blank' || b.kind === 'code' || b.kind === 'properties') return true;
  if (a.kind === 'heading' || a.kind === 'rule' || b.kind === 'heading' || b.kind === 'rule') return true;
  return a.kind !== b.kind;
}

// ---- Оформление --------------------------------------------------------------

/** Задевает ли курсор или выделение участок `[from, to]`, включая края (решение 93). */
function touches(state: EditorState, from: number, to: number): boolean {
  return state.selection.ranges.some((range) => range.from <= to && range.to >= from);
}

/**
 * Оформление живого предпросмотра для участков текста `ranges` (по умолчанию —
 * весь текст). Без живого предпросмотра (режим исходного текста) — пусто.
 */
export function previewDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[] = [{ from: 0, to: state.doc.length }],
): DecorationSet {
  const blocks = state.field(blocksField, false);
  // Поле блоков есть и в исходном тексте (`blockTracking`): оформление — только в живом предпросмотре.
  if (!blocks || !state.facet(livePreviewOn)) return Decoration.none;
  const out: Range<Decoration>[] = [];
  const { doc } = state;
  const resolution = state.field(resolutionField, false);

  for (const range of ranges) {
    let n = doc.lineAt(range.from).number;
    const last = doc.lineAt(range.to).number;
    const stack = listStackBefore(state, blocks, n);
    let info = nest(stack, classify(state, blocks, n));
    for (; n <= last; n++) {
      const line = doc.line(n);
      const next = n < doc.lines ? nest(stack, classify(state, blocks, n + 1)) : null;
      if (next) {
        if (info.block) {
          if (info.block.closeLine === line.from) out.push(GAP_BEFORE.range(line.to + 1));
        } else if (gapBetween(info, next)) {
          out.push(GAP_AFTER.range(line.from));
        }
      }
      const current = info;
      info = next ?? info;
      if (current.kind === 'properties') continue;

      if (current.block) {
        codeLine(current.block, line, touches(state, current.block.from, current.block.to), out);
        continue;
      }

      // Вложенный пункт: отступ по уровню (26 px на уровень), а не по ширине пробелов.
      if (current.kind === 'list' && current.level > 0 && current.indent > 0) {
        const level = Math.min(current.level, MAX_LIST_LEVEL);
        out.push(markDecoration(`cm-rz-list-indent cm-rz-list-indent-${level}`).range(line.from, line.from + current.indent));
      }

      const previous = n > 1 && !blockAt(state, blocks, doc.line(n - 1).from) ? doc.line(n - 1).text : null;
      const markup = parseLine(line.text, previous);
      // Строка без курсора — ничего не раскрыто: без проверки каждого элемента.
      const lineActive = touches(state, line.from, line.to);
      const revealed = (owner: Span) => lineActive && touches(state, line.from + owner.from, line.from + owner.to);
      if (markup.lineClass) out.push(lineDecoration(markup.lineClass).range(line.from));
      for (const piece of markup.pieces) {
        const from = line.from + piece.from;
        const to = line.from + piece.to;
        if (from >= to) continue;
        if (piece.kind === 'mark') {
          out.push(pieceMark(piece, resolution).range(from, to));
          // У нераскрытой ссылки: в раскрытой адрес и так виден.
          if (piece.url !== undefined && isExternalUrl(piece.url) && !(piece.link && revealed(piece.link))) {
            out.push(Decoration.widget({ widget: new ExternalMarkWidget(piece.url), side: -1 }).range(to));
          }
        } else if (revealed(piece.owner)) {
          out.push(MARKUP.range(from, to));
        } else {
          out.push((piece.kind === 'hide' ? HIDE : widgetDecorations[piece.widget]).range(from, to));
        }
      }
    }
  }
  return Decoration.set(out, true);
}

/** Оформление участка: ссылка несёт адрес или цель для щелчка, неразрешённая вики-ссылка — свой класс. */
function pieceMark(piece: MarkPiece, resolution: Resolution | undefined): Decoration {
  if (piece.wiki !== undefined) {
    const unresolved = isUnresolved(resolution, piece.key ?? '');
    return Decoration.mark({
      class: unresolved ? `${piece.cls} cm-rz-wikilink-unresolved` : piece.cls,
      attributes: { 'data-rz-wiki': piece.wiki },
    });
  }
  if (piece.url !== undefined) return Decoration.mark({ class: piece.cls, attributes: { 'data-rz-url': piece.url } });
  return markDecoration(piece.cls);
}

function codeLine(
  block: CodeBlock,
  line: { from: number; to: number; text: string },
  revealed: boolean,
  out: Range<Decoration>[],
): void {
  const first = line.from === block.from;
  const last = block.closeLine === line.from;
  // Скрытая ограда не занимает строку: поле блока — только отступ 12 px (tokens.md «Блок кода», Э1).
  const fence = (first || last) && !revealed;
  const cls = [
    'cm-rz-codeblock',
    first ? 'cm-rz-codeblock-first' : '',
    last ? 'cm-rz-codeblock-last' : '',
    fence ? 'cm-rz-codeblock-fence' : '',
  ]
    .filter(Boolean)
    .join(' ');
  out.push(lineDecoration(cls).range(line.from));
  if (!first && !last) return;
  const from = line.from + (line.text.length - line.text.trimStart().length);
  if (from >= line.to) return;
  out.push((revealed ? MARKUP : HIDE).range(from, line.to));
}

// ---- Чекбоксы и ссылки -------------------------------------------------------

/**
 * Правка, которая переключает чекбокс `[ ]` ↔ `[x]` в позиции `pos` (начало
 * `[`): меняется ровно один символ. `null` — там не чекбокс или правка
 * запрещена.
 */
export function taskToggle(state: EditorState, pos: number): TransactionSpec | null {
  if (state.readOnly) return null;
  const box = state.sliceDoc(pos, pos + 3);
  if (!/^\[[ xX]\]$/.test(box)) return null;
  return {
    changes: { from: pos + 1, to: pos + 2, insert: box[1] === ' ' ? 'x' : ' ' },
    userEvent: 'input.toggle',
  };
}

/** Адреса, которые окно открывает (остальные схемы ядро отклонит, С3.4). */
export function isExternalUrl(url: string): boolean {
  return /^(?:https?:|mailto:)/i.test(url);
}

export interface LivePreviewOptions {
  /** Щелчок по внешней ссылке: открыть через ядро (подтверждение). */
  onOpenLink?: ((url: string) => void) | undefined;
}

/** Ссылка в позиции `pos` раскрыта: курсор или выделение задевает её целиком со скобками (решение 93). */
function linkRevealedAt(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  if (!touches(state, line.from, line.to)) return false;
  const previous = line.number > 1 ? state.doc.line(line.number - 1).text : null;
  const offset = pos - line.from;
  return parseLine(line.text, previous).pieces.some(
    (piece) =>
      piece.kind === 'mark' &&
      piece.link !== undefined &&
      offset >= piece.link.from &&
      offset < piece.link.to &&
      touches(state, line.from + piece.link.from, line.from + piece.link.to),
  );
}

/** Ссылка под щелчком: внешний адрес или текст вики-ссылки между `[[` и `]]`. */
export type LinkHit = { url: string } | { wiki: string };

/**
 * Строка с номером `n` внутри блока кода или блока свойств — по полю блоков
 * (в редакторе оно есть в обоих режимах, `blockTracking`). Без поля
 * (состояние без расширений редактора) — разбором оград от начала текста.
 */
function inBlock(state: EditorState, n: number): boolean {
  const line = state.doc.line(n);
  const blocks = state.field(blocksField, false);
  if (blocks) {
    const { properties } = blocks;
    if (properties && line.from >= properties.from && line.to <= properties.to) return true;
    return blockAt(state, blocks, line.from) !== null;
  }
  const doc: LineSource = state.doc;
  if (doc.length > MAX_BLOCK_TEXT) return false;
  const properties = findProperties(doc);
  if (properties && line.from >= properties.from && line.to <= properties.to) return true;
  let open: OpenFence | null = null;
  const closed: CodeBlock[] = [];
  const first = properties ? state.doc.lineAt(properties.to).number + 1 : 1;
  for (let k = first; k < n; k++) {
    const above = state.doc.line(k);
    open = fenceStep(open, above.text, above.from, above.to, closed);
  }
  if (open) return true;
  // Сама строка — ограда, открывающая блок.
  return isFenceLike(line.text) && fenceStep(null, line.text, line.from, line.to, closed) !== null;
}

/** Ссылка (Markdown или вики) в позиции `pos` текста; `null` — там не ссылка. */
export function linkAt(state: EditorState, pos: number): LinkHit | null {
  const line = state.doc.lineAt(pos);
  if (inBlock(state, line.number)) return null;
  const previous = line.number > 1 ? state.doc.line(line.number - 1).text : null;
  const offset = pos - line.from;
  for (const piece of parseLine(line.text, previous).pieces) {
    if (piece.kind !== 'mark' || !piece.link || offset < piece.link.from || offset >= piece.link.to) continue;
    if (piece.wiki !== undefined) return { wiki: piece.wiki };
    if (piece.url !== undefined) return { url: piece.url };
  }
  return null;
}

/**
 * Позиция `pos` — в коде (в строке `` `…` `` или в блоке с оградой) или в
 * блоке свойств: разметка там не действует (по разбору предпросмотра).
 */
export function inCodeAt(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  if (inBlock(state, line.number)) return true;
  const previous = line.number > 1 ? state.doc.line(line.number - 1).text : null;
  const offset = pos - line.from;
  return parseLine(line.text, previous).pieces.some(
    (piece) => piece.kind === 'mark' && piece.cls === 'cm-rz-code' && offset >= piece.from && offset < piece.to,
  );
}

/** Открыть ссылку: внешнюю — через ядро с подтверждением, вики — переходом к заметке хранилища. */
function follow(view: EditorView, hit: LinkHit, onOpenLink: ((url: string) => void) | undefined): boolean {
  if ('wiki' in hit) {
    const host = view.state.facet(wikiHost);
    if (!host) return false;
    host.open(hit.wiki);
    return true;
  }
  if (!onOpenLink || !isExternalUrl(hit.url)) return false;
  onOpenLink(hit.url);
  return true;
}

const previewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = previewDecorations(view.state, view.visibleRanges);
    }

    update(update: ViewUpdate) {
      if (
        update.docChanged ||
        update.viewportChanged ||
        update.selectionSet ||
        update.startState.readOnly !== update.state.readOnly ||
        update.startState.field(resolutionField, false) !== update.state.field(resolutionField, false)
      ) {
        this.decorations = previewDecorations(update.state, update.view.visibleRanges);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/** Ключи вики-ссылок в участках текста (вне блоков кода и блока свойств). */
export function wikiKeys(state: EditorState, ranges: readonly { from: number; to: number }[]): Set<string> {
  const keys = new Set<string>();
  const blocks = state.field(blocksField, false) ?? NO_BLOCKS;
  const { doc } = state;
  for (const range of ranges) {
    for (let n = doc.lineAt(range.from).number, last = doc.lineAt(range.to).number; n <= last; n++) {
      const line = doc.line(n);
      if (!line.text.includes('[[')) continue;
      const { properties } = blocks;
      if (properties && line.from >= properties.from && line.to <= properties.to) continue;
      if (blockAt(state, blocks, line.from)) continue;
      for (const piece of parseLine(line.text, null).pieces) {
        if (piece.kind === 'mark' && piece.key !== undefined) keys.add(piece.key);
      }
    }
  }
  return keys;
}

/** Пауза после ввода или прокрутки перед вопросом ядру: не на каждое нажатие. */
export const RESOLVE_DELAY_MS = 150;

/**
 * Разрешение ссылок видимой части: ядро спрашивается о ключах, ответа на
 * которые нет, после паузы. После изменения дерева хранилища (`refresh`) —
 * обо всех видимых заново; до ответа ссылки выглядят как прежде.
 */
const resolverPlugin = ViewPlugin.fromClass(
  class {
    #view: EditorView;
    #timer: ReturnType<typeof setTimeout> | null = null;
    #asked = new Set<string>();
    #generation = 0;
    #unsubscribe: (() => void) | null = null;
    #destroyed = false;

    constructor(view: EditorView) {
      this.#view = view;
      const host = view.state.facet(wikiHost);
      if (!host) return;
      this.#unsubscribe = host.subscribe((event) => {
        if (event.kind === 'refresh' && !this.#destroyed) this.#view.dispatch({ effects: staleEffect.of(null) });
      });
      this.#schedule();
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) this.#schedule();
      else if (update.state.field(resolutionField).generation !== this.#generation) this.#schedule();
    }

    #schedule() {
      if (this.#timer !== null) clearTimeout(this.#timer);
      this.#timer = setTimeout(() => this.#ask(), RESOLVE_DELAY_MS);
    }

    #ask() {
      this.#timer = null;
      const host = this.#view.state.facet(wikiHost);
      if (!host || this.#destroyed) return;
      const resolution = this.#view.state.field(resolutionField);
      if (resolution.generation !== this.#generation) {
        this.#generation = resolution.generation;
        this.#asked.clear();
      }
      const wanted = [...wikiKeys(this.#view.state, this.#view.visibleRanges)].filter((key) => !this.#asked.has(key));
      if (wanted.length === 0) return;
      for (const key of wanted) this.#asked.add(key);
      const generation = this.#generation;
      host.resolve(wanted).then(
        (answers) => {
          // Ответ о прежнем дереве (до vault:changed) не затирает свежий: его спросили заново.
          if (this.#destroyed || generation !== this.#view.state.field(resolutionField).generation) return;
          this.#view.dispatch({ effects: resolvedEffect.of(answers) });
        },
        () => {
          // Нет ответа — ссылки выглядят разрешёнными; спросить снова при следующей правке.
          if (generation === this.#generation) for (const key of wanted) this.#asked.delete(key);
        },
      );
    }

    destroy() {
      this.#destroyed = true;
      if (this.#timer !== null) clearTimeout(this.#timer);
      this.#unsubscribe?.();
    }
  },
);

/** Расширение живого предпросмотра. */
export function livePreview({ onOpenLink }: LivePreviewOptions = {}): Extension {
  return [
    livePreviewOn.of(true),
    blocksField,
    resolutionField,
    previewPlugin,
    resolverPlugin,
    tagPreview(inBlock),
    EditorView.domEventHandlers({
      mousedown(event, view) {
        if (event.button !== 0 || !(event.target instanceof Element)) return false;
        const checkbox = event.target.closest('.cm-rz-checkbox');
        if (checkbox) {
          event.preventDefault();
          const spec = taskToggle(view.state, view.posAtDOM(checkbox));
          if (spec) view.dispatch(spec);
          return true;
        }
        const wikiText = event.target.closest('[data-rz-wiki]');
        const linkText = wikiText ?? event.target.closest('[data-rz-url]');
        if (!linkText) return false;
        const hit: LinkHit = wikiText
          ? { wiki: wikiText.getAttribute('data-rz-wiki') ?? '' }
          : { url: linkText.getAttribute('data-rz-url') ?? '' };
        // Раскрытая ссылка (курсор в ней или у края) — текст для правки; открыть — Ctrl+щелчок (решения 76, 93).
        if (!event.ctrlKey && !event.metaKey && linkRevealedAt(view.state, view.posAtDOM(linkText))) return false;
        if (!follow(view, hit, onOpenLink)) return false;
        event.preventDefault();
        return true;
      },
    }),
  ];
}

/**
 * Режим исходного текста: ссылки (Markdown и вики) открываются только
 * Ctrl+щелчком (С3.4, решение 76); обычный щелчок ставит курсор.
 */
export function sourceLinks({ onOpenLink }: LivePreviewOptions = {}): Extension {
  return [sourceTags(inBlock), EditorView.domEventHandlers({
    mousedown(event, view) {
      if (event.button !== 0 || (!event.ctrlKey && !event.metaKey)) return false;
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;
      const hit = linkAt(view.state, pos);
      if (!hit || !follow(view, hit, onOpenLink)) return false;
      event.preventDefault();
      return true;
    },
  })];
}
