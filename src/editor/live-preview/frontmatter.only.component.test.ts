// Заметка только из блока свойств без перевода строки в конце (О-1, отчёт QA
// 0.2-8.3): свёрнутый блок — весь текст, строк для курсора в окне нет, и
// браузеру некуда вставить набранное — ввод пропадал (WebKitGTK, WebView2).
// Ввод с клавиатуры и `beforeinput` (экранная клавиатура, вставка без
// клавиши) редактор берёт сам: перевод строки и набранное — одной правкой.
// В jsdom у нажатия и `beforeinput` нет действия по умолчанию, как и у
// браузера без места для ввода: без исправления текст не меняется.
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

describe('О-1: заметка только из блока свойств без перевода строки', () => {
  it('ввод с клавиатуры — с новой строки под блоком, блок цел', () => {
    const editor = open();
    expect(editor.contentDOM.querySelectorAll('.cm-line')).toHaveLength(0);
    expect(press(editor, 'П').defaultPrevented).toBe(true);
    expect(editor.state.doc.toString()).toBe(`${ONLY}\nП`);
    expect(editor.state.selection.main.head).toBe(editor.state.doc.length);
    expect(collapsed(editor)).toBe(true);
    // Набранное — на экране, в строке под свёрнутым блоком, а не в скрытом участке.
    expect([...editor.contentDOM.querySelectorAll('.cm-line')].map((line) => line.textContent)).toEqual(['П']);
    // Дальше под блоком есть строка — ввод идёт обычным путём браузера.
    expect(press(editor, 'р').defaultPrevented).toBe(false);
    // Отмена возвращает файл к исходному виду.
    undo(editor);
    expect(editor.state.doc.toString()).toBe(ONLY);
  });

  it('вставка в конец блока (правка) — строка под блоком видна', () => {
    const editor = open();
    editor.dispatch({ changes: { from: ONLY.length, insert: '\nтекст' }, selection: { anchor: ONLY.length + 6 }, filter: false });
    expect([...editor.contentDOM.querySelectorAll('.cm-line')].map((line) => line.textContent)).toEqual(['текст']);
    expect(collapsed(editor)).toBe(true);
  });

  it('ввод из `beforeinput` без нажатия клавиши (экранная клавиатура) — с новой строки', () => {
    const editor = open();
    const event = new InputEvent('beforeinput', { inputType: 'insertText', data: 'Х', bubbles: true, cancelable: true });
    editor.contentDOM.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(editor.state.doc.toString()).toBe(`${ONLY}\nХ`);
  });

  it('курсор в начале текста — то же', () => {
    const editor = open(ONLY, 0);
    press(editor, 'Ж');
    expect(editor.state.doc.toString()).toBe(`${ONLY}\nЖ`);
  });

  it('сочетания и служебные клавиши не перехватываются', () => {
    const editor = open();
    for (const key of ['Shift', 'Dead', 'Process', 'F5']) expect(press(editor, key).defaultPrevented, key).toBe(false);
    // Сочетание с Ctrl (Ctrl+B не назначено) — не ввод: текст не меняется.
    press(editor, 'b', { ctrlKey: true });
    press(editor, 'b', { metaKey: true });
    expect(editor.state.doc.toString()).toBe(ONLY);
  });

  it('блок с текстом под ним и раскрытый блок — нажатие не перехватывается', () => {
    const below = open(`${ONLY}\nтекст`);
    expect(press(below, 'я').defaultPrevented).toBe(false);
    below.destroy();
    const expanded = open(ONLY, 6);
    expect(collapsed(expanded)).toBe(false);
    expect(press(expanded, 'я').defaultPrevented).toBe(false);
  });
});
