// Переименование из заголовка заметки над текстом (С3.2, Э4б): та же проверка
// имени, что в дереве; новое имя сразу видно и в дереве, и у оболочки.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TreeEntry } from '../api/index.ts';
import { mockCore } from '../api/testing.ts';
import NoteTitle from './NoteTitle.svelte';
import { FakeVault } from './fakeVault.ts';
import { FileTreeState, type TreeHost } from './tree.svelte.ts';

const ENTRIES: TreeEntry[] = [
  { kind: 'folder', path: ['Проекты'] },
  { kind: 'note', path: ['Проекты', 'Встреча по запуску.md'] },
  { kind: 'note', path: ['Проекты', 'План.md'] },
];

async function setup() {
  const vault = new FakeVault(ENTRIES);
  const calls = mockCore(vault.mocks());
  const host: TreeHost = {
    currentPath: ['Проекты', 'Встреча по запуску.md'],
    confirmDelete: true,
    open: vi.fn(),
    entryRenamed: vi.fn(),
    entryRemoved: vi.fn(),
    notify: vi.fn(),
  };
  const tree = new FileTreeState(host);
  await tree.load();
  const view = render(NoteTitle, { tree, path: ['Проекты', 'Встреча по запуску.md'] });
  return { vault, calls, host, tree, view };
}

const field = () => screen.getByRole('textbox', { name: 'Новое имя' }) as HTMLInputElement;

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('заголовок заметки над текстом', () => {
  it('показывает имя файла без .md и подсказку о переименовании', async () => {
    await setup();
    const title = screen.getByRole('button', { name: 'Встреча по запуску' });
    expect(title.getAttribute('title')).toBe('Нажмите, чтобы переименовать');
  });

  it('клик — поле ввода; Enter сохраняет; дерево и оболочка получают новый путь', async () => {
    const { calls, host, tree } = await setup();
    await fireEvent.click(screen.getByRole('button', { name: 'Встреча по запуску' }));
    expect(field().value).toBe('Встреча по запуску');
    expect(screen.getByText('Enter — сохранить · Esc — отменить')).toBeTruthy();

    await fireEvent.input(field(), { target: { value: 'Встреча по запуску 03.11' } });
    await fireEvent.keyDown(field(), { key: 'Enter' });

    await waitFor(() => expect(screen.queryByRole('textbox')).toBeNull());
    expect(calls.filter(([c]) => c === 'entry_rename').map(([, a]) => a)).toEqual([
      { request: { path: ['Проекты', 'Встреча по запуску.md'], new_name: 'Встреча по запуску 03.11.md' } },
    ]);
    expect(host.entryRenamed).toHaveBeenCalledWith(
      ['Проекты', 'Встреча по запуску.md'],
      ['Проекты', 'Встреча по запуску 03.11.md'],
    );
    expect(tree.entry(['Проекты', 'Встреча по запуску 03.11.md'])).toBeTruthy();
  });

  it('ошибка имени — пояснение под полем, Enter не срабатывает; Esc отменяет', async () => {
    const { calls } = await setup();
    await fireEvent.click(screen.getByRole('button', { name: 'Встреча по запуску' }));
    await fireEvent.input(field(), { target: { value: 'План' } });
    expect((await screen.findByRole('alert')).textContent?.trim()).toBe('В этой папке уже есть элемент с таким именем.');
    await fireEvent.keyDown(field(), { key: 'Enter' });
    await fireEvent.keyDown(field(), { key: 'Escape' });
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.getByRole('button', { name: 'Встреча по запуску' })).toBeTruthy();
    expect(calls.some(([c]) => c === 'entry_rename')).toBe(false);
  });
});
