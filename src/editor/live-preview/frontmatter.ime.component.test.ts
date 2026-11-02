// Русский ввод с составлением (IME) в раскрытом блоке свойств (0.2-3.2,
// инвариант 4): составление не сворачивает блок, узел составления не
// разрезается оформлением, набранное попадает в текст один раз. Составление
// в jsdom — как в tags.ime.component.test.ts: текст узла меняется напрямую,
// затем события composition* и разбор изменений DOM редактором.
import { EditorView } from '@codemirror/view';
import { waitFor } from '@testing-library/svelte';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';

let view: EditorView | null = null;

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

function open(text: string, cursor: number): EditorView {
  view = new EditorView({ parent: document.body, state: createEditorState(text, { mode: 'live_preview' }) });
  view.dispatch({ selection: { anchor: cursor }, userEvent: 'select.pointer' });
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

const collapsed = (editor: EditorView) => editor.contentDOM.querySelector('.cm-rz-properties') !== null;

/** Составление в позиции курсора (в конце текстового узла): шаги — набранное к этому шагу. */
function compose(editor: EditorView, steps: readonly string[]): void {
  editor.contentDOM.dispatchEvent(new CompositionEvent('compositionstart', { data: '' }));
  const node = textNodeEndingAt(editor, editor.state.selection.main.head);
  const base = node.nodeValue ?? '';
  for (const step of steps) {
    node.nodeValue = base + step;
    window.getSelection()?.collapse(node, node.nodeValue.length);
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionupdate', { data: step }));
    flush(editor);
    // Узел составления цел, блок не свернулся посреди составления.
    expect(node.isConnected).toBe(true);
    expect(collapsed(editor)).toBe(false);
  }
  editor.contentDOM.dispatchEvent(new CompositionEvent('compositionend', { data: steps.at(-1) ?? '' }));
  flush(editor);
}

describe('IME в раскрытом блоке свойств', () => {
  it('значение тега составлением в конце строки `tags:`', async () => {
    const text = '---\ntags: проект, \n---\nТекст';
    const editor = open(text, text.indexOf('\n---'));
    expect(collapsed(editor)).toBe(false);
    compose(editor, ['и', 'ид', 'иде', 'идея']);
    expect(editor.state.doc.toString()).toBe('---\ntags: проект, идея\n---\nТекст');
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(collapsed(editor)).toBe(false);
    // Курсор ушёл — блок свернулся, новый тег — в строке «Свойства».
    editor.dispatch({ selection: { anchor: editor.state.doc.length }, userEvent: 'select.pointer' });
    await waitFor(() => expect(collapsed(editor)).toBe(true));
    const tags = [...editor.contentDOM.querySelectorAll('.cm-rz-properties-tag')].map((el) => el.textContent);
    expect(tags).toEqual(['#проект', '#идея']);
  });

  it('элемент списка составлением в строке `- `', () => {
    const text = '---\ntags:\n  - \n---\n';
    const editor = open(text, text.indexOf('\n---'));
    compose(editor, ['п', 'пл', 'план']);
    expect(editor.state.doc.toString()).toBe('---\ntags:\n  - план\n---\n');
    expect(collapsed(editor)).toBe(false);
  });

  it('составление в тексте под свёрнутым блоком: блок остаётся свёрнутым, текст — как набран', () => {
    const text = '---\ntags: [проект]\n---\nТекст ';
    const editor = open(text, text.length);
    expect(collapsed(editor)).toBe(true);
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionstart', { data: '' }));
    const node = textNodeEndingAt(editor, editor.state.selection.main.head);
    const base = node.nodeValue ?? '';
    for (const step of ['с', 'сл', 'слово']) {
      node.nodeValue = base + step;
      window.getSelection()?.collapse(node, node.nodeValue.length);
      editor.contentDOM.dispatchEvent(new CompositionEvent('compositionupdate', { data: step }));
      flush(editor);
      expect(node.isConnected).toBe(true);
      expect(collapsed(editor)).toBe(true);
    }
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionend', { data: 'слово' }));
    flush(editor);
    expect(editor.state.doc.toString()).toBe('---\ntags: [проект]\n---\nТекст слово');
  });
});
