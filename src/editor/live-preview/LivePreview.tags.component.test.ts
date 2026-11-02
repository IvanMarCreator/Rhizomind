// Теги в редакторе (0.2-3.1, С2-3.3 «В редакторе»): плашка в живом
// предпросмотре, щелчок открывает вид «Теги» с раскрытым тегом, в исходном
// тексте — Ctrl+щелчок. Окно целиком на подмене ядра.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import type { NoteReadResponse, Settings } from '../../api/index.ts';
import { indexMocks, mockCore, type RecordedCall } from '../../api/testing.ts';
import Shell from '../../shell/Shell.svelte';
import { WindowModel } from '../../shell/window.svelte.ts';

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

const NOTE = 'Встреча по #Проект и #работа/отчёт.\n\n`#код` и [[Заметка#Заголовок]] и [т](адрес#якорь)\n\nконец\n';
const VERSION = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };

async function openNote(settings: Settings = SETTINGS) {
  const calls = mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...settings } }),
    settings_write: () => null,
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [] }),
    note_read: (): NoteReadResponse => ({ text: NOTE, version: VERSION, read_only: null }),
    note_write: () => ({ outcome: 'saved', version: VERSION }),
    links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: ['Заметка.md'] })) }),
    ...indexMocks({
      tags: [
        { tag: 'проект', key: 'проект', count: 2 },
        { tag: 'работа/отчёт', key: 'работа/отчёт', count: 1 },
      ],
      tagNotes: { проект: [['Заметка.md'], ['Другая.md']] },
    }),
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(['Заметка.md']);
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return { calls, view };
}

const called = (calls: RecordedCall[], command: string) => calls.filter(([name]) => name === command);

/** Вид «Теги» выбран в правой панели. */
function tagsViewShown(): boolean {
  return screen.queryByRole('tab', { name: 'Теги' })?.getAttribute('aria-selected') === 'true';
}

function plaques(view: EditorView): HTMLElement[] {
  return [...view.contentDOM.querySelectorAll<HTMLElement>('.cm-rz-tag')];
}

afterEach(() => {
  cleanup();
  clearMocks();
});

beforeAll(async () => {
  await openNote();
  cleanup();
  clearMocks();
}, 60_000);

describe('теги в живом предпросмотре', () => {
  it('тег плашкой, `#` виден', async () => {
    const { view } = await openNote();
    expect(plaques(view).map((el) => el.textContent)).toEqual(['#Проект', '#работа/отчёт']);
    expect(plaques(view).map((el) => el.dataset.rzTag)).toEqual(['Проект', 'работа/отчёт']);
    // `#` — часть плашки и текста строки: ничего не скрыто.
    expect(view.contentDOM.textContent).toContain('Встреча по #Проект и #работа/отчёт.');
  });

  it('тег в коде и в ссылке — не плашка', async () => {
    const { view } = await openNote();
    const line = view.contentDOM.querySelectorAll('.cm-line')[2] as HTMLElement;
    expect(line.textContent).toContain('#код');
    expect(line.querySelector('.cm-rz-tag')).toBeNull();
  });

  it('щелчок открывает вид Теги с раскрытым тегом', async () => {
    const { view, calls } = await openNote();
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    await fireEvent.mouseDown(plaques(view)[0] as Element, { button: 0 });

    await waitFor(() => expect(tagsViewShown()).toBe(true));
    const row = await within(screen.getByRole('complementary', { name: 'Правая панель' })).findByRole('button', { name: /^#проект/ });
    expect(row.getAttribute('aria-expanded')).toBe('true');
    await waitFor(() => expect(called(calls, 'tag_notes')).toContainEqual(['tag_notes', { request: { key: 'проект' } }]));
    expect(view.state.doc.toString()).toBe(NOTE);
  });

  it('курсор в теге: обычный щелчок — правка, Ctrl+щелчок — вид Теги', async () => {
    const { view, calls } = await openNote();
    view.dispatch({ selection: { anchor: 14 } });

    await fireEvent.mouseDown(plaques(view)[0] as Element, { button: 0 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(tagsViewShown()).toBe(false);
    expect(called(calls, 'tag_notes')).toEqual([]);

    await fireEvent.mouseDown(plaques(view)[0] as Element, { button: 0, ctrlKey: true });
    await waitFor(() => expect(tagsViewShown()).toBe(true));
    await waitFor(() => expect(called(calls, 'tag_notes')).toContainEqual(['tag_notes', { request: { key: 'проект' } }]));
  });
});

describe('теги в исходном тексте', () => {
  it('в исходном тексте — Ctrl+щелчок', async () => {
    const { view, calls } = await openNote({ ...SETTINGS, editor_mode: 'source' });
    expect(plaques(view)).toEqual([]);
    const at = NOTE.indexOf('#работа') + 3;
    view.posAtCoords = (() => at) as typeof view.posAtCoords;

    await fireEvent.mouseDown(view.contentDOM, { button: 0 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(tagsViewShown()).toBe(false);

    await fireEvent.mouseDown(view.contentDOM, { button: 0, ctrlKey: true });
    await waitFor(() => expect(tagsViewShown()).toBe(true));
    const row = await within(screen.getByRole('complementary', { name: 'Правая панель' })).findByRole('button', { name: /^#работа\/отчёт/ });
    expect(row.getAttribute('aria-expanded')).toBe('true');
    expect(called(calls, 'tag_notes')).toContainEqual(['tag_notes', { request: { key: 'работа/отчёт' } }]);
  });

  it('Ctrl+щелчок по `#` в коде — ничего', async () => {
    const { view, calls } = await openNote({ ...SETTINGS, editor_mode: 'source' });
    const at = NOTE.indexOf('#код') + 1;
    view.posAtCoords = (() => at) as typeof view.posAtCoords;
    await fireEvent.mouseDown(view.contentDOM, { button: 0, ctrlKey: true });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(called(calls, 'tag_notes')).toEqual([]);
    expect(tagsViewShown()).toBe(false);
  });
});
