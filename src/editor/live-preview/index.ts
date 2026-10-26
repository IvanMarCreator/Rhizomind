// Живой предпросмотр (С3.3, ADR 0001): оформление поверх текста. Документ
// редактора не меняется — разметка только прячется (вне строки с курсором)
// или приглушается (на ней). Режим исходного текста — без этого расширения:
// вся разметка видна как есть.
//
// Ссылки `[текст](адрес)` — без элементов `<a>` и переходов в окне: щелчок
// передаёт адрес наверх, окно открывает его командой ядра `open_external`
// с подтверждением (С3.4, docs/SECURITY.md).
import { RangeSet, RangeValue, StateField, type EditorState, type Text, type Extension, type Range, type TransactionSpec } from '@codemirror/state';
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import { t } from '../../i18n/index.ts';
import { BLOCK_EDGE, parseLine, scanBlocks, type CodeBlock, type WidgetKind } from './markdown.ts';

// ---- Блоки кода и блок свойств ----------------------------------------------

interface BlockState {
  /** Закрытые блоки кода: набор диапазонов CodeMirror сдвигается за правкой без перебора всех блоков. */
  code: RangeSet<RangeValue>;
  /** Начало незакрытого блока: он идёт до конца текста. */
  unclosed: number | null;
  properties: { from: number; to: number } | null;
}

const CODE_BLOCK = new (class extends RangeValue {})();

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

/**
 * Блоки, зависящие от строк выше. Пересчитываются целиком, только если правка
 * задела строку, похожую на ограду или границу блока свойств; иначе позиции
 * сдвигаются вслед за правкой.
 */
const blocksField = StateField.define<BlockState>({
  create: (state) => scan(state.doc),
  update(blocks, tr) {
    if (!tr.docChanged) return blocks;
    let rescan = false;
    tr.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
      if (rescan) return;
      rescan =
        touchesEdge(tr.startState.doc, fromA, toA) ||
        touchesEdge(tr.state.doc, fromB, toB) ||
        (blocks.properties !== null && fromA <= blocks.properties.to + 1) ||
        fromB <= tr.state.doc.line(1).to;
    });
    if (rescan) return scan(tr.state.doc);
    return {
      properties: blocks.properties,
      code: blocks.code.map(tr.changes),
      unclosed: blocks.unclosed === null ? null : tr.changes.mapPos(blocks.unclosed, -1),
    };
  },
});

function touchesEdge(doc: Text, from: number, to: number): boolean {
  for (let n = doc.lineAt(from).number, last = doc.lineAt(to).number; n <= last; n++) {
    if (BLOCK_EDGE.test(doc.line(n).text)) return true;
  }
  return false;
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

class MarkWidget extends WidgetType {
  readonly kind: WidgetKind;

  constructor(kind: WidgetKind) {
    super();
    this.kind = kind;
  }

  override eq(other: MarkWidget): boolean {
    return other.kind === this.kind;
  }

  toDOM(view: EditorView): HTMLElement {
    const element = document.createElement('span');
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

const WIDGETS: Record<WidgetKind, MarkWidget> = {
  bullet: new MarkWidget('bullet'),
  checkbox: new MarkWidget('checkbox'),
  'checkbox-checked': new MarkWidget('checkbox-checked'),
  rule: new MarkWidget('rule'),
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

// ---- Оформление --------------------------------------------------------------

/** Строки, на которых стоит курсор или выделение: там разметка видна. */
function activeLines(state: EditorState): { from: number; to: number }[] {
  return state.selection.ranges.map((range) => ({
    from: state.doc.lineAt(range.from).from,
    to: state.doc.lineAt(range.to).to,
  }));
}

function touches(active: { from: number; to: number }[], from: number, to: number): boolean {
  return active.some((range) => range.from <= to && range.to >= from);
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
  if (!blocks) return Decoration.none;
  const active = activeLines(state);
  const out: Range<Decoration>[] = [];
  const { doc } = state;

  for (const range of ranges) {
    let n = doc.lineAt(range.from).number;
    const last = doc.lineAt(range.to).number;
    for (; n <= last; n++) {
      const line = doc.line(n);
      const properties = blocks.properties;
      if (properties && line.from >= properties.from && line.to <= properties.to) continue;

      const block = blockAt(state, blocks, line.from);
      if (block) {
        codeLine(block, line, touches(active, block.from, block.to), out);
        continue;
      }

      const previous = n > 1 && !blockAt(state, blocks, doc.line(n - 1).from) ? doc.line(n - 1).text : null;
      const markup = parseLine(line.text, previous);
      const revealed = touches(active, line.from, line.to);
      if (markup.lineClass) out.push(lineDecoration(markup.lineClass).range(line.from));
      for (const piece of markup.pieces) {
        const from = line.from + piece.from;
        const to = line.from + piece.to;
        if (from >= to) continue;
        if (piece.kind === 'mark') {
          const decoration = piece.url === undefined
            ? markDecoration(piece.cls)
            : Decoration.mark({ class: piece.cls, attributes: { 'data-rz-url': piece.url } });
          out.push(decoration.range(from, to));
        } else if (revealed) {
          out.push(MARKUP.range(from, to));
        } else {
          out.push((piece.kind === 'hide' ? HIDE : widgetDecorations[piece.widget]).range(from, to));
        }
      }
    }
  }
  return Decoration.set(out, true);
}

function codeLine(
  block: CodeBlock,
  line: { from: number; to: number; text: string },
  revealed: boolean,
  out: Range<Decoration>[],
): void {
  const first = line.from === block.from;
  const last = block.closeLine === line.from;
  const cls = ['cm-rz-codeblock', first ? 'cm-rz-codeblock-first' : '', last ? 'cm-rz-codeblock-last' : '']
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

function revealedAt(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  return touches(activeLines(state), line.from, line.to);
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
        update.startState.readOnly !== update.state.readOnly
      ) {
        this.decorations = previewDecorations(update.state, update.view.visibleRanges);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/** Расширение живого предпросмотра. */
export function livePreview({ onOpenLink }: LivePreviewOptions = {}): Extension {
  return [
    blocksField,
    previewPlugin,
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
        const linkText = event.target.closest('[data-rz-url]');
        if (!linkText || !onOpenLink) return false;
        const url = linkText.getAttribute('data-rz-url') ?? '';
        if (!isExternalUrl(url)) return false;
        // На строке с курсором ссылка — текст для правки; открыть — Ctrl+щелчок.
        if (!event.ctrlKey && !event.metaKey && revealedAt(view.state, view.posAtDOM(linkText))) return false;
        event.preventDefault();
        onOpenLink(url);
        return true;
      },
    }),
  ];
}
