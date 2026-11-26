import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import type { NewNoteLocation, Settings, VaultPath } from '../api/index.ts';
import { DEFAULT_SETTINGS } from '../settings/defaults.ts';
import TemplateDialog from './TemplateDialog.svelte';

afterEach(() => { cleanup(); clearMocks(); });

function setup(folder: VaultPath | null, location: NewNoteLocation = { kind: 'vault_root' }, extra: CoreMocks = {}) {
  const model = {
    settings: { ...DEFAULT_SETTINGS, template_folder: folder, new_note_location: location },
    currentPath: ['Работа', 'Сегодня.md'],
    updateSettings: async (patch: Partial<Settings>) => { Object.assign(model.settings, patch); },
    open: vi.fn(),
    notify: vi.fn(),
  };
  const calls = mockCore({
    tree_list: () => ({ entries: [
      { kind: 'folder', path: ['Шаблоны'] },
      { kind: 'note', path: ['Шаблоны', 'А', 'План.md'] },
      { kind: 'note', path: ['Шаблоны', 'Б', 'План.md'] },
      { kind: 'note', path: ['Шаблоны-архив', 'Лишняя.md'] },
      { kind: 'attachment', path: ['Шаблоны', 'схема.png'] },
    ] }),
    note_create_from_template: ({ dir, name }) => ({ path: [...dir, name] }),
    ...extra,
  });
  const onClose = vi.fn();
  render(TemplateDialog, { model, onClose });
  return { model, onClose, calls };
}

it.each([
  [{ kind: 'vault_root' }, []],
  [{ kind: 'current_folder' }, ['Работа']],
  [{ kind: 'folder', path: ['Входящие'] }, ['Входящие']],
] as [NewNoteLocation, VaultPath][])('template: копирует выбранный путь и открывает заметку (%j)', async (mode, dir) => {
  const { model, onClose, calls } = setup(['Шаблоны'], mode);
  await fireEvent.click(await screen.findByRole('button', { name: 'Б/План.md' }));
  expect(screen.queryByRole('button', { name: /Лишняя/ })).toBeNull();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Имя новой заметки' }), { target: { value: 'Проверка' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));
  expect(calls.find(([cmd]) => cmd === 'note_create_from_template')?.[1]).toEqual({
    request: { template: ['Шаблоны', 'Б', 'План.md'], dir, name: 'Проверка.md' },
  });
  await waitFor(() => expect(model.open).toHaveBeenCalledWith([...dir, 'Проверка.md']));
  expect(onClose).toHaveBeenCalledOnce();
});

it('template: стрелкой вверх из поиска выбирает последний из двух путей', async () => {
  setup(['Шаблоны']);
  const last = await screen.findByRole('button', { name: 'Б/План.md' });
  const field = screen.getByRole('textbox', { name: 'Найти шаблон' });
  field.focus();
  await fireEvent.keyDown(field, { key: 'ArrowUp' });
  expect(document.activeElement).toBe(last);
});

it.each(['not_found', 'already_exists', 'read_only', 'not_implemented'] as const)('template: ошибка %s оставляет имя для пользователя', async (code) => {
  const { model, onClose } = setup(['Шаблоны'], { kind: 'vault_root' }, {
    note_create_from_template: () => { throw { code, message_key: `errors.${code}` }; },
  });
  await fireEvent.click(await screen.findByRole('button', { name: 'А/План.md' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));
  const message = (await screen.findByRole('alert')).textContent?.trim();
  expect(message).toBeTruthy();
  if (code === 'not_implemented') expect(message).toContain('безопасное создание из шаблона');
  expect((screen.getByRole('textbox', { name: 'Имя новой заметки' }) as HTMLInputElement).value).toBe('План');
  expect(model.open).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});

it('template: опубликованная копия без доступного пути закрывает диалог и показывает результат', async () => {
  const { model, onClose, calls } = setup(['Шаблоны'], { kind: 'vault_root' }, {
    note_create_from_template: () => ({ path: null }),
  });
  await fireEvent.click(await screen.findByRole('button', { name: 'А/План.md' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(model.open).not.toHaveBeenCalled();
  expect(model.notify).toHaveBeenCalledWith(expect.stringContaining('Копия была создана'));
  expect(calls.filter(([cmd]) => cmd === 'note_create_from_template')).toHaveLength(1);
});

it('template: Enter создаёт копию один раз после окончания композиции и отпускания повтора', async () => {
  const { calls, onClose } = setup(['Шаблоны']);
  await fireEvent.click(await screen.findByRole('button', { name: 'А/План.md' }));
  const input = screen.getByRole('textbox', { name: 'Имя новой заметки' });
  await fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
  await fireEvent.keyDown(input, { key: 'Enter', repeat: true });
  expect(calls.some(([cmd]) => cmd === 'note_create_from_template')).toBe(false);
  await fireEvent.keyDown(input, { key: 'Enter' });
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(calls.filter(([cmd]) => cmd === 'note_create_from_template')).toHaveLength(1);
});

it('template: первое открытие предлагает папку, при отмене файл не создаётся', async () => {
  const { calls, model, onClose } = setup(null);
  await fireEvent.click(await screen.findByRole('button', { name: 'Шаблоны' }));
  expect(model.settings.template_folder).toEqual(['Шаблоны']);
  await fireEvent.click(await screen.findByRole('button', { name: 'А/План.md' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Отмена' }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(calls.some(([cmd]) => cmd === 'note_create_from_template')).toBe(false);
});
