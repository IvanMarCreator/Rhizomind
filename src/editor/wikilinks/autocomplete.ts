// Автодополнение вики-ссылок (С3.4): после `[[` под текстом — список
// заметок по набранной части имени (команда ядра `links_suggest`). Enter или
// Tab вставляет имя (для повторяющихся имён — кратчайший различающий путь,
// его считает ядро) и закрывающие `]]`; ↑/↓ — выбор, Esc — закрыть.
// Заметка, на которую нельзя сослаться (`#`, `[`, `]`, пробел в начале
// имени), показана помеченной и не вставляется (С3.2).
//
// Свой небольшой список вместо @codemirror/autocomplete: без новой
// зависимости (решение 75 — меньше чужого кода для проверки ИБ).
import { Prec, StateEffect, StateField, type EditorState, type Extension } from '@codemirror/state';
import {
  EditorView,
  ViewPlugin,
  keymap,
  showTooltip,
  tooltips,
  type Tooltip,
  type TooltipView,
  type ViewUpdate,
} from '@codemirror/view';
import type { LinkSuggestion } from '../../api/index.ts';
import { LIMITS } from '../../api/limits.ts';
import { t } from '../../i18n/index.ts';
import { wikiHost } from './host.ts';

/** Набираемая ссылка: `from` — сразу после `[[`, `to` — курсор. */
export interface Trigger {
  from: number;
  to: number;
  query: string;
}

/** Открытый список. `items: null` — подсказок по этому запросу ещё нет. */
export interface Completion extends Trigger {
  items: readonly LinkSuggestion[] | null;
  /** Запрос, на который получены `items` (пока новых нет, виден прежний список). */
  itemsQuery: string | null;
  selected: number;
  tooltip: Tooltip;
}

interface CompletionState {
  active: Completion | null;
  /** Esc закрыл список ссылки, начатой здесь: он не открывается, пока ссылку не начнут заново. */
  dismissed: number | null;
}

/**
 * Набирается ли ссылка у курсора: на строке до курсора есть `[[`, после
 * неё нет `]`, `|`, `#`. Встраивание `![[` — выпуск 0.3. В заметке только
 * для чтения списка нет.
 */
export function findTrigger(state: EditorState): Trigger | null {
  const { ranges, main } = state.selection;
  if (state.readOnly || ranges.length !== 1 || !main.empty) return null;
  const line = state.doc.lineAt(main.head);
  const before = line.text.slice(0, main.head - line.from);
  const open = before.lastIndexOf('[[');
  if (open < 0 || (open > 0 && before[open - 1] === '!')) return null;
  const query = before.slice(open + 2);
  if (/[\]|#]/.test(query) || [...query].length > LIMITS.max_query_chars) return null;
  return { from: line.from + open + 2, to: main.head, query };
}

const itemsEffect = StateEffect.define<{ from: number; query: string; items: readonly LinkSuggestion[] }>();
const moveEffect = StateEffect.define<number>();
const dismissEffect = StateEffect.define<null>();

const completionField = StateField.define<CompletionState>({
  create: () => ({ active: null, dismissed: null }),
  update(value, tr) {
    let dismissed = value.dismissed === null ? null : tr.changes.mapPos(value.dismissed, -1);
    let previous = value.active;
    for (const effect of tr.effects) {
      if (effect.is(dismissEffect) && previous) {
        dismissed = previous.from;
        previous = null;
      }
    }
    const trigger = findTrigger(tr.state);
    if (!trigger || trigger.from === dismissed) {
      return { active: null, dismissed: trigger ? dismissed : null };
    }
    const from = previous ? tr.changes.mapPos(previous.from, -1) : -1;
    let active: Completion =
      previous && from === trigger.from
        ? { ...previous, ...trigger }
        : {
            ...trigger,
            items: null,
            itemsQuery: null,
            selected: 0,
            tooltip: { pos: trigger.from - 2, above: false, create: (view) => new SuggestionList(view) },
          };
    for (const effect of tr.effects) {
      if (effect.is(itemsEffect) && effect.value.from === active.from && effect.value.query === active.query) {
        const items = effect.value.items;
        const firstLinkable = items.findIndex((item) => item.linkable);
        active = { ...active, items, itemsQuery: active.query, selected: Math.max(0, firstLinkable) };
      } else if (effect.is(moveEffect) && active.items && active.items.length > 0) {
        const count = active.items.length;
        active = { ...active, selected: (active.selected + effect.value + count) % count };
      }
    }
    return { active, dismissed };
  },
  provide: (field) => showTooltip.from(field, (value) => value.active?.tooltip ?? null),
});

/** Открытый список подсказок (для тестов и оболочки). */
export function activeCompletion(state: EditorState): Completion | null {
  return state.field(completionField, false)?.active ?? null;
}

/** Вставить подсказку `index`: текст ссылки и `]]` (если их ещё нет), курсор — после `]]`. */
export function acceptSuggestion(view: EditorView, index: number): boolean {
  const active = activeCompletion(view.state);
  const item = active?.items?.[index];
  if (!active || !item) return false;
  if (!item.linkable || item.insert === null) return true;
  const closed = view.state.sliceDoc(active.to, active.to + 2) === ']]';
  const insert = closed ? item.insert : `${item.insert}]]`;
  view.dispatch({
    changes: { from: active.from, to: active.to, insert },
    selection: { anchor: active.from + item.insert.length + 2 },
    userEvent: 'input.complete',
    scrollIntoView: true,
  });
  return true;
}

const listKeymap = Prec.highest(
  keymap.of([
    {
      key: 'ArrowDown',
      run: (view) => move(view, 1),
    },
    {
      key: 'ArrowUp',
      run: (view) => move(view, -1),
    },
    { key: 'Enter', run: (view) => accept(view) },
    { key: 'Tab', run: (view) => accept(view) },
    {
      key: 'Escape',
      run: (view) => {
        if (!activeCompletion(view.state)) return false;
        view.dispatch({ effects: dismissEffect.of(null) });
        return true;
      },
    },
  ]),
);

function move(view: EditorView, delta: number): boolean {
  const active = activeCompletion(view.state);
  if (!active?.items || active.items.length === 0) return false;
  view.dispatch({ effects: moveEffect.of(delta) });
  return true;
}

function accept(view: EditorView): boolean {
  const active = activeCompletion(view.state);
  if (!active?.items || active.items.length === 0) return false;
  return acceptSuggestion(view, active.selected);
}

let listCount = 0;

/** Список под набираемой ссылкой. Обновляется из состояния, без своих данных. */
class SuggestionList implements TooltipView {
  readonly dom: HTMLElement;
  readonly #view: EditorView;
  readonly #id = `rz-wikilink-suggest-${++listCount}`;
  #shown: Completion | null = null;

  constructor(view: EditorView) {
    this.#view = view;
    this.dom = document.createElement('div');
    this.dom.className = 'cm-rz-suggest';
    this.dom.addEventListener('mousedown', (event) => {
      // Фокус остаётся в тексте; выбор — по пункту.
      event.preventDefault();
      const option = event.target instanceof Element ? event.target.closest('[data-index]') : null;
      if (option) acceptSuggestion(this.#view, Number(option.getAttribute('data-index')));
    });
    this.#render();
  }

  update(update: ViewUpdate): void {
    if (activeCompletion(update.state) !== this.#shown) this.#render();
  }

  #render(): void {
    const active = activeCompletion(this.#view.state);
    this.#shown = active;
    if (!active) return;
    const list = document.createElement('div');
    list.id = this.#id;
    list.setAttribute('role', 'listbox');
    list.setAttribute('aria-label', t('editor.wikilink.suggestions'));
    if (active.items === null) {
      list.append(note(t('editor.wikilink.loading')));
    } else if (active.items.length === 0) {
      list.append(note(t('editor.wikilink.noMatches')));
    }
    (active.items ?? []).forEach((item, index) => list.append(option(item, index, index === active.selected, this.#id)));
    this.dom.replaceChildren(list);
    const current = list.querySelector('[aria-selected="true"]');
    if (current && 'scrollIntoView' in current) current.scrollIntoView({ block: 'nearest' });
  }
}

function note(text: string): HTMLElement {
  const element = document.createElement('div');
  element.className = 'cm-rz-suggest-note';
  element.textContent = text;
  return element;
}

function option(item: LinkSuggestion, index: number, selected: boolean, listId: string): HTMLElement {
  const element = document.createElement('div');
  element.id = `${listId}-${index}`;
  element.className = 'cm-rz-suggest-item';
  element.setAttribute('role', 'option');
  element.setAttribute('data-index', String(index));
  element.setAttribute('aria-selected', String(selected));
  const name = document.createElement('span');
  name.className = 'cm-rz-suggest-name';
  name.textContent = item.name;
  element.append(name);
  const where = item.insert ?? item.path.join('/');
  if (where !== item.name) {
    const path = document.createElement('span');
    path.className = 'cm-rz-suggest-path';
    path.textContent = where;
    element.append(path);
  }
  if (!item.linkable) {
    element.setAttribute('aria-disabled', 'true');
    const mark = document.createElement('span');
    mark.className = 'cm-rz-suggest-unlinkable';
    mark.textContent = t('editor.wikilink.unlinkable');
    element.append(mark);
  }
  return element;
}

/** Запрос подсказок у окна, когда сменился набранный текст. Устаревший ответ отбрасывается полем. */
const requestPlugin = ViewPlugin.fromClass(
  class {
    readonly view: EditorView;
    #asked: string | null = null;
    #destroyed = false;

    constructor(view: EditorView) {
      this.view = view;
      this.#request();
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.selectionSet) this.#request();
    }

    #request() {
      const active = activeCompletion(this.view.state);
      const host = this.view.state.facet(wikiHost);
      if (!active || !host) {
        this.#asked = null;
        return;
      }
      const ask = `${active.from}\u0000${active.query}`;
      if (ask === this.#asked || active.itemsQuery === active.query) return;
      this.#asked = ask;
      const { from, query } = active;
      host.suggest(query).then(
        (items) => {
          if (!this.#destroyed) this.view.dispatch({ effects: itemsEffect.of({ from, query, items }) });
        },
        () => {
          if (!this.#destroyed) this.view.dispatch({ effects: itemsEffect.of({ from, query, items: [] }) });
        },
      );
    }

    destroy() {
      this.#destroyed = true;
    }
  },
);

/** Автодополнение вики-ссылок (в обоих режимах редактора). */
export function wikilinkCompletion(): Extension {
  // Список — относительно редактора (`.cm-editor { position: relative }` в
  // wikilinks.css): базовые стили CodeMirror в окне не действуют (Н7), и
  // без этого список оказывался у левого края окна.
  return [tooltips({ position: 'absolute' }), completionField, listKeymap, requestPlugin];
}
