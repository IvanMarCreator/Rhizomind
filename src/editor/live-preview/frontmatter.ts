// Блок свойств в живом предпросмотре (0.2-3.2, С2-3.4, решение 112, макет
// П-4): на месте блока — свёрнутая строка «Свойства» (шеврон, значок, серая
// подпись, справа — теги из блока плашками). Щелчок по строке или курсор в
// блоке раскрывают исходный текст для правки (строки на фоне `bg/code`),
// уход курсора сворачивает. В режиме исходного текста этого расширения нет —
// блок виден целиком. Сворачивание — только оформление: текст заметки не
// меняется (инвариант 4).
//
// Где блок, решает разбор блоков редактора (`blocksField` в index.ts):
// незакрытый блок — не блок, в заметке больше 10 МБ блок не распознаётся
// (решение 77). Теги блока — свой разбор ровно форм С2-3.3 (Р14, Р17 п.7,
// Р18), как у ядра (src-tauri/src/markdown/frontmatter.rs); обе стороны
// проверяются общими образцами tests/fixtures/frontmatter/.
//
// Когда блок раскрыт: курсор (голова любого выделения) стоит в блоке после
// его первого символа — `(from, to]`. Начало текста (`from`) — край строки
// «Свойства»: курсор там виден перед ней, блок свёрнут (новая заметка
// открывается с курсором в начале — блок не раскрывается сам). Так курсор
// никогда не оказывается внутри скрытого текста. Стрелки ↑/↓ обходят
// свёрнутый блок как одну строку: ↑ из строки под блоком ставит курсор в
// конец блока (он раскрывается), ↓ из начала текста — на строку под блоком.
//
// Составление (IME): ввод в раскрытом блоке оставляет курсор в блоке — блок
// не сворачивается; оформление раскрытого блока — только классы строк (одни
// и те же объекты оформления), узлы текста не разрезаются.
import { EditorSelection, Prec, StateField, type EditorState, type Extension, type Range, type Transaction } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, keymap, type Command, type DecorationSet } from '@codemirror/view';
import { isTagName, tagKey } from '../../core/tags/index.ts';
import { t } from '../../i18n/index.ts';
import { PROPERTIES_CLOSE, PROPERTIES_OPEN, type Span } from './markdown.ts';
import { tagOpener } from './tags.ts';

// ---- Разбор блока и тегов ----------------------------------------------------

/** Блок свойств: от первой `---` до конца закрывающей строки без перевода строки; теги по порядку. */
export interface Frontmatter extends Span {
  tags: string[];
}

/** Строки текста: переводы `\r\n`, `\r`, `\n` (как у ядра). */
function* lines(text: string): Generator<Span> {
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    if (ch !== 0x0a && ch !== 0x0d) continue;
    yield { from: start, to: i };
    if (ch === 0x0d && text.charCodeAt(i + 1) === 0x0a) i++;
    start = i + 1;
  }
  yield { from: start, to: text.length };
}

const SPACES = /^[ \t]*|[ \t]*$/g;

/** Ключ `tags:`/`tag:` в начале строки (регистр ключа не важен) — значение после `:`. */
function tagsValue(line: string): string | null {
  const colon = line.indexOf(':');
  if (colon < 0) return null;
  const key = line.slice(0, colon).toLowerCase();
  return key === 'tags' || key === 'tag' ? line.slice(colon + 1) : null;
}

/** Элемент списка `- значение` (отступ пробелами и табуляциями допустим) — значение. */
function listItem(line: string): string | null {
  const rest = line.replace(/^[ \t]*/, '');
  if (!rest.startsWith('-')) return null;
  const after = rest.slice(1);
  return after === '' || after.startsWith(' ') || after.startsWith('\t') ? after : null;
}

/** Одно значение: без пробелов по краям, без парных кавычек и `#` впереди; тег, если подходит целиком. */
function pushValue(raw: string, out: string[]): void {
  let value = raw.replace(SPACES, '');
  if (value.length >= 2 && ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'")))) {
    value = value.slice(1, -1);
  }
  if (value.startsWith('#')) value = value.slice(1);
  if (isTagName(value)) out.push(value);
}

function splitValues(raw: string, out: string[]): void {
  for (const part of raw.split(',')) pushValue(part, out);
}

/**
 * Блок свойств в начале `text` и теги ключей `tags`/`tag` из него (формы
 * `[a, b]`, `a, b`, `a`, список строками `- a`); `null` — блока нет
 * (незакрытый — не блок). Размер текста не проверяется: в редакторе блок
 * не ищется в заметке больше 10 МБ (решение 77).
 */
export function parseFrontmatter(text: string): Frontmatter | null {
  const all = lines(text);
  const first = all.next();
  if (first.done || !PROPERTIES_OPEN.test(text.slice(first.value.from, first.value.to))) return null;
  const tags: string[] = [];
  let inList = false;
  for (const span of all) {
    const line = text.slice(span.from, span.to);
    if (PROPERTIES_CLOSE.test(line)) return { from: 0, to: span.to, tags };
    if (inList) {
      const item = listItem(line);
      if (item !== null) {
        pushValue(item, tags);
        continue;
      }
    }
    inList = false;
    const value = tagsValue(line);
    if (value === null) continue;
    const raw = value.replace(SPACES, '');
    if (raw === '') inList = true;
    else if (raw.startsWith('[')) {
      if (raw.length >= 2 && raw.endsWith(']')) splitValues(raw.slice(1, -1), tags);
    } else splitValues(raw, tags);
  }
  return null;
}

/** Теги блока свойств для строки «Свойства»: без повторов (`#Проект` и `#проект` — один), в первом написании. */
export function frontmatterTags(text: string): string[] {
  const seen = new Set<string>();
  return (parseFrontmatter(text)?.tags ?? []).filter((tag) => {
    const key = tagKey(tag);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

// ---- Состояние: свёрнут или раскрыт ------------------------------------------

/** Блок свойств из разбора блоков редактора; `null` — блока нет. */
export type PropertiesLookup = (state: EditorState) => Span | null;

/** Блок свойств на экране: где он, раскрыт ли, теги для строки «Свойства». */
export interface PropertiesView extends Span {
  expanded: boolean;
  tags: string[];
}

interface FieldValue {
  view: PropertiesView | null;
  decorations: DecorationSet;
}

/** Курсор (голова любого выделения) в блоке после его начала — блок раскрыт. */
function expandedAt(state: EditorState, span: Span): boolean {
  return state.selection.ranges.some((range) => range.head > span.from && range.head <= span.to);
}

function sameTags(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((tag, i) => tag === b[i]);
}

const SOURCE_LINE = 'cm-rz-properties-source';
const sourceLines = new Map<string, Decoration>();

function sourceLine(cls: string): Decoration {
  let decoration = sourceLines.get(cls);
  if (!decoration) {
    decoration = Decoration.line({ attributes: { class: cls } });
    sourceLines.set(cls, decoration);
  }
  return decoration;
}

/** Строка «Свойства»: шеврон, значок `list`, подпись, теги плашками. */
class PropertiesWidget extends WidgetType {
  readonly tags: readonly string[];

  constructor(tags: readonly string[]) {
    super();
    this.tags = tags;
  }

  override eq(other: PropertiesWidget): boolean {
    return sameTags(this.tags, other.tags);
  }

  toDOM(): HTMLElement {
    const wrap = document.createElement('div');
    wrap.className = 'cm-rz-properties-wrap';
    const row = document.createElement('div');
    row.className = 'cm-rz-properties';
    row.setAttribute('role', 'button');
    row.setAttribute('aria-expanded', 'false');
    row.setAttribute('title', t('editor.properties.expand'));
    row.append(icon('chevron', ['m9 18 6-6-6-6']), icon('list', ['M3 5h.01', 'M3 12h.01', 'M3 19h.01', 'M8 5h13', 'M8 12h13', 'M8 19h13']));
    const label = document.createElement('span');
    label.className = 'cm-rz-properties-label';
    label.textContent = t('editor.properties');
    row.append(label);
    if (this.tags.length > 0) {
      const list = document.createElement('span');
      list.className = 'cm-rz-properties-tags';
      for (const tag of this.tags) {
        const chip = document.createElement('span');
        chip.className = 'cm-rz-properties-tag';
        chip.textContent = `#${tag}`;
        chip.setAttribute('data-rz-properties-tag', tag);
        list.append(chip);
      }
      row.append(list);
    }
    wrap.append(row);
    return wrap;
  }

  override get estimatedHeight(): number {
    return 40;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

const SVG = 'http://www.w3.org/2000/svg';

/** Значок Lucide 1.61.0 (ISC, как src/shell/Icon.svelte): контуры `paths`, 14 px. */
function icon(name: string, paths: readonly string[]): SVGElement {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('class', `cm-rz-properties-icon cm-rz-properties-${name}`);
  for (const [attr, value] of [
    ['width', '14'],
    ['height', '14'],
    ['viewBox', '0 0 24 24'],
    ['fill', 'none'],
    ['stroke', 'currentColor'],
    ['stroke-width', '2'],
    ['stroke-linecap', 'round'],
    ['stroke-linejoin', 'round'],
    ['aria-hidden', 'true'],
    ['focusable', 'false'],
  ] as const) {
    svg.setAttribute(attr, value);
  }
  for (const d of paths) {
    const path = document.createElementNS(SVG, 'path');
    path.setAttribute('d', d);
    svg.append(path);
  }
  return svg;
}

function decorate(state: EditorState, view: PropertiesView): DecorationSet {
  if (!view.expanded) {
    return Decoration.set(Decoration.replace({ widget: new PropertiesWidget(view.tags), block: true }).range(view.from, view.to));
  }
  const out: Range<Decoration>[] = [];
  const first = state.doc.lineAt(view.from).number;
  const last = state.doc.lineAt(view.to).number;
  for (let n = first; n <= last; n++) {
    const cls = [SOURCE_LINE, n === first ? `${SOURCE_LINE}-first` : '', n === last ? `${SOURCE_LINE}-last` : '']
      .filter(Boolean)
      .join(' ');
    out.push(sourceLine(cls).range(state.doc.line(n).from));
  }
  // Промежуток до текста ниже — сверху у строки под блоком: фон и скругление блока не растягиваются.
  if (last < state.doc.lines) out.push(sourceLine('cm-rz-gap-before').range(state.doc.line(last + 1).from));
  return Decoration.set(out);
}

function compute(state: EditorState, lookup: PropertiesLookup, previous: FieldValue | null, tr: Transaction | null): FieldValue {
  const span = lookup(state);
  if (!span) return { view: null, decorations: Decoration.none };
  const old = previous?.view ?? null;
  // Теги — заново, только если правка задела блок (или блок появился).
  const tagsStale = !old || !tr || tr.changes.touchesRange(0, old.to) !== false;
  const tags = tagsStale || !old ? frontmatterTags(state.sliceDoc(0, span.to)) : old.tags;
  const view: PropertiesView = {
    from: span.from,
    to: span.to,
    expanded: expandedAt(state, span),
    tags: old && sameTags(old.tags, tags) ? old.tags : tags,
  };
  if (previous && old && old.from === view.from && old.to === view.to && old.expanded === view.expanded && old.tags === view.tags) {
    return { view, decorations: previous.decorations.map(tr?.changes ?? state.changes()) };
  }
  return { view, decorations: decorate(state, view) };
}

function propertiesField(lookup: PropertiesLookup) {
  return StateField.define<FieldValue>({
    create: (state) => compute(state, lookup, null, null),
    update(value, tr) {
      if (!tr.docChanged && !tr.selection && !tr.reconfigured) return value;
      return compute(tr.state, lookup, value, tr);
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}

/** Поле подключённого расширения (для `propertiesView`). */
let current: StateField<FieldValue> | null = null;

/** Блок свойств на экране (для тестов и окна); `null` — блока нет или режим исходного текста. */
export function propertiesView(state: EditorState): PropertiesView | null {
  return (current && state.field(current, false)?.view) ?? null;
}

// ---- Стрелки и щелчок ------------------------------------------------------------

function collapsedView(state: EditorState): PropertiesView | null {
  const view = propertiesView(state);
  return view && !view.expanded ? view : null;
}

/** ↑ из строки под свёрнутым блоком — в конец блока (блок раскрывается). */
function up(extend: boolean): Command {
  return (editor) => {
    const { state } = editor;
    const view = collapsedView(state);
    if (!view || view.to >= state.doc.length) return false;
    const below = state.doc.lineAt(view.to + 1).number;
    const { main } = state.selection;
    if (state.doc.lineAt(main.head).number !== below) return false;
    const range = extend ? EditorSelection.range(main.anchor, view.to) : EditorSelection.cursor(view.to);
    editor.dispatch({ selection: state.selection.replaceRange(range), scrollIntoView: true, userEvent: 'select' });
    return true;
  };
}

/** ↓ из начала текста (перед свёрнутой строкой) — на строку под блоком, мимо скрытых строк. */
function down(extend: boolean): Command {
  return (editor) => {
    const { state } = editor;
    const view = collapsedView(state);
    const { main } = state.selection;
    if (!view || main.head !== view.from) return false;
    const target = Math.min(view.to + 1, state.doc.length);
    const range = extend ? EditorSelection.range(main.anchor, target) : EditorSelection.cursor(target);
    editor.dispatch({ selection: state.selection.replaceRange(range), scrollIntoView: true, userEvent: 'select' });
    return true;
  };
}

/** Щелчок по строке «Свойства»: курсор в конец первой строки блока — блок раскрыт для правки. */
function expandAt(editor: EditorView, view: PropertiesView): void {
  const { doc } = editor.state;
  const second = doc.lineAt(view.from).number + 1;
  const target = second <= doc.lines ? Math.min(doc.line(second).to, view.to) : view.to;
  editor.dispatch({ selection: { anchor: target }, scrollIntoView: true, userEvent: 'select.pointer' });
  editor.focus();
}

/** Расширение живого предпросмотра: строка «Свойства» вместо блока свойств. */
export function frontmatterPreview(lookup: PropertiesLookup): Extension {
  const field = (current ??= propertiesField(lookup));
  return [
    field,
    Prec.high(
      keymap.of([
        { key: 'ArrowUp', run: up(false), shift: up(true) },
        { key: 'ArrowDown', run: down(false), shift: down(true) },
      ]),
    ),
    EditorView.domEventHandlers({
      mousedown(event, editor) {
        if (event.button !== 0 || !(event.target instanceof Element)) return false;
        const row = event.target.closest('.cm-rz-properties');
        if (!row) return false;
        const view = collapsedView(editor.state);
        if (!view) return false;
        event.preventDefault();
        const chip = event.target.closest('[data-rz-properties-tag]');
        const opener = editor.state.facet(tagOpener);
        if (chip && opener) {
          opener(chip.getAttribute('data-rz-properties-tag') ?? '');
          return true;
        }
        expandAt(editor, view);
        return true;
      },
    }),
  ];
}
