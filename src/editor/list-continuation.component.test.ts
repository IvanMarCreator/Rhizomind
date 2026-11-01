// Продолжение списков в настоящем редакторе (решение 92, задача 2.15):
// нажатия клавиш, а не вызов функции — проверяется подключение в
// codemirror.ts, порядок сочетаний (подсказки `[[` важнее), IME и отмена.
import { EditorView } from '@codemirror/view';
import { fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { EditorMode, LinkSuggestion } from '../api/index.ts';
import { createEditorState, setMode } from './codemirror.ts';
import type { WikiHost } from './wikilinks/host.ts';

if (typeof Range.prototype.getClientRects !== 'function') {
  const empty = { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, toJSON: () => ({}) };
  Object.assign(Range.prototype, {
    getClientRects: () => Object.assign([], { item: () => null }) as unknown as DOMRectList,
    getBoundingClientRect: () => empty as DOMRect,
  });
}

const SUGGESTIONS: LinkSuggestion[] = [{ insert: 'Встреча', name: 'Встреча', path: ['Встреча.md'], linkable: true }];

const host: WikiHost = {
  resolve: () => Promise.resolve(new Map()),
  open: () => undefined,
  suggest: () => Promise.resolve(SUGGESTIONS),
  subscribe: () => () => undefined,
};

let view: EditorView | undefined;

function open(text: string, mode: EditorMode = 'live_preview'): EditorView {
  const changes: string[] = [];
  view = new EditorView({
    parent: document.body,
    state: createEditorState(text, { mode, wiki: host, onChange: (t) => changes.push(t) }),
  });
  view.dispatch({ selection: { anchor: text.length } });
  return view;
}

async function press(target: EditorView, key: string, init: KeyboardEventInit = {}): Promise<void> {
  await fireEvent.keyDown(target.contentDOM, { key, ...init });
}

function type(target: EditorView, text: string): void {
  const at = target.state.selection.main.head;
  target.dispatch({ changes: { from: at, insert: text }, selection: { anchor: at + text.length }, userEvent: 'input.type' });
}

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.body.replaceChildren();
});

describe('Enter в редакторе продолжает список (оба режима)', () => {
  it.each<EditorMode>(['live_preview', 'source'])('режим %s: три пункта и выход пустым Enter', async (mode) => {
    const editor = open('- [ ] ', mode);
    type(editor, 'первый');
    await press(editor, 'Enter');
    type(editor, 'второй');
    await press(editor, 'Enter');
    type(editor, 'третий');
    await press(editor, 'Enter');
    await press(editor, 'Enter');
    type(editor, 'абзац');
    expect(editor.state.doc.toString()).toBe('- [ ] первый\n- [ ] второй\n- [ ] третий\nабзац');
  });

  it('после смены режима продолжение работает', async () => {
    const editor = open('1. раз');
    setMode(editor, 'source');
    await press(editor, 'Enter');
    expect(editor.state.doc.toString()).toBe('1. раз\n2. ');
  });

  it('Ctrl+Z отменяет продолжение одним шагом', async () => {
    const editor = open('> - мысль');
    await press(editor, 'Enter');
    expect(editor.state.doc.toString()).toBe('> - мысль\n> - ');
    await press(editor, 'z', { ctrlKey: true });
    expect(editor.state.doc.toString()).toBe('> - мысль');
  });

  it('вне списка Enter обычный', async () => {
    const editor = open('текст');
    await press(editor, 'Enter');
    expect(editor.state.doc.toString()).toBe('текст\n');
  });

  it('во время набора через IME не вмешивается', async () => {
    const editor = open('- а');
    await fireEvent.compositionStart(editor.contentDOM);
    expect(editor.compositionStarted).toBe(true);
    await press(editor, 'Enter', { isComposing: true, keyCode: 229 });
    // Маркер не добавлен: нажатие осталось обычной обработке.
    expect(editor.state.doc.toString()).not.toContain('\n- ');
  });

  it('только чтение — текст не меняется', async () => {
    view = new EditorView({ parent: document.body, state: createEditorState('- а', { readOnly: true }) });
    view.dispatch({ selection: { anchor: 3 } });
    await press(view, 'Enter');
    await press(view, 'Tab');
    expect(view.state.doc.toString()).toBe('- а');
  });
});

describe('подсказки вики-ссылок важнее', () => {
  it('Enter при открытом списке `[[` выбирает подсказку, а не продолжает список', async () => {
    const editor = open('- ');
    type(editor, 'см. [[Вст');
    await waitFor(() => expect(editor.dom.querySelectorAll('[role="option"]')).toHaveLength(1));
    await press(editor, 'Enter');
    expect(editor.state.doc.toString()).toBe('- см. [[Встреча]]');
    await press(editor, 'Enter');
    expect(editor.state.doc.toString()).toBe('- см. [[Встреча]]\n- ');
  });
});

describe('Tab и Shift+Tab', () => {
  it('в пункте меняют вложенность и не уводят фокус', async () => {
    const editor = open('- а\n- б');
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    editor.contentDOM.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(editor.state.doc.toString()).toBe('- а\n\t- б');
    await press(editor, 'Tab', { shiftKey: true });
    expect(editor.state.doc.toString()).toBe('- а\n- б');
  });

  it('во время набора через IME Tab не меняет вложенность', async () => {
    const editor = open('- а\n- б');
    await fireEvent.compositionStart(editor.contentDOM);
    expect(editor.compositionStarted).toBe(true);
    await press(editor, 'Tab', { isComposing: true, keyCode: 229 });
    await press(editor, 'Tab', { shiftKey: true, isComposing: true, keyCode: 229 });
    expect(editor.state.doc.toString()).toBe('- а\n- б');
  });

  it('в пункте Esc, затем Tab — фокус уходит дальше (выход из текста с клавиатуры)', async () => {
    const editor = open('- а');
    await press(editor, 'Escape', { keyCode: 27 });
    const tab = new KeyboardEvent('keydown', { key: 'Tab', keyCode: 9, bubbles: true, cancelable: true });
    editor.contentDOM.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(false);
    expect(editor.state.doc.toString()).toBe('- а');
  });

  it('вне списка Tab не перехватывается: фокус уходит дальше, как прежде', () => {
    const editor = open('текст');
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    editor.contentDOM.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(false);
    expect(editor.state.doc.toString()).toBe('текст');
  });
});
