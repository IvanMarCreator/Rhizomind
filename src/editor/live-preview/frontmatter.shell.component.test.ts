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

async function openShell(first: VaultPath, settings: Settings = SETTINGS) {
  const files = new Map<string, string>([
    [HOME.join('/'), HOME_TEXT],
    [MEETING.join('/'), MEETING_TEXT],
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
    note_read: ({ path }): NoteReadResponse => ({ text: files.get(path.join('/')) ?? '', version: VERSION, read_only: null }),
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
