// Переход к заголовку по ссылке `[[Заметка#Заголовок]]` (С3.4): после
// открытия заметки курсор встаёт в начало строки заголовка, строка
// прокручивается вверх видимой части. Заголовок не найден — ничего не
// происходит. Сравнение — без учёта регистра, с нормализацией Unicode.
import { EditorSelection, type Text } from '@codemirror/state';
import { EditorView, ViewPlugin } from '@codemirror/view';
import { MAX_BLOCK_TEXT, fenceStep, findProperties, type CodeBlock, type OpenFence } from '../live-preview/markdown.ts';
import { wikiHost } from './host.ts';

const HEADING = /^ {0,3}#{1,6}(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;

function headingKey(text: string): string {
  return text.trim().normalize('NFC').toLocaleLowerCase('ru').normalize('NFC');
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

/** Курсор к заголовку и прокрутка к нему; `false` — заголовка нет. */
export function revealHeading(view: EditorView, heading: string): boolean {
  const at = findHeading(view.state.doc, heading);
  if (at === null) return false;
  view.dispatch({
    selection: EditorSelection.cursor(at),
    effects: EditorView.scrollIntoView(at, { y: 'start', yMargin: 24 }),
  });
  return true;
}

/** Переход к заголовку по событию окна (в обоих режимах редактора). */
export const revealPlugin = ViewPlugin.fromClass(
  class {
    #unsubscribe: (() => void) | null = null;
    #destroyed = false;

    constructor(view: EditorView) {
      const host = view.state.facet(wikiHost);
      // Событие может прийти сразу при подписке, пока редактор создаётся: переход — после.
      this.#unsubscribe =
        host?.subscribe((event) => {
          if (event.kind !== 'reveal') return;
          setTimeout(() => {
            if (!this.#destroyed) revealHeading(view, event.heading);
          }, 0);
        }) ?? null;
    }

    destroy() {
      this.#destroyed = true;
      this.#unsubscribe?.();
    }
  },
);
