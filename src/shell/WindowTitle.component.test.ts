// Заголовок окна (задача 2.12, решение владельца 82): оболочка сообщает ядру,
// какая заметка показана (`window_shown_note`), — при открытии, переходе,
// переименовании и закрытии заметки; ввод текста вызовов не добавляет.
// Сам заголовок ставит ядро (src-tauri/src/commands/window.rs).
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { Settings, VaultPath } from '../api/index.ts';
import { mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import { FakeVault } from '../tree/fakeVault.ts';
import Shell from './Shell.svelte';
import { WindowModel } from './window.svelte.ts';

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'source',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
};

const version = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };

function savedLayout(path: VaultPath) {
  return {
    format: 'rhizomind-layout',
    version: 1,
    root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path }], active: 0 },
    active_pane: 'pane-1',
  };
}

function mocks(vault: FakeVault, layout: VaultPath | null = null): CoreMocks {
  return {
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Архив' } }),
    settings_read: () => ({ settings: SETTINGS }),
    workspace_read: () => ({ workspace: layout ? savedLayout(layout) : null }),
    workspace_write: () => null,
    note_read: () => ({ text: 'Год закрыт', version, read_only: null }),
    note_write: () => ({ outcome: 'saved', version }),
    window_shown_note: () => null,
    ...vault.mocks(),
  };
}

/** Пути из вызовов `window_shown_note` по порядку. */
const shown = (calls: RecordedCall[]) =>
  calls
    .filter(([command]) => command === 'window_shown_note')
    .map(([, args]) => (args as { request: { path: VaultPath | null } }).request.path);

async function renderShell(vault: FakeVault, layout: VaultPath | null = null) {
  const calls = mockCore(mocks(vault, layout));
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  return { calls, model };
}

const notes = () =>
  new FakeVault([
    { path: ['Итоги года.md'], kind: 'note' },
    { path: ['План.md'], kind: 'note' },
  ]);

afterEach(() => {
  cleanup();
  clearMocks();
  delete document.documentElement.dataset.theme;
});

describe('заголовок окна: ядру сообщается показанная заметка', () => {
  it('заметка из раскладки при запуске — сразу', async () => {
    const { calls } = await renderShell(notes(), ['Итоги года.md']);

    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md']]));
  });

  it('без заметки — ни одного вызова: заголовок «<хранилище> — Rhizomind» ядро ставит само', async () => {
    const { calls } = await renderShell(notes());
    await screen.findByText('Нет открытой заметки');

    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(shown(calls)).toEqual([]);
  });

  it('открытие, переход к другой заметке, «назад»', async () => {
    const { calls, model } = await renderShell(notes());
    await screen.findByText('Нет открытой заметки');

    model.open(['Итоги года.md']);
    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md']]));
    model.open(['План.md']);
    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md'], ['План.md']]));
    await fireEvent.click(screen.getByRole('button', { name: 'Назад (Alt+←)' }));
    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md'], ['План.md'], ['Итоги года.md']]));
    // Повторное открытие показанной заметки — без вызова.
    model.open(['Итоги года.md']);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(shown(calls)).toHaveLength(3);
  });

  it('переименование открытой заметки и её папки — новый путь', async () => {
    const vault = new FakeVault([{ path: ['Проекты', 'Итоги года.md'], kind: 'note' }]);
    const { calls, model } = await renderShell(vault, ['Проекты', 'Итоги года.md']);
    await waitFor(() => expect(shown(calls)).toHaveLength(1));

    // Так дерево файлов сообщает окну о переименовании (TreeHost.entryRenamed).
    model.entryRenamed(['Проекты', 'Итоги года.md'], ['Проекты', 'Итоги 2026.md']);
    await waitFor(() => expect(shown(calls).at(-1)).toEqual(['Проекты', 'Итоги 2026.md']));
    model.entryRenamed(['Проекты'], ['Архив']);
    await waitFor(() => expect(shown(calls).at(-1)).toEqual(['Архив', 'Итоги 2026.md']));
    expect(shown(calls)).toHaveLength(3);
  });

  it('удаление открытой заметки в приложении — «заметки нет»', async () => {
    const { calls } = await renderShell(notes(), ['Итоги года.md']);
    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md']]));

    await fireEvent.contextMenu(await screen.findByRole('treeitem', { name: 'Итоги года' }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /Удалить/ }));
    await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Удалить' }));

    await waitFor(() => expect(shown(calls)).toEqual([['Итоги года.md'], null]));
  });

  it('ввод текста не добавляет вызовов', async () => {
    const { calls } = await renderShell(notes(), ['Итоги года.md']);
    const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
    const view = EditorView.findFromDOM(content);
    if (!view) throw new Error('нет редактора');
    await waitFor(() => expect(shown(calls)).toHaveLength(1));

    for (const letter of ' и начат новый') {
      view.dispatch({ changes: { from: view.state.doc.length, insert: letter }, userEvent: 'input.type' });
    }
    await waitFor(() => expect(calls.some(([command]) => command === 'note_write')).toBe(true), { timeout: 5_000 });

    expect(shown(calls)).toEqual([['Итоги года.md']]);
  });

  it('ядро отказало (заметки уже нет) — заголовок без заметки', async () => {
    const vault = notes();
    const calls = mockCore({
      ...mocks(vault, ['Удалена.md']),
      window_shown_note: ({ path }) => {
        if (path) throw { code: 'not_found', message_key: 'errors.not_found' };
        return null;
      },
    });
    render(Shell, { model: new WindowModel() });

    await waitFor(() => expect(shown(calls)).toEqual([['Удалена.md'], null]));
  });
});
