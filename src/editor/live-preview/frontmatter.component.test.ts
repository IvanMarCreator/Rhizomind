// Блок свойств в живом предпросмотре (0.2-3.2, С2-3.4, решение 112): на месте
// блока — свёрнутая строка «Свойства» с тегами; щелчок или курсор в блоке
// раскрывают исходный текст, уход курсора сворачивает. Сворачивание — только
// оформление: текст заметки не меняется (инвариант 4).
import { EditorView } from '@codemirror/view';
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { createEditorState, setMode } from '../codemirror.ts';

const NOTE = '---\ntags: [проект, "#идея"]\ntitle: Встреча\n---\nТекст заметки\nвторая строка\n';
const BLOCK_END = NOTE.indexOf('---\nТекст') + 3;
const BODY = BLOCK_END + 1;

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

function open(text = NOTE, cursor = text.length, onOpenTag?: (tag: string) => void): EditorView {
  view = new EditorView({
    parent: document.body,
    state: createEditorState(text, { mode: 'live_preview', ...(onOpenTag ? { onOpenTag } : {}) }),
  });
  view.dispatch({ selection: { anchor: cursor } });
  return view;
}

const row = (editor: EditorView) => editor.contentDOM.querySelector<HTMLElement>('.cm-rz-properties');
const rowTags = (editor: EditorView) =>
  [...editor.contentDOM.querySelectorAll('.cm-rz-properties-tag')].map((el) => el.textContent);
const lineTexts = (editor: EditorView) => [...editor.contentDOM.querySelectorAll('.cm-line')].map((el) => el.textContent);

function key(editor: EditorView, init: KeyboardEventInit): void {
  editor.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }));
}

function mousedown(target: Element, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0, ...init });
  target.dispatchEvent(event);
  return event;
}

describe('блок свойств в живом предпросмотре', () => {
  it('блок свёрнут в строку Свойства с тегами', () => {
    const editor = open();
    const properties = row(editor);
    expect(properties).not.toBeNull();
    expect(properties?.querySelector('.cm-rz-properties-label')?.textContent).toBe('Свойства');
    expect(rowTags(editor)).toEqual(['#проект', '#идея']);
    // Исходных строк блока на экране нет, текст заметки — без изменений.
    expect(lineTexts(editor)).toEqual(['Текст заметки', 'вторая строка', '']);
    expect(editor.state.doc.toString()).toBe(NOTE);
  });

  it('блок без тегов — строка «Свойства» без плашек', () => {
    const editor = open('---\ntitle: Встреча\n---\nТекст');
    expect(row(editor)).not.toBeNull();
    expect(rowTags(editor)).toEqual([]);
  });

  it('щелчок раскрывает исходный текст', () => {
    const editor = open();
    const properties = row(editor);
    if (!properties) throw new Error('нет строки «Свойства»');
    const event = mousedown(properties);
    expect(event.defaultPrevented).toBe(true);
    expect(row(editor)).toBeNull();
    expect(lineTexts(editor).slice(0, 4)).toEqual(['---', 'tags: [проект, "#идея"]', 'title: Встреча', '---']);
    // Курсор — в конце первой строки свойств, текст не изменился.
    expect(editor.state.selection.main.head).toBe(NOTE.indexOf('\ntitle'));
    expect(editor.state.doc.toString()).toBe(NOTE);
    expect(editor.contentDOM.querySelectorAll('.cm-rz-properties-source')).toHaveLength(4);
    // Ограничители `---` — бледнее (макет П-4).
    expect([...editor.contentDOM.querySelectorAll('.cm-rz-properties-delimiter')].map((el) => el.textContent)).toEqual(['---', '---']);
  });

  it('щелчок по тегу в строке «Свойства» открывает вид «Теги», блок остаётся свёрнутым', () => {
    const opened = vi.fn();
    const editor = open(NOTE, NOTE.length, opened);
    const chip = editor.contentDOM.querySelectorAll('.cm-rz-properties-tag')[1];
    if (!chip) throw new Error('нет плашки');
    mousedown(chip);
    expect(opened).toHaveBeenCalledWith('идея');
    expect(row(editor)).not.toBeNull();
  });

  it('курсор ушёл — свернулось', () => {
    const editor = open(NOTE, 6);
    expect(row(editor)).toBeNull();
    editor.dispatch({ selection: { anchor: BODY + 3 }, userEvent: 'select.pointer' });
    expect(row(editor)).not.toBeNull();
    expect(editor.state.doc.toString()).toBe(NOTE);
  });

  it('стрелки обходят блок как одну строку', () => {
    const editor = open(NOTE, BODY + 4);
    // ↑ с первой строки под блоком — на строку блока: курсор в конце закрывающей строки, блок раскрыт.
    key(editor, { key: 'ArrowUp' });
    expect(editor.state.selection.main.head).toBe(BLOCK_END);
    expect(row(editor)).toBeNull();
    // Курсор в начало текста при свёрнутом блоке (Ctrl+Home) — в начало текста под блоком: не перед `---`.
    editor.dispatch({ selection: { anchor: BODY + 4 } });
    key(editor, { key: 'Home', ctrlKey: true });
    expect(editor.state.selection.main.head).toBe(BODY);
    expect(row(editor)).not.toBeNull();
    // ← с начала строки под блоком — в конец блока (раскрыт), → — обратно.
    key(editor, { key: 'ArrowLeft' });
    expect(editor.state.selection.main.head).toBe(BLOCK_END);
    expect(row(editor)).toBeNull();
    key(editor, { key: 'ArrowRight' });
    expect(editor.state.selection.main.head).toBe(BODY);
    expect(row(editor)).not.toBeNull();
    expect(editor.state.doc.toString()).toBe(NOTE);
  });

  it('выделение через блок', () => {
    // Редактор без заданного выделения: курсор в начале текста, перед строкой «Свойства».
    const editor = new EditorView({ parent: document.body, state: createEditorState(NOTE, { mode: 'live_preview' }) });
    view = editor;
    expect(editor.state.selection.main.head).toBe(0);
    // Shift+↓ из начала текста выделяет весь блок как одну строку; блок свёрнут.
    key(editor, { key: 'ArrowDown', shiftKey: true });
    expect(editor.state.selection.main.anchor).toBe(0);
    expect(editor.state.selection.main.head).toBe(BODY);
    expect(row(editor)).not.toBeNull();
    // Shift+Ctrl+Home из текста ниже — выделение до начала, блок свёрнут, выделен исходный текст.
    editor.dispatch({ selection: { anchor: NOTE.length } });
    key(editor, { key: 'Home', ctrlKey: true, shiftKey: true });
    const { from, to } = editor.state.selection.main;
    expect(editor.state.sliceDoc(from, to)).toBe(NOTE);
    expect(row(editor)).not.toBeNull();
  });

  it('Ctrl+A и копирование дают исходный текст', () => {
    const editor = open(NOTE, BODY + 2);
    editor.focus();
    key(editor, { key: 'a', ctrlKey: true });
    expect(editor.state.selection.main.from).toBe(0);
    expect(editor.state.selection.main.to).toBe(NOTE.length);
    expect(row(editor)).not.toBeNull();
    let copied = '';
    const copy = new Event('copy', { bubbles: true, cancelable: true });
    Object.defineProperty(copy, 'clipboardData', {
      value: { clearData: () => undefined, setData: (_type: string, text: string) => (copied = text) },
    });
    // Выделение DOM — в редакторе (копирование проверяет, что оно его).
    window.getSelection()?.selectAllChildren(editor.contentDOM);
    (editor as unknown as { observer: { readSelectionRange(): boolean } }).observer.readSelectionRange();
    editor.contentDOM.dispatchEvent(copy);
    expect(copied).toBe(NOTE);
    expect(editor.state.doc.toString()).toBe(NOTE);
  });

  it('в исходном режиме виден целиком', () => {
    view = new EditorView({ parent: document.body, state: createEditorState(NOTE, { mode: 'source' }) });
    expect(row(view)).toBeNull();
    expect(lineTexts(view).slice(0, 4)).toEqual(['---', 'tags: [проект, "#идея"]', 'title: Встреча', '---']);
    expect(view.contentDOM.querySelectorAll('.cm-rz-properties-source')).toHaveLength(0);
  });

  it('переключение режима не меняет текст', () => {
    const editor = open(NOTE, BODY + 2);
    const selection = editor.state.selection;
    setMode(editor, 'source');
    expect(row(editor)).toBeNull();
    expect(lineTexts(editor)[0]).toBe('---');
    setMode(editor, 'live_preview');
    expect(row(editor)).not.toBeNull();
    expect(editor.state.doc.toString()).toBe(NOTE);
    expect(editor.state.selection.eq(selection)).toBe(true);
    // Курсор в блоке при переключении в предпросмотр — блок раскрыт: курсор не прячется в скрытом тексте.
    editor.dispatch({ selection: { anchor: 8 } });
    setMode(editor, 'source');
    setMode(editor, 'live_preview');
    expect(row(editor)).toBeNull();
    expect(editor.state.doc.toString()).toBe(NOTE);
  });

  it('правка, которая убрала закрывающую строку, показывает текст как есть', () => {
    const editor = open(NOTE, BLOCK_END);
    editor.dispatch({ changes: { from: BLOCK_END - 3, to: BLOCK_END, insert: '--' }, userEvent: 'delete' });
    editor.dispatch({ selection: { anchor: editor.state.doc.length } });
    expect(row(editor)).toBeNull();
    expect(lineTexts(editor).slice(1, 4)).toEqual(['tags: [проект, "#идея"]', 'title: Встреча', '--']);
  });
});
