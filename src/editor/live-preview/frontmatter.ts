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
// (решение 77). Теги блока — общий разбор src/core/tags/frontmatter.ts.
//
// Когда блок раскрыт: курсор (голова любого выделения) стоит в блоке после
// его первого символа — `(from, to]`, или в его начале, если блок уже был
// раскрыт (Home в первой строке). Так курсор никогда не оказывается внутри
// скрытого текста. Стрелки ↑/↓ обходят свёрнутый блок как одну строку: ↑ из
// строки под блоком ставит курсор в конец блока (он раскрывается).
//
// Начало текста при свёрнутом блоке — перед `---`: ввод там разрушил бы блок
// (инвариант 4, замечание QA). Поэтому заметка открывается и переход по
// `[[ссылке]]` без заголовка ведёт с курсором в начало текста под блоком
// (`textStart`, NoteText.svelte и wikilinks/reveal.ts); курсор, поставленный
// в начало текста при свёрнутом блоке (Ctrl+Home, переключение режима и
// др.), переходит туда же;
// ввод и вставка в начало текста при свёрнутом блоке уходят под блок одной
// правкой (одна отмена).
//
// Составление (IME): ввод в раскрытом блоке оставляет курсор в блоке — блок
// не сворачивается; оформление раскрытого блока — только классы строк (одни
// и те же объекты оформления), узлы текста не разрезаются.
import {
  EditorSelection,
  EditorState,
  Prec,
  StateEffect,
  StateField,
  Transaction,
  type Extension,
  type Range,
  type Text,
  type TransactionSpec,
} from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, WidgetType, keymap, type Command, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { frontmatterTags } from '../../core/tags/index.ts';
import { t } from '../../i18n/index.ts';
import { MAX_BLOCK_TEXT, findProperties, type Span } from './markdown.ts';
import { tagOpener } from './tags.ts';

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

/**
 * Курсор (голова любого выделения) внутри блока — блок раскрыт. Края блока:
 * начало текста и конец блока, за которым текста нет (заметка только из
 * блока свойств), — места курсора «перед» и «под» свёрнутой строкой: там
 * блок остаётся, каким был. Конец блока с текстом ниже — внутри блока.
 */
function expandedAt(state: EditorState, span: Span, wasExpanded: boolean): boolean {
  const atEnd = span.to >= state.doc.length;
  return state.selection.ranges.some((range) => {
    const { head } = range;
    if (head > span.from && head < span.to) return true;
    if (head === span.to) return !atEnd || wasExpanded;
    return head === span.from && wasExpanded;
  });
}

/**
 * Начало текста под блоком свойств: начало строки после закрывающей (если
 * после блока текста нет — конец блока: ввод там начинается с новой строки,
 * см. `guardStart`); без блока или в заметке больше 10 МБ (решение 77) —
 * `0`. Сюда встаёт курсор при открытии заметки.
 */
export function textStart(doc: Text): number {
  const properties = doc.length > MAX_BLOCK_TEXT ? null : findProperties(doc);
  return properties ? Math.min(properties.to + 1, doc.length) : 0;
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
    const cls = [
      SOURCE_LINE,
      n === first ? `${SOURCE_LINE}-first` : '',
      n === last ? `${SOURCE_LINE}-last` : '',
      // Строки-ограничители `---` / `...` бледнее (макет П-4).
      n === first || n === last ? 'cm-rz-properties-delimiter' : '',
    ]
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
    expanded: expandedAt(state, span, old?.expanded === true || tr?.effects.some((effect) => effect.is(expandEffect)) === true),
    tags: old && sameTags(old.tags, tags) ? old.tags : tags,
  };
  // Сдвинуть прежнее оформление можно, только если правка не задела блок и его
  // конец: вставка ровно в конец блока при сдвиге вошла бы в скрытый участок
  // (конец замены включительный) — набранное под блоком пропало бы с экрана
  // (О-1). Иначе — заново: виджет с теми же тегами не перерисовывается.
  const untouched = !tr || tr.changes.touchesRange(0, old?.to ?? 0) === false;
  if (previous && old && untouched && old.from === view.from && old.to === view.to && old.expanded === view.expanded && old.tags === view.tags) {
    return { view, decorations: previous.decorations.map(tr?.changes ?? state.changes()) };
  }
  return { view, decorations: decorate(state, view) };
}

/** Раскрыть блок при курсоре на его краю (конец блока без текста под ним): Backspace там. */
const expandEffect = StateEffect.define<null>();

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

/**
 * Backspace в начале строки под свёрнутым блоком склеил бы её с `---`, а в
 * конце свёрнутого блока без текста под ним стёр бы `-` из невидимой
 * `---`: вместо этого блок раскрывается, курсор — в конце `---`, ничего не
 * удаляется.
 */
const backspace: Command = (editor) => {
  const { state } = editor;
  const view = collapsedView(state);
  const { main } = state.selection;
  if (!view || state.selection.ranges.length !== 1 || !main.empty) return false;
  if (main.head !== view.to + 1 && !(main.head === view.to && view.to >= state.doc.length)) return false;
  editor.dispatch({
    selection: EditorSelection.cursor(view.to),
    effects: expandEffect.of(null),
    scrollIntoView: true,
    userEvent: 'select',
  });
  return true;
};

/**
 * Составление (IME) в конце свёрнутого блока без текста под ним дописало бы
 * текст к `---`, а переносить правки составления нельзя — их узел уже в окне.
 * Поэтому до появления узла составления отдельной правкой вставляется
 * перевод строки, и составление идёт на новой строке под блоком.
 */
function newLineBeforeComposition(editor: EditorView): void {
  insertBelowOnlyBlock(editor, '');
}

/**
 * Свёрнутый блок — весь текст заметки (под ним нет даже перевода строки), а
 * курсор у его края: строк для ввода в окне нет, и браузер набранное не
 * вставляет — ввод пропадал (О-1, WebKitGTK и WebView2). Тогда редактор
 * вставляет сам: перевод строки и `text` одной правкой (одна отмена), курсор
 * — в конце. `false` — не тот случай.
 */
function insertBelowOnlyBlock(editor: EditorView, text: string): boolean {
  const { state } = editor;
  const view = collapsedView(state);
  const { main } = state.selection;
  if (!view || view.to < state.doc.length || state.selection.ranges.length !== 1 || !main.empty) return false;
  if (main.head !== view.to && main.head !== view.from) return false;
  if (state.readOnly) return false;
  const insert = `\n${text}`;
  editor.dispatch({
    changes: { from: view.to, insert },
    selection: EditorSelection.cursor(view.to + insert.length),
    scrollIntoView: true,
    userEvent: 'input.type',
    // Мимо `guardStart`: перевод строки уже на своём месте, второй не нужен.
    filter: false,
  });
  return true;
}

/** Нажатие вводит один символ (без Ctrl, Cmd; Alt — только как AltGr): `Dead`, `Process`, F5 — не ввод. */
function typedChar(event: KeyboardEvent): string | null {
  if (event.ctrlKey !== event.altKey || event.metaKey || event.isComposing) return null;
  if (event.ctrlKey && !event.getModifierState('AltGraph')) return null;
  return [...event.key].length === 1 ? event.key : null;
}

/**
 * Щелчок по строке «Свойства»: курсор в конец первой строки свойств — блок
 * раскрыт для правки. В пустом блоке `---` / `---` строки свойств нет:
 * между ограничителями вставляется пустая строка (правка по щелчку, её можно
 * отменить), курсор — на ней; иначе ввод дописывался бы к `---`.
 */
function expandAt(editor: EditorView, view: PropertiesView): void {
  const { doc } = editor.state;
  const first = doc.lineAt(view.from);
  const second = first.number + 1;
  if (second <= doc.lines && doc.line(second).to < view.to) {
    editor.dispatch({ selection: { anchor: doc.line(second).to }, scrollIntoView: true, userEvent: 'select.pointer' });
  } else {
    editor.dispatch({
      changes: { from: first.to, insert: '\n' },
      selection: { anchor: first.to + 1 },
      scrollIntoView: true,
      userEvent: 'input',
    });
  }
  editor.focus();
}

/**
 * Начало текста при свёрнутом блоке. Ввод и вставка только в позицию 0
 * (`input.*`, кроме составления — его узел уже в окне) переносятся в начало
 * текста под блоком: одна правка вместо исходной, отмена — одна. Курсор,
 * поставленный в позицию 0, переходит туда же.
 */
const guardStart = EditorState.transactionFilter.of((tr): Transaction | readonly (Transaction | TransactionSpec)[] | TransactionSpec => {
  const before = propertiesView(tr.startState);
  if (before?.expanded) return tr;
  if (before && tr.docChanged && tr.isUserEvent('input') && !tr.isUserEvent('input.type.compose')) {
    const length = tr.startState.doc.length;
    // Под блоком нет текста: конец блока — тоже край свёрнутой строки.
    const noBody = before.to >= length;
    let at: number | null = null;
    let single = true;
    let inserted = '';
    tr.changes.iterChanges((fromA, toA, _fromB, _toB, text) => {
      if (fromA !== toA || (fromA !== before.from && !(noBody && fromA === before.to)) || (at !== null && at !== fromA)) single = false;
      at = fromA;
      inserted += text.toString();
    });
    if (single && at !== null) {
      // Без текста под блоком набранное начинается с новой строки: `---` остаётся закрывающей строкой.
      const start = noBody ? length : textStart(tr.startState.doc);
      const prefix = noBody ? '\n' : '';
      const offset = Math.min(Math.max(tr.newSelection.main.head - at, 0), inserted.length);
      return {
        changes: { from: start, insert: prefix + inserted },
        selection: EditorSelection.cursor(start + prefix.length + offset),
        scrollIntoView: true,
        userEvent: tr.annotation(Transaction.userEvent) ?? 'input',
      };
    }
  }
  if (!tr.selection && !tr.docChanged) return tr;
  const { main } = tr.newSelection;
  if (tr.newSelection.ranges.length !== 1 || !main.empty || main.head !== 0) return tr;
  const after = propertiesView(tr.state);
  if (!after || after.expanded || after.from !== 0) return tr;
  return [tr, { selection: EditorSelection.cursor(textStart(tr.state.doc)), sequential: true }];
});

/**
 * Курсор уже в начале текста, когда расширение подключается (переключение
 * из исходного текста, редактор без заданного выделения) или после отмены
 * и повтора: затем — в начало текста под свёрнутым блоком. Правкой это
 * сделать нельзя: фильтр правок не видит переключения режима, отмена идёт
 * мимо фильтра.
 */
const startOnAttach = ViewPlugin.define((view) => {
  let alive = true;
  const relocate = () =>
    queueMicrotask(() => {
      if (!alive || view.composing) return;
      const { main } = view.state.selection;
      const shown = collapsedView(view.state);
      if (!shown || view.state.selection.ranges.length !== 1 || !main.empty || main.head !== shown.from) return;
      view.dispatch({ selection: EditorSelection.cursor(textStart(view.state.doc)) });
    });
  relocate();
  return {
    // Отмена и повтор идут мимо фильтра правок и возвращают прежнее выделение — меняется только выделение.
    update(update: ViewUpdate) {
      if (update.transactions.some((tr) => tr.isUserEvent('undo') || tr.isUserEvent('redo'))) relocate();
    },
    destroy() {
      alive = false;
    },
  };
});

/** Расширение живого предпросмотра: строка «Свойства» вместо блока свойств. */
export function frontmatterPreview(lookup: PropertiesLookup): Extension {
  const field = (current ??= propertiesField(lookup));
  return [
    field,
    guardStart,
    startOnAttach,
    Prec.high(
      keymap.of([
        { key: 'ArrowUp', run: up(false), shift: up(true) },
        { key: 'ArrowDown', run: down(false), shift: down(true) },
        { key: 'Backspace', run: backspace },
      ]),
    ),
    EditorView.domEventHandlers({
      compositionstart(_event, editor) {
        newLineBeforeComposition(editor);
        return false;
      },
      keydown(event, editor) {
        const char = typedChar(event);
        if (char === null || !insertBelowOnlyBlock(editor, char)) return false;
        event.preventDefault();
        return true;
      },
      beforeinput(event, editor) {
        // Ввод без нажатия клавиши (экранная клавиатура, автозамена): то же.
        if (event.inputType !== 'insertText' || !event.data || !insertBelowOnlyBlock(editor, event.data)) return false;
        event.preventDefault();
        return true;
      },
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
