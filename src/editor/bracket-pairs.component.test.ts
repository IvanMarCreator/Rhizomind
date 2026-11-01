// Парная скобка в настоящем редакторе (решение 95, задача 2.18): ввод идёт
// тем же путём, что ввод с клавиатуры в CodeMirror (обработчики
// `EditorView.inputHandler`, затем обычная вставка), Backspace и Enter —
// нажатиями. Проверяются подключение в codemirror.ts, оба режима, список
// подсказок `[[`, продолжение задач по Enter, IME и отмена.
import { EditorView } from '@codemirror/view';
import { fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { EditorMode, LinkSuggestion } from '../api/index.ts';
import { createEditorState } from './codemirror.ts';
import { activeCompletion } from './wikilinks/autocomplete.ts';
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

function open(text: string, mode: EditorMode = 'live_preview', readOnly = false): EditorView {
  view = new EditorView({ parent: document.body, state: createEditorState(text, { mode, wiki: host, readOnly }) });
  view.dispatch({ selection: { anchor: text.length } });
  return view;
}

/**
 * Набрать текст посимвольно, как CodeMirror обрабатывает ввод из DOM:
 * сначала обработчики `inputHandler`, иначе — обычная вставка `input.type`.
 */
function type(target: EditorView, text: string): void {
  for (const ch of text) {
    const { from, to } = target.state.selection.main;
    const insert = () =>
      target.state.update({ changes: { from, to, insert: ch }, selection: { anchor: from + ch.length }, userEvent: 'input.type' });
    const handled = target.state.facet(EditorView.inputHandler).some((handler) => handler(target, from, to, ch, insert));
    if (!handled && !target.state.readOnly) target.dispatch(insert());
  }
}

async function press(target: EditorView, key: string, init: KeyboardEventInit = {}): Promise<void> {
  await fireEvent.keyDown(target.contentDOM, { key, ...init });
}

function marked(target: EditorView): string {
  const doc = target.state.doc.toString();
  const { from, to } = target.state.selection.main;
  return from === to ? `${doc.slice(0, from)}|${doc.slice(from)}` : `${doc.slice(0, from)}|${doc.slice(from, to)}|${doc.slice(to)}`;
}

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.body.replaceChildren();
});

describe('парная скобка в редакторе (оба режима)', () => {
  it.each<EditorMode>(['live_preview', 'source'])('режим %s: `- [` + пробел + текст → задача', (mode) => {
    const editor = open('', mode);
    type(editor, '- [ купить хлеб');
    expect(marked(editor)).toBe('- [ ] купить хлеб|');
  });

  it.each<EditorMode>(['live_preview', 'source'])('режим %s: `[` → `[]`, `]` проскакивает', (mode) => {
    const editor = open('', mode);
    type(editor, '[');
    expect(marked(editor)).toBe('[|]');
    type(editor, 'текст](адрес)');
    expect(marked(editor)).toBe('[текст](адрес)|');
  });

  it('Backspace в пустой паре удаляет обе скобки', async () => {
    const editor = open('а ');
    type(editor, '[');
    await press(editor, 'Backspace');
    expect(marked(editor)).toBe('а |');
  });

  it('`[` при выделении оборачивает его', () => {
    const editor = open('см. заметку');
    editor.dispatch({ selection: { anchor: 4, head: 11 } });
    type(editor, '[');
    expect(marked(editor)).toBe('см. [|заметку|]');
  });

  it('Ctrl+Z отменяет пару одним шагом', async () => {
    const editor = open('текст ');
    type(editor, '[');
    expect(editor.state.doc.toString()).toBe('текст []');
    await press(editor, 'z', { ctrlKey: true });
    expect(editor.state.doc.toString()).toBe('текст ');
  });

  it('в коде в строке — без пары', () => {
    const editor = open('`код `');
    editor.dispatch({ selection: { anchor: 5 } });
    type(editor, '[');
    expect(editor.state.doc.toString()).toBe('`код [`');
  });

  it('во время набора через IME не вмешивается', async () => {
    const editor = open('');
    await fireEvent.compositionStart(editor.contentDOM);
    expect(editor.compositionStarted).toBe(true);
    type(editor, '[');
    expect(editor.state.doc.toString()).toBe('[');
  });

  it('только чтение — текст не меняется', async () => {
    const editor = open('[]', 'live_preview', true);
    editor.dispatch({ selection: { anchor: 1 } });
    type(editor, '[');
    await press(editor, 'Backspace');
    expect(editor.state.doc.toString()).toBe('[]');
  });
});

describe('с подсказками вики-ссылок', () => {
  it('`[[` открывает список; выбор Enter даёт `[[Встреча]]` без лишних скобок', async () => {
    const editor = open('см. ');
    type(editor, '[');
    type(editor, '[');
    expect(marked(editor)).toBe('см. [[|]]');
    expect(activeCompletion(editor.state)).not.toBeNull();
    type(editor, 'Вст');
    await waitFor(() => expect(editor.dom.querySelectorAll('[role="option"]')).toHaveLength(1));
    await press(editor, 'Enter');
    expect(marked(editor)).toBe('см. [[Встреча]]|');
  });

  it('выбор Tab — тоже без лишних скобок', async () => {
    const editor = open('');
    type(editor, '[[Вст');
    await waitFor(() => expect(editor.dom.querySelectorAll('[role="option"]')).toHaveLength(1));
    await press(editor, 'Tab');
    expect(marked(editor)).toBe('[[Встреча]]|');
  });

  it('имя набрано целиком и закрыто `]]` — скобки проскакивают, список закрыт', async () => {
    const editor = open('');
    type(editor, '[[Встреча]]');
    expect(marked(editor)).toBe('[[Встреча]]|');
    expect(activeCompletion(editor.state)).toBeNull();
  });

  it('Backspace в `[[]]` — сначала внутренняя пара, список закрывается', async () => {
    const editor = open('');
    type(editor, '[[');
    await press(editor, 'Backspace');
    expect(marked(editor)).toBe('[|]');
    expect(activeCompletion(editor.state)).toBeNull();
  });
});

describe('с продолжением задач по Enter', () => {
  it('задача, Enter, задача: `- [ ]` продолжается, пустой Enter выходит', async () => {
    const editor = open('');
    type(editor, '- [ первая');
    await press(editor, 'Enter');
    type(editor, 'вторая');
    await press(editor, 'Enter');
    await press(editor, 'Enter');
    type(editor, 'абзац');
    expect(editor.state.doc.toString()).toBe('- [ ] первая\n- [ ] вторая\nабзац');
  });

  it('новый пункт `- ` + `[` + пробел — задача', async () => {
    const editor = open('- раз');
    await press(editor, 'Enter');
    type(editor, '[ два');
    expect(editor.state.doc.toString()).toBe('- раз\n- [ ] два');
  });
});
