// Дерево файлов в оболочке окна (задача 2.3): дерево в боковой панели,
// открытие заметки, новое имя сразу и в дереве, и в пути над заметкой (С3.2),
// удалённая заметка закрывается, подтверждение удаления — по настройке.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { Settings } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import { FakeVault } from '../tree/fakeVault.ts';
import Shell from './Shell.svelte';
import { WindowModel } from './window.svelte.ts';

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
};

function setup(settings: Partial<Settings> = {}) {
  const vault = new FakeVault([
    { kind: 'folder', path: ['Проекты'] },
    { kind: 'note', path: ['Проекты', 'Встреча.md'] },
    { kind: 'note', path: ['План.md'] },
  ]);
  const mocks: CoreMocks = {
    ...vault.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    settings_read: () => ({ settings: { ...SETTINGS, ...settings } }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
  };
  const calls = mockCore(mocks);
  const model = new WindowModel();
  render(Shell, { model });
  return { calls, model };
}

const headerPath = () =>
  screen
    .getByRole('navigation', { name: 'Путь к заметке' })
    .textContent?.replace(/\s*\/\s*/g, '/')
    .trim();

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('дерево в оболочке окна', () => {
  it('дерево в боковой панели; клик по заметке открывает её', async () => {
    setup();
    const sidebar = await screen.findByRole('complementary', { name: 'Боковая панель' });
    const tree = within(sidebar).getByRole('tree', { name: 'Файлы хранилища' });
    await waitFor(() => expect(within(tree).getByRole('treeitem', { name: 'План' })).toBeTruthy());

    await fireEvent.click(within(tree).getByRole('treeitem', { name: 'План' }));

    expect(headerPath()).toBe('План');
    expect(within(tree).getByRole('treeitem', { name: 'План' }).getAttribute('aria-current')).toBe('page');
    expect(screen.getByRole('button', { name: 'План' }).getAttribute('title')).toBe('Нажмите, чтобы переименовать');
  });

  it('переименование открытой заметки: новое имя в дереве и в пути над заметкой', async () => {
    const { model } = setup();
    await screen.findByRole('treeitem', { name: 'План' });
    await fireEvent.click(screen.getByRole('treeitem', { name: 'План' }));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await fireEvent.input(screen.getByRole('textbox', { name: 'Новое имя' }), { target: { value: 'План работ' } });
    await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Новое имя' }), { key: 'Enter' });

    await waitFor(() => expect(headerPath()).toBe('План работ'));
    expect(screen.getByRole('treeitem', { name: 'План работ' })).toBeTruthy();
    expect(model.currentPath).toEqual(['План работ.md']);
  });

  it('перемещение папки с открытой заметкой: путь над заметкой меняется', async () => {
    const { model } = setup();
    await screen.findByRole('treeitem', { name: 'Проекты' });
    model.open(['Проекты', 'Встреча.md']);
    await waitFor(() => expect(headerPath()).toBe('Проекты/Встреча'));
    await fireEvent.click(screen.getByRole('treeitem', { name: 'Проекты' }));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await fireEvent.input(screen.getByRole('textbox', { name: 'Новое имя' }), { target: { value: 'Работа' } });
    await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Новое имя' }), { key: 'Enter' });

    await waitFor(() => expect(headerPath()).toBe('Работа/Встреча'));
  });

  it('удаление открытой заметки закрывает её; без подтверждения, если оно выключено', async () => {
    const { model } = setup({ confirm_delete: false });
    await screen.findByRole('treeitem', { name: 'План' });
    await fireEvent.click(screen.getByRole('treeitem', { name: 'План' }));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'Delete' });

    await waitFor(() => expect(screen.getByText('Нет открытой заметки')).toBeTruthy());
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(model.currentPath).toBeNull();
    expect(model.canGoBack).toBe(false);
  });

  it('по умолчанию удаление спрашивает подтверждение', async () => {
    setup();
    await screen.findByRole('treeitem', { name: 'План' });
    await fireEvent.click(screen.getByRole('treeitem', { name: 'План' }));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'Delete' });
    expect(screen.getByRole('dialog', { name: 'Удалить «План»?' })).toBeTruthy();
  });
});
