// Переход по вики-ссылке к открытой заметке (С3.4, решения 85, 86): курсор
// встаёт в начало строки заголовка `[[Заметка#Заголовок]]` (без заголовка
// или если его нет — в начало заметки), строка прокручивается вверх видимой
// части, фокус — в тексте: можно сразу печатать. Сравнение заголовков — без
// учёта регистра (ключ имени ядра), с нормализацией Unicode.
import { EditorSelection, type Text } from '@codemirror/state';
import { EditorView, ViewPlugin } from '@codemirror/view';
import { MAX_BLOCK_TEXT, fenceStep, findProperties, type CodeBlock, type OpenFence } from '../live-preview/markdown.ts';
import { nameKey } from '../../core/names/index.ts';
import { wikiHost } from './host.ts';

const HEADING = /^ {0,3}#{1,6}(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;

function headingKey(text: string): string {
  return nameKey(text.trim());
}

/** Начало строки заголовка `heading` вне блоков кода и блока свойств; `null` — нет такого. */
export function findHeading(doc: Text, heading: string): number | null {
  const want = headingKey(heading);
  if (want === '') return null;
  const properties = doc.length > MAX_BLOCK_TEXT ? null : findProperties(doc);
  let open: OpenFence | null = null;
  const closed: CodeBlock[] = [];
  const first = properties ? doc.lineAt(properties.to).number + 1 : 1;
  for (let n = first; n <= doc.lines; n++) {
    const line = doc.line(n);
    const wasOpen = open !== null;
    open = fenceStep(open, line.text, line.from, line.to, closed);
    if (wasOpen || open !== null) continue;
    const match = HEADING.exec(line.text);
    if (match && headingKey(match[1] ?? '') === want) return line.from;
  }
  return null;
}

/**
 * Курсор к заголовку `heading` (нет его или `null` — в начало заметки),
 * прокрутка к нему и фокус в текст: можно сразу печатать (решение 86). В
 * заметке только для чтения фокус не ставится, как при открытии (NoteText).
 */
export function arriveAt(view: EditorView, heading: string | null): void {
  const at = (heading === null ? null : findHeading(view.state.doc, heading)) ?? 0;
  view.dispatch({
    selection: EditorSelection.cursor(at),
    effects: EditorView.scrollIntoView(at, { y: 'start', yMargin: 24 }),
  });
  if (!view.state.readOnly) view.focus();
}

/** Переход по ссылке к открытой заметке — по событию окна (в обоих режимах редактора). */
export const revealPlugin = ViewPlugin.fromClass(
  class {
    #unsubscribe: (() => void) | null = null;
    #destroyed = false;

    constructor(view: EditorView) {
      const host = view.state.facet(wikiHost);
      // Событие может прийти сразу при подписке, пока редактор создаётся: переход — после.
      this.#unsubscribe =
        host?.subscribe((event) => {
          if (event.kind !== 'arrive') return;
          setTimeout(() => {
            if (!this.#destroyed) arriveAt(view, event.heading);
          }, 0);
        }) ?? null;
    }

    destroy() {
      this.#destroyed = true;
      this.#unsubscribe?.();
    }
  },
);
