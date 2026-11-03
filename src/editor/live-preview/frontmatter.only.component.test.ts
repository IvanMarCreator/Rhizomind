// Заметка только из блока свойств без перевода строки в конце (О-1, отчёт QA
// 0.2-8.3): свёрнутый блок был бы всем текстом — строк для курсора в окне
// нет, выделение браузера вне поля, и вставка, составление, экранная
// клавиатура, голосовой ввод до редактора не доходили. Поэтому без текста
// под блоком блок не сворачивается: он раскрыт, курсор — в видимой строке, ввод
// идёт обычным путём; ввод в конец `---` начинается с новой строки (`guardStart`).
import { undo } from '@codemirror/commands';
import { EditorView } from '@codemirror/view';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';

const ONLY = '---\ntags: [идея]\n---';

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

function open(text = ONLY, cursor = text.length): EditorView {
  view = new EditorView({ parent: document.body, state: createEditorState(text, { mode: 'live_preview' }) });
  view.dispatch({ selection: { anchor: cursor } });
  return view;
}

const collapsed = (editor: EditorView) => editor.contentDOM.querySelector('.cm-rz-properties') !== null;

function press(editor: EditorView, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  editor.contentDOM.dispatchEvent(event);
  return event;
}

describe('О-1: заметка только из блока свойств', () => {
  it('без текста под блоком блок раскрыт: строки видны, курсор в строке', () => {
    const editor = open();
    expect(collapsed(editor)).toBe(false);
    expect([...editor.contentDOM.querySelectorAll('.cm-line')].map((line) => line.textContent)).toEqual(['---', 'tags: [идея]', '---']);
    // Курсор вне блока (в начале текста) — блок всё равно раскрыт.
    editor.dispatch({ selection: { anchor: 0 } });
    expect(collapsed(editor)).toBe(false);
  });

  it('с переводом строки в конце блок свёрнут, пустая строка под ним — видимая строка редактора', () => {
    const editor = open(`${ONLY}\n`);
    expect(collapsed(editor)).toBe(true);
    expect([...editor.contentDOM.querySelectorAll('.cm-line')].map((line) => line.textContent)).toEqual(['']);
  });

  it('вставка из буфера (Ctrl+V) в конец блока — с новой строки, блок цел', () => {
    const editor = open();
    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', { value: { getData: (type: string) => (type === 'text/plain' ? 'Вставка' : '') } });
    editor.contentDOM.dispatchEvent(paste);
    expect(editor.state.doc.toString()).toBe(`${ONLY}\nВставка`);
    expect(collapsed(editor)).toBe(true);
    undo(editor);
    expect(editor.state.doc.toString()).toBe(ONLY);
  });

  it('нажатия и `beforeinput` редактор не перехватывает: ввод идёт обычным путём браузера', () => {
    const editor = open();
    expect(press(editor, 'П').defaultPrevented).toBe(false);
    const event = new InputEvent('beforeinput', { inputType: 'insertText', data: 'Х', bubbles: true, cancelable: true });
    editor.contentDOM.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(editor.state.doc.toString()).toBe(ONLY);
  });
});
