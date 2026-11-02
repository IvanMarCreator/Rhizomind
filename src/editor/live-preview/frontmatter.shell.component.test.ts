// Блок свойств и ввод сразу после открытия заметки (0.2-3.2, замечание QA,
// инвариант 4): при открытии и переходе по `[[ссылке]]` без заголовка курсор
// встаёт в начало текста под блоком свойств, а не перед `---`; ввод и вставка
// в начало текста при свёрнутом блоке уходят под блок. Набранное не
// разрушает блок, отмена — одной правкой. Окно целиком на подмене ядра.
import { undo } from '@codemirror/commands';
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import type { NoteReadResponse, Settings, TreeEntry, VaultPath } from '../../api/index.ts';
import { mockCore } from '../../api/testing.ts';
import Shell from '../../shell/Shell.svelte';
import { WindowModel } from '../../shell/window.svelte.ts';
import { createEditorState, setMode } from '../codemirror.ts';

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
  cleanup();
  clearMocks();
  document.body.innerHTML = '';
});

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
  rename_links: 'ask',
};

const VERSION = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };
const HOME: VaultPath = ['Дневник.md'];
const MEETING: VaultPath = ['Встреча.md'];
const HOME_TEXT = 'См. [[Встреча]].\n';
const BLOCK = '---\ntags: [проект]\ntitle: Встреча\n---\n';
const MEETING_TEXT = `${BLOCK}Текст встречи\n`;

async function openShell(first: VaultPath, settings: Settings = SETTINGS, meetingText = MEETING_TEXT) {
  const files = new Map<string, string>([
    [HOME.join('/'), HOME_TEXT],
    [MEETING.join('/'), meetingText],
  ]);
  const entries: TreeEntry[] = [
    { path: HOME, kind: 'note' },
    { path: MEETING, kind: 'note' },
  ];
  mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...settings } }),
    settings_write: () => null,
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries }),
    // Ядро при чтении приводит `\r\n` к `\n` (как в Editor.component.test.ts).
    note_read: ({ path }): NoteReadResponse => ({
      text: (files.get(path.join('/')) ?? '').replaceAll('\r\n', '\n'),
      version: VERSION,
      read_only: null,
    }),
    note_write: () => ({ outcome: 'saved', version: VERSION }),
    links_resolve: ({ targets }) => ({
      results: targets.map((target) => ({ target, path: target === 'Встреча' ? MEETING : null })),
    }),
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(first);
  return { model, view: await editorView() };
}

async function editorView(previous?: EditorView): Promise<EditorView> {
  let found: EditorView | null = null;
  await waitFor(async () => {
    const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
    found = EditorView.findFromDOM(content);
    expect(found).not.toBeNull();
    expect(found).not.toBe(previous);
  });
  return found as unknown as EditorView;
}

/** Набор с клавиатуры в позиции курсора. */
function type(view: EditorView, text: string): void {
  for (const ch of text) view.dispatch(view.state.replaceSelection(ch), { userEvent: 'input.type' });
}

const collapsed = (view: EditorView) => view.contentDOM.querySelector('.cm-rz-properties') !== null;

describe('ввод сразу после открытия: блок свойств цел', () => {
  it('открытие заметки и ввод — блок цел', async () => {
    const { view } = await openShell(MEETING);
    expect(view.state.selection.main.head).toBe(BLOCK.length);
    expect(collapsed(view)).toBe(true);
    type(view, 'Привет');
    expect(view.state.doc.toString()).toBe(`${BLOCK}ПриветТекст встречи\n`);
    expect(collapsed(view)).toBe(true);
  });

  it('открытие в режиме исходного текста — курсор тоже под блоком', async () => {
    const { view } = await openShell(MEETING, { ...SETTINGS, editor_mode: 'source' });
    expect(view.state.selection.main.head).toBe(BLOCK.length);
  });

  it('переход по ссылке и ввод — блок цел', async () => {
    const { view, model } = await openShell(HOME);
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    const link = await waitFor(() => {
      const element = view.contentDOM.querySelector('[data-rz-wiki="Встреча"]');
      if (!element) throw new Error('нет ссылки');
      return element;
    });
    await fireEvent.mouseDown(link, { button: 0 });
    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
    const meeting = await editorView(view);
    await waitFor(() => expect(meeting.hasFocus).toBe(true));
    expect(meeting.state.selection.main.head).toBe(BLOCK.length);
    type(meeting, 'Привет');
    expect(meeting.state.doc.toString()).toBe(`${BLOCK}ПриветТекст встречи\n`);
  });

  it('русский ввод с составлением (IME) сразу после открытия — блок цел', async () => {
    // Составление в начале строки jsdom не воспроизводит (узел пересоздаётся и
    // без блока свойств), поэтому шаги составления — правками с меткой
    // `input.type.compose`, как их передаёт редактор.
    const { view } = await openShell(MEETING);
    const at = view.state.selection.main.head;
    expect(at).toBe(BLOCK.length);
    let previous = '';
    for (const step of ['п', 'пр', 'при', 'привет']) {
      view.dispatch({
        changes: { from: at, to: at + previous.length, insert: step },
        selection: { anchor: at + step.length },
        userEvent: 'input.type.compose',
      });
      previous = step;
      expect(collapsed(view)).toBe(true);
    }
    expect(view.state.doc.toString()).toBe(`${BLOCK}приветТекст встречи\n`);
  });

  it('переключение в предпросмотр с курсором в начале текста — курсор под блоком', async () => {
    const { view } = await openShell(MEETING, { ...SETTINGS, editor_mode: 'source' });
    view.dispatch({ selection: { anchor: 0 } });
    setMode(view, 'live_preview');
    await waitFor(() => expect(view.state.selection.main.head).toBe(BLOCK.length));
    expect(collapsed(view)).toBe(true);
    type(view, 'Привет');
    expect(view.state.doc.toString()).toBe(`${BLOCK}ПриветТекст встречи\n`);
  });
});

describe('ввод в начало текста при свёрнутом блоке', () => {
  let view: EditorView | null = null;
  afterEach(() => {
    view?.destroy();
    view = null;
  });

  /** Редактор с курсором в начале текста (выделение не задано). */
  function bare(): EditorView {
    view = new EditorView({ parent: document.body, state: createEditorState(MEETING_TEXT, { mode: 'live_preview' }) });
    return view;
  }

  it('вставка в позицию 0 при свёрнутом блоке — блок цел', () => {
    const editor = bare();
    expect(editor.state.selection.main.head).toBe(0);
    expect(collapsed(editor)).toBe(true);
    editor.dispatch({ changes: { from: 0, insert: 'Вставка' }, selection: { anchor: 7 }, userEvent: 'input.paste' });
    expect(editor.state.doc.toString()).toBe(`${BLOCK}ВставкаТекст встречи\n`);
    expect(editor.state.selection.main.head).toBe(BLOCK.length + 'Вставка'.length);
    expect(collapsed(editor)).toBe(true);
    // Отмена — одной правкой.
    undo(editor);
    expect(editor.state.doc.toString()).toBe(MEETING_TEXT);
  });

  it('набор в позиции 0 при свёрнутом блоке — под блоком, отмена одной правкой', () => {
    const editor = bare();
    editor.dispatch(editor.state.replaceSelection('Х'), { userEvent: 'input.type' });
    expect(editor.state.doc.toString()).toBe(`${BLOCK}ХТекст встречи\n`);
    undo(editor);
    expect(editor.state.doc.toString()).toBe(MEETING_TEXT);
  });

  it('курсор, поставленный в начало текста при свёрнутом блоке, переходит под блок', () => {
    const editor = bare();
    editor.dispatch({ selection: { anchor: editor.state.doc.length } });
    editor.dispatch({ selection: { anchor: 0 }, userEvent: 'select' });
    expect(editor.state.selection.main.head).toBe(BLOCK.length);
    expect(collapsed(editor)).toBe(true);
  });

  it('Home в первой строке раскрытого блока — блок остаётся раскрытым, курсор в начале', () => {
    const editor = bare();
    editor.dispatch({ selection: { anchor: 2 }, userEvent: 'select' });
    expect(collapsed(editor)).toBe(false);
    editor.dispatch({ selection: { anchor: 0 }, userEvent: 'select' });
    expect(editor.state.selection.main.head).toBe(0);
    expect(collapsed(editor)).toBe(false);
  });
});

describe('заметка только из блока свойств, без перевода строки в конце', () => {
  const ONLY = '---\ntags: [проект]\n---';

  it('открытие заметки и ввод — блок цел, набранное с новой строки', async () => {
    const { view } = await openShell(MEETING, SETTINGS, ONLY);
    expect(collapsed(view)).toBe(true);
    type(view, 'Привет');
    expect(view.state.doc.toString()).toBe(`${ONLY}\nПривет`);
    expect(collapsed(view)).toBe(true);
    // Набор подряд — одна группа отмены вместе с новой строкой: текст файла возвращается к прежнему.
    undo(view);
    expect(view.state.doc.toString()).toBe(ONLY);
  });

  it('переход по ссылке и ввод — блок цел', async () => {
    const { view, model } = await openShell(HOME, SETTINGS, ONLY);
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    const link = await waitFor(() => {
      const element = view.contentDOM.querySelector('[data-rz-wiki="Встреча"]');
      if (!element) throw new Error('нет ссылки');
      return element;
    });
    await fireEvent.mouseDown(link, { button: 0 });
    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
    const meeting = await editorView(view);
    await waitFor(() => expect(meeting.hasFocus).toBe(true));
    expect(collapsed(meeting)).toBe(true);
    type(meeting, 'Привет');
    expect(meeting.state.doc.toString()).toBe(`${ONLY}\nПривет`);
  });

  it('файл с переводами CRLF: открытие и ввод — блок цел', async () => {
    const { view } = await openShell(MEETING, SETTINGS, '---\r\ntags: x\r\n---');
    expect(collapsed(view)).toBe(true);
    type(view, 'Привет');
    expect(view.state.doc.toString()).toBe('---\ntags: x\n---\nПривет');
  });

  it('вставка в позицию 0 и в конец блока — с новой строки, отмена одной правкой', () => {
    for (const at of [0, ONLY.length]) {
      const editor = new EditorView({ parent: document.body, state: createEditorState(ONLY, { mode: 'live_preview' }) });
      editor.dispatch({ selection: { anchor: at } });
      expect(collapsed(editor)).toBe(true);
      editor.dispatch({ changes: { from: at, insert: 'Вставка' }, selection: { anchor: at + 7 }, userEvent: 'input.paste' });
      expect(editor.state.doc.toString(), `позиция ${at}`).toBe(`${ONLY}\nВставка`);
      expect(editor.state.selection.main.head).toBe(editor.state.doc.length);
      expect(collapsed(editor)).toBe(true);
      undo(editor);
      expect(editor.state.doc.toString()).toBe(ONLY);
      editor.destroy();
    }
  });
});

describe('отмена и Backspace у свёрнутого блока', () => {
  let view: EditorView | null = null;
  afterEach(() => {
    view?.destroy();
    view = null;
  });

  it('после отмены курсор не остаётся в начале текста при свёрнутом блоке', async () => {
    view = new EditorView({ parent: document.body, state: createEditorState(MEETING_TEXT, { mode: 'live_preview' }) });
    await new Promise((resolve) => setTimeout(resolve, 0));
    // Курсор в 0 в обход фильтра правок (как его восстанавливает отмена), затем ввод — он уходит под блок.
    view.dispatch({ selection: { anchor: 0 }, filter: false });
    view.dispatch({ changes: { from: 0, insert: 'Х' }, userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe(`${BLOCK}ХТекст встречи\n`);
    // Отмена идёт мимо фильтра и возвращает выделение, бывшее до ввода, — 0.
    undo(view);
    expect(view.state.doc.toString()).toBe(MEETING_TEXT);
    await waitFor(() => expect(view?.state.selection.main.head).toBe(BLOCK.length));
    expect(collapsed(view)).toBe(true);
  });

  it('Backspace в начале строки под свёрнутым блоком раскрывает блок, ничего не удаляя', () => {
    view = new EditorView({ parent: document.body, state: createEditorState(MEETING_TEXT, { mode: 'live_preview' }) });
    view.dispatch({ selection: { anchor: BLOCK.length } });
    expect(collapsed(view)).toBe(true);
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(MEETING_TEXT);
    expect(view.state.selection.main.head).toBe(BLOCK.length - 1);
    expect(collapsed(view)).toBe(false);
  });
});

describe('заметка только из блока свойств: составление, пустой блок, Backspace', () => {
  const ONLY = '---\ntags: [проект]\n---';
  let view: EditorView | null = null;
  afterEach(() => {
    view?.destroy();
    view = null;
  });

  function make(text: string, cursor = text.length): EditorView {
    view = new EditorView({ parent: document.body, state: createEditorState(text, { mode: 'live_preview' }) });
    view.dispatch({ selection: { anchor: cursor } });
    return view;
  }

  /** Составление: событие `compositionstart`, затем шаги правками с меткой `input.type.compose`. */
  function compose(editor: EditorView, steps: readonly string[]): void {
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionstart', { data: '', bubbles: true }));
    const at = editor.state.selection.main.head;
    let previous = '';
    for (const step of steps) {
      editor.dispatch({
        changes: { from: at, to: at + previous.length, insert: step },
        selection: { anchor: at + step.length },
        userEvent: 'input.type.compose',
      });
      previous = step;
    }
    editor.contentDOM.dispatchEvent(new CompositionEvent('compositionend', { data: previous, bubbles: true }));
  }

  it('составление (IME) в конце блока без текста под ним — с новой строки, блок цел', () => {
    const editor = make(ONLY);
    expect(collapsed(editor)).toBe(true);
    compose(editor, ['п', 'пр', 'привет']);
    expect(editor.state.doc.toString()).toBe(`${ONLY}\nпривет`);
    expect(collapsed(editor)).toBe(true);
    // Отмена возвращает файл к исходному виду (набранное и новая строка).
    let undos = 0;
    while (editor.state.doc.toString() !== ONLY && undos < 5) {
      undo(editor);
      undos++;
    }
    expect(editor.state.doc.toString()).toBe(ONLY);
    expect(undos).toBeLessThanOrEqual(2);
  });

  it('пустой блок `---\\n---`: щелчок ставит курсор на новую пустую строку между `---`, ввод — внутри блока', () => {
    const editor = make('---\n---');
    const row = editor.contentDOM.querySelector('.cm-rz-properties');
    if (!row) throw new Error('нет строки «Свойства»');
    row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
    expect(editor.state.doc.toString()).toBe('---\n\n---');
    expect(editor.state.selection.main.head).toBe(4);
    expect(collapsed(editor)).toBe(false);
    type(editor, 'a: 1');
    expect(editor.state.doc.toString()).toBe('---\na: 1\n---');
    undo(editor);
    undo(editor);
    expect(editor.state.doc.toString()).toBe('---\n---');
  });

  it('Backspace в конце свёрнутого блока без текста под ним раскрывает блок, ничего не удаляя', () => {
    const editor = make(ONLY);
    expect(collapsed(editor)).toBe(true);
    editor.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }));
    expect(editor.state.doc.toString()).toBe(ONLY);
    expect(editor.state.selection.main.head).toBe(ONLY.length);
    expect(collapsed(editor)).toBe(false);
  });
});
