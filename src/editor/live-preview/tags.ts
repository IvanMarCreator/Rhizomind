// Теги в редакторе (0.2-3.1, С2-3.3 «В редакторе»): в живом предпросмотре тег
// оформлен плашкой (цвета — переменные темы `--tag-*`), `#` остаётся видимым;
// щелчок открывает вид «Теги» с раскрытым тегом. В режиме исходного текста
// тег — обычный текст, Ctrl+щелчок — то же, что щелчок в предпросмотре.
//
// Где теги, решает общий разбор src/core/tags (те же правила и образцы, что у
// ядра). Разбор всего текста на каждое нажатие был бы дорог (заметки до
// 10 МБ, решение 103), поэтому разбирается только видимая часть: от ближайшей
// строки выше, с которой состояние разбора известно без строк над ней
// (начало текста или строка после пустой без отступа — там сбрасывается
// «идёт список»), и до конца абзаца ниже. Блоки с оградой и блок свойств —
// из разбора блоков редактора (`inBlock` из index.ts), как их и показывает
// предпросмотр. Подъём вверх и спуск вниз ограничены `LOOKBACK`/`LOOKAHEAD`
// строками: абзац или список длиннее — разбирается с этого места заново
// (в худшем случае код в строке, начатый выше, не узнаётся).
//
// Щелчок как у ссылок (решения 76, 93): курсор в теге или у его края —
// обычный щелчок ставит курсор (тег можно править), Ctrl+щелчок открывает.
import { Facet, Prec, type EditorState, type Extension, type Range } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { LineClassifier, indentColumns, tagsInParagraph, type LineSpan, type TextTag } from '../../core/tags/index.ts';

/** Строка с номером `n` — в блоке с оградой или в блоке свойств (по разбору блоков редактора). */
export type BlockLookup = (state: EditorState, n: number) => boolean;

/** Строк выше видимой части, среди которых ищется начало разбора. */
export const LOOKBACK = 200;
/** Строк ниже видимой части, до которых дочитывается начатый абзац. */
export const LOOKAHEAD = 200;

const blockLookup = Facet.define<BlockLookup, BlockLookup | null>({ combine: (values) => values[0] ?? null });

/** Открыть вид «Теги» с раскрытым тегом (имя без `#`, как в тексте). Задаёт окно (Editor.svelte). */
export const tagOpener = Facet.define<(tag: string) => void, ((tag: string) => void) | null>({
  combine: (values) => values[0] ?? null,
});

const noBlocks: BlockLookup = () => false;

/** Строка разбора: участок без перевода строки; `block` — строка блока кода или свойств. */
interface ScanLine extends LineSpan {
  block: boolean;
  docLine: number;
}

/**
 * Строки текста с `first`, как их делит ядро: переводы `\r\n`, `\r`, `\n`.
 * В редакторе строки делит только `\n` (`lineSeparator`), одиночный `\r`
 * остаётся в тексте строки.
 */
function* scanLines(state: EditorState, inBlock: BlockLookup, first: number): Generator<ScanLine> {
  const { doc } = state;
  for (let n = first; n <= doc.lines; n++) {
    const line = doc.line(n);
    const block = inBlock(state, n);
    const { text } = line;
    let start = 0;
    for (let cr = text.indexOf('\r'); cr >= 0; cr = text.indexOf('\r', start)) {
      yield { from: line.from + start, to: line.from + cr, block, docLine: n };
      start = cr + 1;
    }
    // `\r` в конце строки перед `\n` — один перевод строки `\r\n`.
    if (start < text.length || start === 0 || n === doc.lines) {
      yield { from: line.from + start, to: line.to, block, docLine: n };
    }
  }
}

const BLANK = /^[ \t]*\r?$/;

/**
 * Начало разбора для строки `first`: ближайшая строка не выше `LOOKBACK`, с
 * которой состояние известно — первая строка текста или непустая строка без
 * отступа (меньше 2 колонок) вне блоков после пустой строки.
 */
function syncLine(state: EditorState, inBlock: BlockLookup, first: number): number {
  const { doc } = state;
  const floor = Math.max(1, first - LOOKBACK);
  for (let n = first; n > floor; n--) {
    const text = doc.line(n).text;
    if (BLANK.test(text) || indentColumns(text) >= 2 || !BLANK.test(doc.line(n - 1).text)) continue;
    if (!inBlock(state, n) && !inBlock(state, n - 1)) return n;
  }
  return floor;
}

/** Теги в участках текста `ranges` (по порядку, без повторов); положения — в тексте редактора. */
export function editorTags(state: EditorState, ranges: readonly { from: number; to: number }[]): TextTag[] {
  const inBlock = state.facet(blockLookup) ?? noBlocks;
  const { doc } = state;
  const out: TextTag[] = [];
  let seen = -1;
  for (const range of ranges) {
    const last = doc.lineAt(range.to).number;
    const take = (paragraph: readonly ScanLine[]) => {
      for (const tag of paragraphTags(state, paragraph, range)) {
        if (tag.from < seen) continue;
        out.push(tag);
        seen = tag.to;
      }
    };
    const lines = new LineClassifier();
    let paragraph: ScanLine[] = [];
    for (const line of scanLines(state, inBlock, syncLine(state, inBlock, doc.lineAt(range.from).number))) {
      if ((line.docLine > last && paragraph.length === 0) || line.docLine > last + LOOKAHEAD) break;
      if (line.block) lines.fencedCode();
      if (!line.block && lines.classify(doc.sliceString(line.from, line.to)) === 'text') {
        paragraph.push(line);
      } else {
        take(paragraph);
        paragraph = [];
      }
    }
    take(paragraph);
  }
  return out;
}

/** Теги абзаца, задевающие участок `range`. */
function paragraphTags(state: EditorState, paragraph: readonly ScanLine[], range: { from: number; to: number }): TextTag[] {
  const head = paragraph[0];
  const tail = paragraph.at(-1);
  if (!head || !tail || tail.to < range.from || head.from > range.to) return [];
  const offset = head.from;
  const text = state.doc.sliceString(offset, tail.to);
  const lines = paragraph.map((line) => ({ from: line.from - offset, to: line.to - offset }));
  return tagsInParagraph(text, lines)
    .map((tag) => ({ name: tag.name, from: tag.from + offset, to: tag.to + offset }))
    .filter((tag) => tag.to >= range.from && tag.from <= range.to);
}

/** Тег, в котором (или у начала которого) стоит позиция `pos`; `null` — там не тег. */
export function tagAt(state: EditorState, pos: number): TextTag | null {
  const line = state.doc.lineAt(pos);
  return editorTags(state, [line]).find((tag) => pos >= tag.from && pos < tag.to) ?? null;
}

/** Задевает ли курсор или выделение тег, включая края (решение 93). */
function touches(state: EditorState, tag: TextTag): boolean {
  return state.selection.ranges.some((range) => range.from <= tag.to && range.to >= tag.from);
}

/** Открыть вид «Теги» с раскрытым тегом; `false` — окно не задало, как открыть. */
function openTag(view: EditorView, tag: TextTag): boolean {
  const opener = view.state.facet(tagOpener);
  if (!opener) return false;
  opener(tag.name);
  return true;
}

const plaques = new Map<string, Decoration>();

/** Плашка тега: класс и имя тега (для щелчка и проверок); одна на имя. */
function plaque(name: string): Decoration {
  let decoration = plaques.get(name);
  if (!decoration) {
    decoration = Decoration.mark({ class: 'cm-rz-tag', attributes: { 'data-rz-tag': name } });
    if (plaques.size > 1000) plaques.clear();
    plaques.set(name, decoration);
  }
  return decoration;
}

/** Плашки тегов в участках текста `ranges` (для тестов и расширения). */
export function tagDecorations(state: EditorState, ranges: readonly { from: number; to: number }[]): DecorationSet {
  const out: Range<Decoration>[] = editorTags(state, ranges).map((tag) => plaque(tag.name).range(tag.from, tag.to));
  return Decoration.set(out);
}

const plaquePlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = tagDecorations(view.state, view.visibleRanges);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) this.decorations = tagDecorations(update.state, update.view.visibleRanges);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/**
 * Теги в живом предпросмотре: плашки и щелчок. Обработчик щелчка — раньше
 * ссылок (тег в тексте ссылки `[про #тег](адрес)` открывает тег).
 */
export function tagPreview(inBlock: BlockLookup): Extension {
  return [
    blockLookup.of(inBlock),
    plaquePlugin,
    Prec.high(
      EditorView.domEventHandlers({
        mousedown(event, view) {
          if (event.button !== 0 || !(event.target instanceof Element)) return false;
          const element = event.target.closest('[data-rz-tag]');
          if (!element) return false;
          const tag = tagAt(view.state, view.posAtDOM(element));
          if (!tag) return false;
          // Курсор в теге или у края — щелчок для правки; открыть — Ctrl+щелчок (как у ссылок, решения 76, 93).
          if (!event.ctrlKey && !event.metaKey && touches(view.state, tag)) return false;
          if (!openTag(view, tag)) return false;
          event.preventDefault();
          return true;
        },
      }),
    ),
  ];
}

/** Режим исходного текста: тег открывается Ctrl+щелчком, обычный щелчок ставит курсор. */
export function sourceTags(inBlock: BlockLookup): Extension {
  return [
    blockLookup.of(inBlock),
    Prec.high(
      EditorView.domEventHandlers({
        mousedown(event, view) {
          if (event.button !== 0 || (!event.ctrlKey && !event.metaKey)) return false;
          const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
          const tag = pos === null ? null : tagAt(view.state, pos);
          if (!tag || !openTag(view, tag)) return false;
          event.preventDefault();
          return true;
        },
      }),
    ),
  ];
}
