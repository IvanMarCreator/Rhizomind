import { clearMocks } from '@tauri-apps/api/mocks';
import { EditorView } from '@codemirror/view';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { mockCore } from '../api/testing.ts';
import type { HotkeyOverrides } from '../api/index.ts';
import { DEFAULT_SETTINGS } from '../settings/defaults.ts';
import Shell from '../shell/Shell.svelte';

async function setup(hotkeys: HotkeyOverrides = {}) {
  const calls = mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'palette', name: 'Тест' } }),
    settings_read: () => ({ settings: { ...DEFAULT_SETTINGS, hotkeys } }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [
      { name: 'Планы', path: ['Планы'], kind: 'folder', children: [] },
      { name: 'Текст.md', path: ['Текст.md'], kind: 'note' },
    ] }),
    note_read: () => ({ text: 'Личный план', version: { hash: 'a'.repeat(64), size: 21, modified_ms: 1 }, read_only: null }),
    switcher_search: () => ({ items: [] }),
  });
  render(Shell);
  await screen.findByRole('treeitem', { name: 'Планы' });
  return calls;
}

const key = (target: Element, code: string) => fireEvent.keyDown(target, { key: code, code });
const palette = () => screen.getByRole('dialog', { name: 'Палитра команд' });
const input = () => within(palette()).getByRole('combobox');

afterEach(() => { cleanup(); clearMocks(); });

it('F12 из CodeMirror, отмена возвращает фокус и исходное выделение текста', async () => {
  await setup();
  await fireEvent.click(screen.getByRole('treeitem', { name: 'Текст' }));
  const view = await waitFor(() => {
    const content = document.querySelector('.cm-content');
    const found = content && EditorView.findFromDOM(content as HTMLElement);
    if (!found) throw new Error('Редактор ещё не открыт');
    return found;
  });
  view.focus();
  view.dispatch({ selection: { anchor: 7, head: 11 } });
  await key(view.contentDOM, 'F12');
  expect(document.activeElement).toBe(input());
  await fireEvent.input(input(), { target: { value: 'нет такой команды' } });
  expect(within(palette()).getByRole('status').textContent).toBe('Команды не найдены.');
  await key(input(), 'Enter');
  expect(document.activeElement).toBe(input());
  await key(input(), 'Tab');
  expect(document.activeElement).toBe(input());
  await key(input(), 'Escape');
  expect(document.activeElement).toBe(view.contentDOM);
  expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('план');
});

it('F12 из дерева сохраняет контекст, поиск по русскому названию запускает переименование', async () => {
  await setup();
  const tree = screen.getByRole('tree');
  tree.focus();
  await key(tree, 'ArrowDown');
  await key(tree, 'F12');
  expect(document.activeElement).toBe(input());
  tree.focus();
  expect(document.activeElement).toBe(input());
  expect(within(palette()).getByRole('option', { name: /Назад по истории/ }).getAttribute('aria-disabled')).toBe('true');
  const rename = within(palette()).getByRole('option', { name: /Переименовать/ });
  expect(rename.textContent).toContain('F2');
  await fireEvent.input(input(), { target: { value: 'ИМЕНОВ' } });
  expect(within(palette()).getAllByRole('option')).toHaveLength(1);
  await key(input(), 'Enter');
  await screen.findByDisplayValue('Планы');
  expect(screen.queryByRole('dialog', { name: 'Палитра команд' })).toBeNull();
});

it('стрелки выбирают действие, Enter один раз открывает быстрый переход, F12 не прерывает его', async () => {
  const calls = await setup();
  await key(document.body, 'F12');
  const options = within(palette()).getAllByRole('option');
  expect(options[0]?.getAttribute('aria-selected')).toBe('true');
  await key(input(), 'ArrowDown');
  expect(options[1]?.getAttribute('aria-selected')).toBe('true');
  await key(input(), 'ArrowUp');
  await key(input(), 'Enter');
  const switcher = await screen.findByRole('combobox', { name: 'Быстрый переход' });
  await key(switcher, 'F12');
  expect(document.activeElement).toBe(switcher);
  expect(screen.getAllByRole('dialog')).toHaveLength(1);
  await waitFor(() => expect(calls.filter(([command]) => command === 'switcher_search')).toHaveLength(1));
});

it('F12 запускает выбор шаблона при первом вызове и не открывает вторую палитру поверх', async () => {
  await setup();
  await key(document.body, 'F12');
  await fireEvent.input(input(), { target: { value: 'Создать из шаблона' } });
  await key(input(), 'Enter');
  const chooser = await screen.findByRole('dialog', { name: 'Папка шаблонов' });
  await key(within(chooser).getByRole('textbox'), 'F12');
  expect(screen.queryByRole('dialog', { name: 'Палитра команд' })).toBeNull();
  await key(within(chooser).getByRole('textbox'), 'Escape');
  expect(screen.queryByRole('dialog', { name: 'Создать из шаблона' })).toBeNull();
});

it('прежнее назначение F12 в дереве сохраняется, у палитры есть доступ с ленты', async () => {
  await setup({ 'tree.rename': ['F12'] });
  const tree = screen.getByRole('tree');
  tree.focus();
  await key(tree, 'ArrowDown');
  await key(tree, 'F12');
  const editing = await screen.findByDisplayValue('Планы');
  expect(screen.queryByRole('dialog')).toBeNull();
  await key(editing, 'Escape');
  const button = screen.getByRole('button', { name: 'Палитра команд' });
  button.focus();
  await fireEvent.click(button);
  expect(within(palette()).getByRole('option', { name: /Переименовать/ }).getAttribute('aria-disabled')).toBe('true');
  await key(input(), 'Escape');
  expect(document.activeElement).toBe(button);
});
