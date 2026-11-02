// Переход по вики-ссылке к открытой заметке (С3.4, решения 85, 86): курсор
// встаёт в начало строки заголовка `[[Заметка#Заголовок]]` (без заголовка
// или если его нет — в начало текста под блоком свойств, 0.2-3.2), строка прокручивается вверх видимой
// части, фокус — в тексте: можно сразу печатать. Сравнение заголовков — без
// учёта регистра (ключ имени ядра), с нормализацией Unicode. Переход из вида
// «Обратные ссылки» (0.2-2.2) ставит курсор на саму ссылку: строка и столбец
// UTF-16 — из индекса ядра.
import { EditorSelection, type Text } from '@codemirror/state';
import { EditorView, ViewPlugin } from '@codemirror/view';
import { MAX_BLOCK_TEXT, fenceStep, findProperties, type CodeBlock, type OpenFence } from '../live-preview/markdown.ts';
import { nameKey } from '../../core/names/index.ts';
import { textStart } from '../live-preview/frontmatter.ts';
import { wikiHost, type LinkPosition } from './host.ts';

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
 * Смещение в тексте по строке (с 1) и столбцу UTF-16. Текст могли изменить
 * после сборки индекса: строка и столбец ограничиваются текстом.
 */
export function positionAt(doc: Text, at: LinkPosition): number {
  const line = doc.line(Math.min(Math.max(Math.trunc(at.line), 1), doc.lines));
  return line.from + Math.min(Math.max(Math.trunc(at.column), 0), line.length);
}

/**
 * Курсор на ссылку `link` или к заголовку `heading` (нет его или `null` — в начало текста под блоком свойств),
 * прокрутка к нему и фокус в текст: можно сразу печатать (решение 86). В
 * заметке только для чтения фокус не ставится, как при открытии (NoteText).
 */
export function arriveAt(view: EditorView, heading: string | null, link?: LinkPosition): void {
  const doc = view.state.doc;
  const at = link ? positionAt(doc, link) : ((heading === null ? null : findHeading(doc, heading)) ?? textStart(doc));
  view.dispatch({
    selection: EditorSelection.cursor(at),
    // Ссылка из обратных ссылок — в середине видимой части, заголовок — вверху.
    effects: EditorView.scrollIntoView(at, link ? { y: 'center' } : { y: 'start', yMargin: 24 }),
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
            if (!this.#destroyed) arriveAt(view, event.heading, event.at);
          }, 0);
        }) ?? null;
    }

    destroy() {
      this.#destroyed = true;
      this.#unsubscribe?.();
    }
  },
);
