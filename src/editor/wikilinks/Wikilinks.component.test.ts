// Вики-ссылки в окне на подмене ядра (С3.4, решение 76): щелчок вне строки с
// курсором и Ctrl+щелчок на ней, Ctrl+щелчок в режиме исходного текста,
// неразрешённая ссылка — вид и создание заметки, переход к заголовку,
// автодополнение. Вики-ссылка никогда не уходит в open_external.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { LinkSuggestion, NoteReadResponse, Settings, TreeEntry, VaultPath } from '../../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../../api/testing.ts';
import Shell from '../../shell/Shell.svelte';
import { WindowModel } from '../../shell/window.svelte.ts';
import { activeCompletion } from './autocomplete.ts';

// В jsdom нет раскладки: у Range нет размеров, а CodeMirror меряет текст,
// когда редактор получает фокус (переход по ссылке, решение 86).
if (typeof Range.prototype.getClientRects !== 'function') {
  const empty = { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, toJSON: () => ({}) };
  Object.assign(Range.prototype, {
    getClientRects: () => Object.assign([], { item: () => null }) as unknown as DOMRectList,
    getBoundingClientRect: () => empty as DOMRect,
  });
}

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
};

const VERSION = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };
const HOME: VaultPath = ['Проекты', 'Дневник.md'];
const MEETING: VaultPath = ['Проекты', 'Альфа', 'Встреча.md'];
const HOME_TEXT = 'См. [[Альфа/Встреча#Итоги|итоги]] и [[Новая]].\n\n[сайт](https://example.org)\n\nконец';
const MEETING_TEXT = '# Встреча\n\nтекст\n\n## Итоги\n\nрешили\n';

interface Setup {
  settings?: Settings;
  extra?: CoreMocks;
}

async function openHome({ settings = SETTINGS, extra = {} }: Setup = {}) {
  const files = new Map<string, string>([
    [HOME.join('/'), HOME_TEXT],
    [MEETING.join('/'), MEETING_TEXT],
  ]);
  const entries: TreeEntry[] = [
    { path: ['Проекты'], kind: 'folder' },
    { path: ['Проекты', 'Альфа'], kind: 'folder' },
    { path: HOME, kind: 'note' },
    { path: MEETING, kind: 'note' },
  ];
  const resolveTarget = (target: string): VaultPath | null => {
    const name = target.replace(/^\//, '').split('/').at(-1) ?? '';
    const hit = entries.find((e) => e.kind === 'note' && e.path.at(-1) === `${name}.md`);
    return hit ? [...hit.path] : null;
  };
  const calls = mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...settings } }),
    settings_write: () => null,
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries }),
    note_read: ({ path }): NoteReadResponse => ({ text: files.get(path.join('/')) ?? '', version: VERSION, read_only: null }),
    note_write: () => ({ outcome: 'saved', version: VERSION }),
    links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: resolveTarget(target) })) }),
    note_create: ({ dir, name }) => {
      const path = [...dir, name];
      entries.push({ path, kind: 'note' });
      files.set(path.join('/'), '');
      return { path };
    },
    open_external: () => ({ outcome: 'cancelled' }),
    ...extra,
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(HOME);
  const view = await editorView();
  return { calls, model, view, entries };
}

async function editorView(): Promise<EditorView> {
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return view;
}

/** Редактор другой заметки (прежний удалён после перехода). */
async function nextView(previous: EditorView): Promise<EditorView> {
  let view = previous;
  await waitFor(async () => {
    view = await editorView();
    expect(view).not.toBe(previous);
  });
  return view;
}

const called = (calls: RecordedCall[], command: string) => calls.filter(([name]) => name === command);
const wikiLink = (view: EditorView, inner: string) =>
  view.contentDOM.querySelector(`[data-rz-wiki="${inner}"]`) as Element;

function cursorToEnd(view: EditorView) {
  view.dispatch({ selection: { anchor: view.state.doc.length } });
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('щелчок по вики-ссылке (С3.4, решение 76)', () => {
  it('вне строки с курсором — обычный щелчок открывает заметку и переходит к заголовку', async () => {
    const { view, model, calls } = await openHome();
    cursorToEnd(view);

    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0 });

    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
    const meeting = await nextView(view);
    await waitFor(() => expect(meeting.state.doc.lineAt(meeting.state.selection.main.head).text).toBe('## Итоги'));
    expect(called(calls, 'open_external')).toEqual([]);
    // Переход — шаг истории: назад возвращает к заметке со ссылкой.
    expect(model.canGoBack).toBe(true);
  });

  it('на строке с курсором обычный щелчок — правка, Ctrl+щелчок — переход', async () => {
    const { view, model } = await openHome();
    view.dispatch({ selection: { anchor: 2 } });

    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(model.currentPath).toEqual(HOME);

    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0, ctrlKey: true });
    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
  });

  it('режим исходного текста: обычный щелчок не открывает, Ctrl+щелчок открывает вики- и Markdown-ссылку', async () => {
    const { view, model, calls } = await openHome({ settings: { ...SETTINGS, editor_mode: 'source' } });
    expect(view.contentDOM.querySelector('[data-rz-wiki]')).toBeNull();
    const at = (text: string) => HOME_TEXT.indexOf(text);
    // В jsdom нет раскладки: позицию под указателем подставляет тест.
    let pos = at('Встреча#');
    view.posAtCoords = (() => pos) as typeof view.posAtCoords;
    const line = view.contentDOM.querySelector('.cm-line') as Element;

    await fireEvent.mouseDown(line, { button: 0 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(model.currentPath).toEqual(HOME);

    pos = at('сайт');
    await fireEvent.mouseDown(line, { button: 0, ctrlKey: true });
    await waitFor(() => expect(called(calls, 'open_external')).toEqual([['open_external', { request: { url: 'https://example.org' } }]]));

    pos = at('Встреча#');
    await fireEvent.mouseDown(line, { button: 0, ctrlKey: true });
    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
    expect(called(calls, 'open_external')).toHaveLength(1);
  });
});

describe('после перехода можно сразу печатать (решение 86)', () => {
  it('переход к заголовку другой заметки: фокус в тексте, курсор на заголовке', async () => {
    const { view } = await openHome();
    cursorToEnd(view);
    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0 });
    const meeting = await nextView(view);
    await waitFor(() => expect(meeting.hasFocus).toBe(true));
    expect(meeting.state.doc.lineAt(meeting.state.selection.main.head).text).toBe('## Итоги');
    expect(meeting.state.selection.main.head).toBe(MEETING_TEXT.indexOf('## Итоги'));
  });

  it('переход без заголовка и созданная по ссылке заметка: фокус в тексте, курсор в начале', async () => {
    const { view, model } = await openHome();
    cursorToEnd(view);
    await fireEvent.mouseDown(wikiLink(view, 'Новая'), { button: 0 });
    await waitFor(() => expect(model.currentPath).toEqual(['Проекты', 'Новая.md']));
    const created = await nextView(view);
    await waitFor(() => expect(created.hasFocus).toBe(true));
    expect(created.state.selection.main.head).toBe(0);
  });

  it('переход к заголовку своей заметки: фокус в тексте, курсор на заголовке', async () => {
    const { view } = await openHome();
    const end = view.state.doc.length;
    view.dispatch({ changes: { from: end, insert: '\n\n# Конец\n\n[[Дневник#Конец]]\n\nпоследняя' } });
    cursorToEnd(view);
    expect(view.hasFocus).toBe(false);
    await fireEvent.mouseDown(wikiLink(view, 'Дневник#Конец'), { button: 0 });
    await waitFor(() => expect(view.hasFocus).toBe(true));
    expect(view.state.doc.lineAt(view.state.selection.main.head).text).toBe('# Конец');
  });
});

describe('решение 86: заметка только для чтения и незавершённый переход', () => {
  it('в заметке только для чтения фокус после перехода не ставится, курсор — на заголовке', async () => {
    const { view } = await openHome({
      extra: {
        note_read: ({ path }): NoteReadResponse =>
          path.join('/') === MEETING.join('/')
            ? { text: MEETING_TEXT, version: VERSION, read_only: { reason: 'too_large', limit_bytes: 1 } }
            : { text: HOME_TEXT, version: VERSION, read_only: null },
      },
    });
    cursorToEnd(view);
    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0 });
    const meeting = await nextView(view);
    await waitFor(() => expect(meeting.state.doc.lineAt(meeting.state.selection.main.head).text).toBe('## Итоги'));
    expect(meeting.state.readOnly).toBe(true);
    expect(meeting.hasFocus).toBe(false);
  });

  it('другое открытие до конца перехода (Ctrl+O) сбрасывает его: позже заметка открывается без перехода', async () => {
    let release: (() => void) | null = null;
    let meetingReads = 0;
    const OTHER: VaultPath = ['Проекты', 'Другая.md'];
    const { view, model } = await openHome({
      extra: {
        note_read: ({ path }): NoteReadResponse | Promise<NoteReadResponse> => {
          const text = path.join('/') === MEETING.join('/') ? MEETING_TEXT : path.join('/') === HOME.join('/') ? HOME_TEXT : '';
          const response = { text, version: VERSION, read_only: null };
          if (path.join('/') === MEETING.join('/') && ++meetingReads === 1) {
            return new Promise((resolve) => (release = () => resolve(response)));
          }
          return response;
        },
      },
    });
    cursorToEnd(view);
    await fireEvent.mouseDown(wikiLink(view, 'Альфа/Встреча#Итоги|итоги'), { button: 0 });
    await waitFor(() => expect(model.currentPath).toEqual(MEETING));
    // Заметка из ссылки ещё читается — пользователь открыл другую.
    model.open(OTHER);
    const other = await nextView(view);
    expect(model.currentPath).toEqual(OTHER);
    (release as (() => void) | null)?.();
    // Позже ту заметку открыли просто так: курсор в начале, фокус не тронут.
    model.open(MEETING);
    const meeting = await nextView(other);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(meeting.state.selection.main.head).toBe(0);
    expect(meeting.hasFocus).toBe(false);
  });
});

describe('неразрешённая ссылка', () => {
  it('выглядит иначе; щелчок создаёт заметку в папке текущей и открывает её; вид обновляется по vault:changed', async () => {
    const { view, model, calls } = await openHome();
    cursorToEnd(view);
    await waitFor(() => expect(wikiLink(view, 'Новая').classList.contains('cm-rz-wikilink-unresolved')).toBe(true));
    expect(wikiLink(view, 'Альфа/Встреча#Итоги|итоги').classList.contains('cm-rz-wikilink-unresolved')).toBe(false);

    await fireEvent.mouseDown(wikiLink(view, 'Новая'), { button: 0 });

    await waitFor(() => expect(model.currentPath).toEqual(['Проекты', 'Новая.md']));
    expect(called(calls, 'note_create')).toEqual([['note_create', { request: { dir: ['Проекты'], name: 'Новая.md' } }]]);

    const created = await editorView();
    model.back();
    const home = await nextView(created);
    cursorToEnd(home);
    await emitCoreEvent('vault:changed', { changes: [], rescan: true });
    await waitFor(() => expect(wikiLink(home, 'Новая').classList.contains('cm-rz-wikilink-unresolved')).toBe(false));
  });

  it('ошибка ядра при создании — уведомление, заметка не меняется', async () => {
    const { view, model } = await openHome({
      extra: { note_create: () => Promise.reject({ code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: 'reserved_name' }) },
    });
    cursorToEnd(view);
    await fireEvent.mouseDown(wikiLink(view, 'Новая'), { button: 0 });
    await screen.findByText(/Не удалось открыть ссылку/);
    expect(model.currentPath).toEqual(HOME);
  });
});

describe('автодополнение после [[', () => {
  const ITEMS: LinkSuggestion[] = [
    { insert: 'Альфа/Встреча', name: 'Встреча', path: MEETING, linkable: true },
    { insert: '/Встреча', name: 'Встреча', path: ['Встреча.md'], linkable: true },
    { insert: null, name: '#Встреча 2', path: ['#Встреча 2.md'], linkable: false },
  ];

  async function typing(text: string) {
    const setup = await openHome({ extra: { links_suggest: ({ query }) => ({ items: query.length > 0 ? ITEMS : [] }) } });
    const { view } = setup;
    const at = view.state.doc.length;
    view.dispatch({ changes: { from: at, insert: `\n${text}` }, selection: { anchor: at + text.length + 1 }, userEvent: 'input.type' });
    await waitFor(() => expect(view.dom.querySelectorAll('[role="option"]')).toHaveLength(3));
    return setup;
  }

  const lastLine = (view: EditorView) => view.state.doc.line(view.state.doc.lines).text;

  it('список заметок по части имени (команда links_suggest); Enter вставляет кратчайший путь и ]]', async () => {
    const { view, calls } = await typing('встреча с [[встр');
    expect(called(calls, 'links_suggest').at(-1)).toEqual(['links_suggest', { request: { from: HOME, query: 'встр' } }]);
    expect(screen.getByRole('listbox', { name: 'Заметки для ссылки' })).toBeTruthy();
    const options = screen.getAllByRole('option');
    expect(options[0]?.getAttribute('aria-selected')).toBe('true');
    expect(options[2]?.getAttribute('aria-disabled')).toBe('true');
    expect(options[2]?.textContent).toContain('нельзя сослаться');

    await fireEvent.keyDown(view.contentDOM, { key: 'Enter' });

    expect(lastLine(view)).toBe('встреча с [[Альфа/Встреча]]');
    expect(view.state.selection.main.head).toBe(view.state.doc.length);
    expect(activeCompletion(view.state)).toBeNull();
  });

  it('↓ выбирает следующую; Tab вставляет; закрывающие ]] не удваиваются', async () => {
    const { view } = await typing('[[вс');
    view.dispatch({ changes: { from: view.state.doc.length, insert: ']]' } });
    await fireEvent.keyDown(view.contentDOM, { key: 'ArrowDown' });
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true');
    await fireEvent.keyDown(view.contentDOM, { key: 'Tab' });
    expect(lastLine(view)).toBe('[[/Встреча]]');
  });

  it('заметку, на которую нельзя сослаться, не вставить; Esc закрывает список', async () => {
    const { view } = await typing('[[вс');
    await fireEvent.mouseDown(screen.getAllByRole('option')[2] as Element);
    expect(lastLine(view)).toBe('[[вс');
    await fireEvent.keyDown(view.contentDOM, { key: 'Escape' });
    expect(activeCompletion(view.state)).toBeNull();
    expect(view.dom.querySelector('[role="listbox"]')).toBeNull();
  });

  /** Строка с курсором `¦` в середине набираемого имени. */
  async function typingAt(marked: string) {
    const pos = marked.indexOf('¦');
    const setup = await openHome({ extra: { links_suggest: ({ query }) => ({ items: query.length > 0 ? ITEMS : [] }) } });
    const { view } = setup;
    const at = view.state.doc.length;
    view.dispatch({
      changes: { from: at, insert: `\n${marked.slice(0, pos)}${marked.slice(pos + 1)}` },
      selection: { anchor: at + 1 + pos },
      userEvent: 'input.type',
    });
    await waitFor(() => expect(view.dom.querySelectorAll('[role="option"]')).toHaveLength(3));
    return setup;
  }

  // Курсор посреди имени: заменяется вся цель до `]]`, `]`, `|`, `#`; без
  // закрывающих скобок — слово после курсора. Скобки не удваиваются.
  it.each([
    ['[[Пл¦ан]]', '[[Альфа/Встреча]]', '[[Альфа/Встреча]]'],
    ['[[Пл¦]', '[[Альфа/Встреча]]', '[[Альфа/Встреча]]'],
    ['[[Пл¦ан', '[[Альфа/Встреча]]', '[[Альфа/Встреча]]'],
    ['[[Пл¦ан потом', '[[Альфа/Встреча]] потом', '[[Альфа/Встреча]]'],
    ['[[Пл¦ан дальше]] и [[Б]]', '[[Альфа/Встреча]] и [[Б]]', '[[Альфа/Встреча]]'],
    ['[[Пл¦ан|подпись]]', '[[Альфа/Встреча|подпись]]', '[[Альфа/Встреча'],
    ['[[Пл¦ан#Итоги]]', '[[Альфа/Встреча#Итоги]]', '[[Альфа/Встреча'],
  ])('курсор посреди имени: %s → %s', async (marked, expected, beforeCursor) => {
    const { view } = await typingAt(marked);
    await fireEvent.keyDown(view.contentDOM, { key: 'Enter' });
    expect(lastLine(view)).toBe(expected);
    const line = view.state.doc.line(view.state.doc.lines);
    expect(view.state.sliceDoc(line.from, view.state.selection.main.head)).toBe(beforeCursor);
  });

  it('щелчок по пункту вставляет его', async () => {
    const { view } = await typing('[[вс');
    await fireEvent.mouseDown(screen.getAllByRole('option')[1] as Element);
    expect(lastLine(view)).toBe('[[/Встреча]]');
  });
});
