// Сценарии окна «Настройки хранилища» (С3.1, задача 2.8б): каждая настройка
// меняется в окне, записывается в хранилище и действует сразу.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { Settings, SettingsWriteRequest } from '../api/index.ts';
import { mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';
import { FakeVault } from '../tree/fakeVault.ts';
import { DEFAULT_SETTINGS } from './defaults.ts';

const MIB = 1024 * 1024;

function setup(settings: Partial<Settings> = {}, extra: CoreMocks = {}) {
  const vault = new FakeVault([
    { kind: 'folder', path: ['Проекты'] },
    { kind: 'folder', path: ['Проекты', 'Альфа'] },
    { kind: 'folder', path: ['.rhizomind'] },
    { kind: 'note', path: ['Проекты', 'Встреча.md'] },
    { kind: 'note', path: ['План.md'] },
  ]);
  const mocks: CoreMocks = {
    ...vault.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    vault_list: () => ({
      vaults: [{ id: 'v1', name: 'Рабочая база', path: 'D:\\Заметки\\Рабочая база', is_open: true, available: true }],
      last_opened: 'v1',
    }),
    settings_read: () => ({ settings: { ...DEFAULT_SETTINGS, ...settings } }),
    settings_write: () => null,
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    switcher_search: () => ({ items: [] }),
    ...extra,
  };
  const calls = mockCore(mocks);
  const model = new WindowModel();
  render(Shell, { model });
  return { calls, model, vault };
}

/** Последние записанные настройки. */
function lastWrite(calls: RecordedCall[]): Settings | undefined {
  const writes = calls.filter(([command]) => command === 'settings_write');
  return (writes.at(-1)?.[1] as { request: SettingsWriteRequest } | undefined)?.request.settings;
}

async function openSettings(section?: string) {
  await fireEvent.click(await screen.findByRole('button', { name: 'Настройки' }));
  const dialog = screen.getByRole('dialog', { name: 'Настройки хранилища «Рабочая база»' });
  if (section) await fireEvent.click(within(dialog).getByRole('button', { name: section }));
  return dialog;
}

const root = () => document.documentElement;

afterEach(() => {
  cleanup();
  clearMocks();
  delete root().dataset.theme;
  root().style.removeProperty('--note-column-width');
  root().style.removeProperty('--note-font-size');
});

describe('окно «Настройки хранилища»', () => {
  it('шестерёнка открывает окно с разделами; Esc и «Закрыть» закрывают', async () => {
    setup();
    const dialog = await openSettings();
    const nav = within(dialog).getByRole('navigation', { name: 'Разделы настроек' });
    expect(within(nav).getAllByRole('button').map((b) => b.textContent?.trim())).toEqual([
      'Общие',
      'Редактор',
      'Файлы и ссылки',
      'Оформление',
      'Горячие клавиши',
      'О программе',
    ]);
    expect(within(dialog).getByRole('heading', { name: 'Общие' })).toBeTruthy();
    expect(await within(dialog).findByText('Рабочая база · D:\\Заметки\\Рабочая база')).toBeTruthy();
    expect(within(dialog).getByText('.rhizomind/settings.json')).toBeTruthy();

    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();

    await openSettings();
    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('пока окно открыто, горячие клавиши окна не действуют', async () => {
    setup();
    await openSettings();
    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    expect(screen.queryByRole('dialog', { name: 'Быстрый переход' })).toBeNull();
  });

  it('в «Редакторе» нет режима по умолчанию — только подсказка про кнопки и Ctrl+E', async () => {
    setup();
    const dialog = await openSettings('Редактор');
    expect(within(dialog).queryByText('Режим по умолчанию')).toBeNull();
    expect(within(dialog).getByText(/кнопками «книга \/ карандаш» справа над заметкой или Ctrl\+E/)).toBeTruthy();
    expect(within(dialog).getByText('в следующем выпуске')).toBeTruthy();
  });

  it('тема: «Тёмная» — сразу тёмная тема окна и запись в хранилище', async () => {
    const { calls } = setup();
    await waitFor(() => expect(root().dataset.theme).toBe('light'));
    const dialog = await openSettings('Оформление');

    await fireEvent.click(within(dialog).getByRole('radio', { name: 'Тёмная' }));

    expect(root().dataset.theme).toBe('dark');
    expect(within(dialog).getByRole('radio', { name: 'Тёмная' }).getAttribute('aria-checked')).toBe('true');
    await waitFor(() => expect(lastWrite(calls)?.theme).toBe('dark'));
    // Пишутся все настройки целиком: остальное не меняется.
    expect(lastWrite(calls)).toEqual({ ...DEFAULT_SETTINGS, theme: 'dark' });
  });

  it('пишутся только изменённые поля поверх файла: режим заметки, выбранный в заметке, не затирается', async () => {
    let disk: Settings = { ...DEFAULT_SETTINGS };
    const { calls } = setup(
      {},
      {
        settings_read: () => ({ settings: disk }),
        settings_write: ({ settings }) => {
          disk = settings;
          return null;
        },
      },
    );
    const dialog = await openSettings('Оформление');
    // Режим сменили в заметке (задача 2.5) уже после открытия окна.
    disk = { ...disk, editor_mode: 'source' };

    await fireEvent.click(within(dialog).getByRole('radio', { name: 'Светлая' }));

    await waitFor(() => expect(lastWrite(calls)).toEqual({ ...DEFAULT_SETTINGS, theme: 'light', editor_mode: 'source' }));
  });

  it('ширина текста: «Во всю ширину» снимает ограничение колонки', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Редактор');
    expect(root().style.getPropertyValue('--note-column-width')).toBe('');

    await fireEvent.click(within(dialog).getByRole('radio', { name: 'Во всю ширину' }));

    expect(root().style.getPropertyValue('--note-column-width')).toBe('none');
    await waitFor(() => expect(lastWrite(calls)?.text_width).toBe('full'));

    await fireEvent.click(within(dialog).getByRole('radio', { name: 'Колонка ~70 знаков' }));
    expect(root().style.getPropertyValue('--note-column-width')).toBe('');
  });

  it('размер шрифта: новое значение сразу; вне пределов — прежнее', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Редактор');
    await waitFor(() => expect(root().style.getPropertyValue('--note-font-size')).toBe('16px'));
    const field = within(dialog).getByRole('spinbutton', { name: 'Размер шрифта текста' }) as HTMLInputElement;

    await fireEvent.change(field, { target: { value: '20' } });
    expect(root().style.getPropertyValue('--note-font-size')).toBe('20px');
    await waitFor(() => expect(lastWrite(calls)?.font_size).toBe(20));

    await fireEvent.change(field, { target: { value: '99' } });
    expect(field.value).toBe('20');
    expect(root().style.getPropertyValue('--note-font-size')).toBe('20px');
  });

  it('где создавать новые заметки: «В папке текущей заметки» — Ctrl+N (русская раскладка) создаёт там', async () => {
    const { calls, model } = setup();
    const dialog = await openSettings('Файлы и ссылки');
    await fireEvent.change(within(dialog).getByRole('combobox', { name: 'Где создавать новые заметки' }), {
      target: { value: 'current_folder' },
    });
    await waitFor(() => expect(lastWrite(calls)?.new_note_location).toEqual({ kind: 'current_folder' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть' }));

    model.open(['Проекты', 'Встреча.md']);
    await fireEvent.keyDown(window, { key: 'т', code: 'KeyN', ctrlKey: true });

    await waitFor(() =>
      expect(calls.find(([command]) => command === 'note_create')?.[1]).toEqual({
        request: { dir: ['Проекты'], name: 'Без названия.md' },
      }),
    );
  });

  it('где создавать новые заметки: «В указанной папке» — выбор папки внутри хранилища, Ctrl+N создаёт в ней', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Файлы и ссылки');
    await fireEvent.change(within(dialog).getByRole('combobox', { name: 'Где создавать новые заметки' }), {
      target: { value: 'folder' },
    });
    expect(within(dialog).getByTestId('new-note-folder').textContent).toContain('Корень хранилища');

    await fireEvent.click(within(dialog).getByRole('button', { name: 'Выбрать…' }));
    const picker = within(dialog).getByRole('dialog', { name: 'Папка для новых заметок' });
    // Служебная папка .rhizomind в списке не показывается.
    await within(picker).findByRole('button', { name: 'Альфа' });
    expect(within(picker).queryByRole('button', { name: '.rhizomind' })).toBeNull();
    await fireEvent.click(within(picker).getByRole('button', { name: 'Альфа' }));

    expect(within(dialog).getByTestId('new-note-folder').textContent).toContain('Проекты / Альфа');
    await waitFor(() =>
      expect(lastWrite(calls)?.new_note_location).toEqual({ kind: 'folder', path: ['Проекты', 'Альфа'] }),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть' }));

    await fireEvent.keyDown(window, { key: 'n', code: 'KeyN', ctrlKey: true });
    await waitFor(() =>
      expect(calls.find(([command]) => command === 'note_create')?.[1]).toEqual({
        request: { dir: ['Проекты', 'Альфа'], name: 'Без названия.md' },
      }),
    );
  });

  it('подтверждение удаления: выключено — Delete в дереве удаляет без вопроса', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Файлы и ссылки');
    const toggle = within(dialog).getByRole('switch', { name: 'Подтверждать удаление в корзину' });
    expect(toggle.getAttribute('aria-checked')).toBe('true');

    await fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    await waitFor(() => expect(lastWrite(calls)?.confirm_delete).toBe(false));
    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть' }));

    await fireEvent.click(await screen.findByRole('treeitem', { name: 'План' }));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'Delete', code: 'Delete' });

    expect(screen.queryByRole('dialog', { name: 'Удалить «План»?' })).toBeNull();
    await waitFor(() => expect(calls.some(([command]) => command === 'entry_trash')).toBe(true));
  });

  it('лимит размера для редактирования — в МБ; действует при следующем открытии', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Файлы и ссылки');
    expect(within(dialog).getByText(/Новое значение действует при следующем открытии файла/)).toBeTruthy();
    const field = within(dialog).getByRole('spinbutton', {
      name: 'Максимальный размер файла для редактирования',
    }) as HTMLInputElement;
    expect(field.value).toBe('10');

    await fireEvent.change(field, { target: { value: '25' } });

    await waitFor(() => expect(lastWrite(calls)?.max_edit_size_bytes).toBe(25 * MIB));
    await fireEvent.change(field, { target: { value: '0' } });
    expect(field.value).toBe('25');
  });

  it('запись не удалась — уведомление, настройка возвращается к записанной', async () => {
    setup(
      {},
      {
        settings_write: () => {
          throw { code: 'io_failed', message_key: 'errors.io_failed' };
        },
      },
    );
    const dialog = await openSettings('Оформление');

    await fireEvent.click(within(dialog).getByRole('radio', { name: 'Тёмная' }));

    expect(await screen.findByText('Не удалось прочитать или записать файл.')).toBeTruthy();
    await waitFor(() => expect(root().dataset.theme).toBe('light'));
    expect(within(dialog).getByRole('radio', { name: 'Как в системе' }).getAttribute('aria-checked')).toBe('true');
  });

  it('«О программе»: версия, редакция, лицензия, безопасность', async () => {
    setup();
    const dialog = await openSettings('О программе');
    expect(within(dialog).getByText('0.1.0')).toBeTruthy();
    expect(within(dialog).getByText('Secure')).toBeTruthy();
    expect(within(dialog).getByText('GPL-3.0')).toBeTruthy();
    expect(within(dialog).getByText('Приложение не выходит в сеть: в нём нет сетевых функций.')).toBeTruthy();
  });
});

describe('горячие клавиши в окне', () => {
  it('переназначенное сочетание из настроек хранилища действует, прежнее — нет', async () => {
    setup({ hotkeys: { 'quick_switcher.open': ['Ctrl+KeyP'] } });
    await screen.findByText('Нет открытой заметки');
    expect(screen.getByRole('button', { name: 'Быстрый переход (Ctrl+P)' })).toBeTruthy();

    await fireEvent.keyDown(window, { key: 'щ', code: 'KeyO', ctrlKey: true });
    expect(screen.queryByRole('dialog', { name: 'Быстрый переход' })).toBeNull();

    await fireEvent.keyDown(window, { key: 'з', code: 'KeyP', ctrlKey: true });
    expect(await screen.findByRole('dialog', { name: 'Быстрый переход' })).toBeTruthy();
  });

  it('назначение в окне настроек действует сразу и пишется в хранилище', async () => {
    const { calls } = setup();
    const dialog = await openSettings('Горячие клавиши');

    await fireEvent.click(within(dialog).getByRole('button', { name: 'Изменить сочетание Ctrl+O' }));
    await fireEvent.keyDown(window, { key: 'з', code: 'KeyP', ctrlKey: true });

    await waitFor(() => expect(lastWrite(calls)?.hotkeys).toEqual({ 'quick_switcher.open': ['Ctrl+KeyP'] }));
    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть' }));
    await fireEvent.keyDown(window, { key: 'з', code: 'KeyP', ctrlKey: true });
    expect(await screen.findByRole('dialog', { name: 'Быстрый переход' })).toBeTruthy();
  });

  it('F2 переназначен на F3 — в дереве переименовывает F3, F2 — нет; меню показывает F3', async () => {
    setup({ hotkeys: { 'tree.rename': ['F3'] } });
    await fireEvent.click(await screen.findByRole('treeitem', { name: 'План' }));

    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2', code: 'F2' });
    expect(screen.queryByRole('textbox', { name: 'Новое имя' })).toBeNull();

    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F3', code: 'F3' });
    expect(screen.getByRole('textbox', { name: 'Новое имя' })).toBeTruthy();
    await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Новое имя' }), { key: 'Escape' });

    await fireEvent.contextMenu(screen.getByRole('treeitem', { name: 'План' }));
    expect(screen.getByRole('menuitem', { name: /Переименовать/ }).textContent).toContain('F3');
  });

  it('режим заметки — по сочетанию из реестра (переназначенному) и запоминается в хранилище; без заметки не переключается', async () => {
    const { calls, model } = setup({ hotkeys: { 'editor.toggle-mode': ['Ctrl+Shift+KeyM'] } });
    await screen.findByText('Нет открытой заметки');
    await fireEvent.keyDown(window, { key: 'Ь', code: 'KeyM', ctrlKey: true, shiftKey: true });
    expect(model.editorMode).toBe('live_preview');

    model.open(['План.md']);
    // Прежнее Ctrl+E больше не команда режима.
    await fireEvent.keyDown(window, { key: 'у', code: 'KeyE', ctrlKey: true });
    expect(model.editorMode).toBe('live_preview');
    await fireEvent.keyDown(window, { key: 'Ь', code: 'KeyM', ctrlKey: true, shiftKey: true });
    expect(model.editorMode).toBe('source');
    await waitFor(() => expect(lastWrite(calls)?.editor_mode).toBe('source'));
    // Пишется только режим, назначения клавиш на месте.
    expect(lastWrite(calls)?.hotkeys).toEqual({ 'editor.toggle-mode': ['Ctrl+Shift+KeyM'] });
  });

  it('гонка старта: режим, выбранный до чтения настроек, не откатывается к прочитанному и записывается', async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => (release = resolve));
    const mocks: CoreMocks = {
      vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
      settings_read: async () => {
        await gate;
        return { settings: { ...DEFAULT_SETTINGS } };
      },
      settings_write: () => null,
      workspace_read: () => ({ workspace: null }),
      workspace_write: () => null,
    };
    const calls = mockCore(mocks);
    const model = new WindowModel();
    const started = model.start();
    await waitFor(() => expect(model.vault).not.toBeNull());

    model.setEditorMode('source');
    release();
    await started;

    expect(model.editorMode).toBe('source');
    expect(model.settings.editor_mode).toBe('source');
    await waitFor(() => expect(lastWrite(calls)?.editor_mode).toBe('source'));
  });
});
