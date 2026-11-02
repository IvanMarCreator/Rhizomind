// Русский ввод с составлением (IME) рядом с тегом (0.2-3.1, замечание QA):
// плашка, появившаяся или изменившаяся посреди составления, разрезала бы узел
// составления, и браузер дописал бы набранное заново («#тег#те#т»). Пока идёт
// составление, плашки только сдвигаются; пересчёт — после `compositionend`.
// Составление в jsdom — как в образце QA: текст узла меняется напрямую, затем
// события composition* и разбор изменений DOM редактором.
import { EditorView } from '@codemirror/view';
import { waitFor } from '@testing-library/svelte';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';

let view: EditorView | null = null;

// jsdom не считает раскладку: без заглушки CodeMirror пишет в журнал
// TypeError из измерения выделения (Range.getClientRects).
const originalRects = Range.prototype.getClientRects;
const originalRect = Range.prototype.getBoundingClientRect;

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

afterAll(() => {
  Range.prototype.getClientRects = originalRects;
  Range.prototype.getBoundingClientRect = originalRect;
});

afterEach(() => {
  view?.destroy();
  view = null;
  document.body.innerHTML = '';
});

function open(text: string): EditorView {
  view = new EditorView({ parent: document.body, state: createEditorState(text, { mode: 'live_preview' }) });
  view.dispatch({ selection: { anchor: text.length } });
  return view;
}

/** Текстовый узел, который кончается в позиции `pos`. */
function textNodeEndingAt(editor: EditorView, pos: number): Text {
  const { node, offset } = editor.domAtPos(pos);
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  let found: Node | null = node.childNodes[offset - 1] ?? null;
  while (found && found.nodeType !== Node.TEXT_NODE) found = found.lastChild;
  if (!found) throw new Error('нет текстового узла');
  return found as Text;
}

/** Разобрать изменения DOM сейчас (в окне это делает MutationObserver). */
function flush(editor: EditorView): void {
  (editor as unknown as { observer: { flush(): void } }).observer.flush();
}

const plaques = (editor: EditorView) => [...editor.contentDOM.querySelectorAll('.cm-rz-tag')].map((el) => el.textContent);

/** Составление в конце текста: шаги — набранное к этому шагу (`т`, `те`, `тег`). */
function compose(editor: EditorView, steps: readonly string[]): void {
  editor.contentDOM.dispatchEvent(new CompositionEvent('compositionstart', { data: '' }));
  const node = textNodeEndingAt(editor, editor.state.selection.main.head);
  const base = node.nodeValue ?? '';
  for (const step of steps) {
    node.nodeValue = base + step;
    window.getSelection()?.collapse(node, node.nodeValue.length);
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionupdate', { data: step }));
    flush(editor);
    // Узел составления цел: плашка его не разрезала.
    expect(node.isConnected).toBe(true);
  }
  editor.contentDOM.dispatchEvent(new CompositionEvent('compositionend', { data: steps.at(-1) ?? '' }));
  flush(editor);
}

describe('IME рядом с тегом', () => {
  it('набор `#` и «тег» составлением: текст без повторов, плашка — после составления', async () => {
    const editor = open('Текст #');
    compose(editor, ['т', 'те', 'тег']);
    expect(editor.state.doc.toString()).toBe('Текст #тег');
    await waitFor(() => expect(plaques(editor)).toEqual(['#тег']));
  });

  it('знак, который кончает тег, набран составлением сразу после тега', async () => {
    const editor = open('Текст #тег');
    compose(editor, ['。']);
    expect(editor.state.doc.toString()).toBe('Текст #тег。');
    await waitFor(() => expect(plaques(editor)).toEqual(['#тег']));
  });

  it('ввод составлением внутри существующего тега (в конце плашки)', async () => {
    const editor = open('Текст #тег');
    compose(editor, ['и', 'ив']);
    expect(editor.state.doc.toString()).toBe('Текст #тегив');
    await waitFor(() => expect(plaques(editor)).toEqual(['#тегив']));
  });
});
