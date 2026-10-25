import { clearMocks } from '@tauri-apps/api/mocks';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { CoreError, Settings } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
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

const WITH_VAULT: CoreMocks = {
  vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
  settings_read: () => ({ settings: SETTINGS }),
  workspace_read: () => ({ workspace: null }),
  workspace_write: () => null,
};

function savedLayout(path: string[]) {
  return {
    format: 'rhizomind-layout',
    version: 1,
    root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path }], active: 0 },
    active_pane: 'pane-1',
  };
}

async function renderShell(mocks: CoreMocks, model = new WindowModel()) {
  const calls = mockCore(mocks);
  render(Shell, { model });
  return { calls, model };
}

afterEach(() => {
  clearMocks();
  delete document.documentElement.dataset.theme;
});

describe('оболочка окна', () => {
  it('каркас К2: лента, боковая панель с переключателем хранилищ, пустое состояние', async () => {
    await renderShell(WITH_VAULT);

    expect(await screen.findByText('Нет открытой заметки')).toBeTruthy();
    const ribbon = screen.getByRole('toolbar', { name: 'Лента иконок' });
    expect(within(ribbon).getByRole('button', { name: 'Файлы' })).toBeTruthy();
    expect(within(ribbon).getByRole('button', { name: 'Быстрый переход (Ctrl+O)' })).toBeTruthy();
    expect(within(ribbon).getByRole('button', { name: 'Настройки' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Хранилище «Рабочая база»' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Уведомления' })).toBeTruthy();
  });

  it('«Файлы» на ленте прячет и показывает боковую панель', async () => {
    await renderShell(WITH_VAULT);
    const files = await screen.findByRole('button', { name: 'Файлы' });
    expect(files.getAttribute('aria-pressed')).toBe('true');

    await fireEvent.click(files);

    expect(files.getAttribute('aria-pressed')).toBe('false');
    expect(screen.queryByRole('complementary', { name: 'Боковая панель' })).toBeNull();
  });

  it('окно без хранилища не читает настройки и раскладку', async () => {
    const { calls } = await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
    });

    expect(await screen.findByRole('button', { name: 'Хранилище не открыто' })).toBeTruthy();
    expect(calls.map(([command]) => command)).toEqual(['vault_current']);
  });

  it('несовпадение версии API — понятное сообщение вместо пустого окна', async () => {
    await renderShell({ vault_current: () => ({ api_version: 2, vault: null }) });

    expect(await screen.findByText('Части приложения не совпадают по версии')).toBeTruthy();
    expect(
      screen.getByText(
        'Интерфейс ожидает API ядра версии 1, а ядро сообщает версию 2. Установите Rhizomind заново из одного выпуска.',
      ),
    ).toBeTruthy();
    expect(screen.queryByRole('toolbar')).toBeNull();
  });

  it('сбой моста при запуске: общий текст без подробностей Tauri', async () => {
    mockCore({});
    // Отказ моста — не CoreError: так приходят ошибки разрешений Tauri.
    window.__TAURI_INTERNALS__.invoke = () =>
      Promise.reject(new Error('command vault_current not allowed: Секретная/Заметка.md'));
    render(Shell, { model: new WindowModel() });

    expect(await screen.findByText('Окно не открылось')).toBeTruthy();
    expect(screen.getByText('Внутренняя ошибка приложения. Перезапустите окно.')).toBeTruthy();
    expect(document.body.textContent).not.toContain('Секретная');
  });

  it('тема из настроек хранилища ставится на окно', async () => {
    await renderShell({ ...WITH_VAULT, settings_read: () => ({ settings: { ...SETTINGS, theme: 'dark' } }) });

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe('dark'));
  });

  it('открытая заметка из раскладки: путь над заметкой и переключатель режима', async () => {
    await renderShell({ ...WITH_VAULT, workspace_read: () => ({ workspace: savedLayout(['Проекты', 'Альфа', 'Встреча по запуску.md']) }) });

    const path = await screen.findByRole('navigation', { name: 'Путь к заметке' });
    // Отступы вокруг «/» — средствами CSS, в тексте их нет.
    expect(path.textContent?.replace(/\s*\/\s*/g, '/').trim()).toBe('Проекты/Альфа/Встреча по запуску');
    expect(screen.queryByText('Нет открытой заметки')).toBeNull();

    const preview = screen.getByRole('button', { name: 'Живой предпросмотр' });
    const source = screen.getByRole('button', { name: 'Исходный текст (Ctrl+E)' });
    expect(preview.getAttribute('aria-pressed')).toBe('true');
    await fireEvent.click(source);
    expect(source.getAttribute('aria-pressed')).toBe('true');
    expect(preview.getAttribute('aria-pressed')).toBe('false');
  });

  it('повреждённая раскладка: окно открывается, внизу уведомление, его можно закрыть', async () => {
    await renderShell({ ...WITH_VAULT, workspace_read: () => ({ workspace: { format: 'чужой' } }) });

    const notice = await screen.findByText('Раскладка окна была повреждена и сброшена.');
    expect(screen.getByText('Нет открытой заметки')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Закрыть уведомление' }));

    expect(notice.isConnected).toBe(false);
  });

  it('ошибка чтения настроек — уведомление с текстом из словаря, окно работает', async () => {
    const failure: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    await renderShell({
      ...WITH_VAULT,
      settings_read: () => {
        throw failure;
      },
    });

    expect(await screen.findByText('Не удалось прочитать или записать файл.')).toBeTruthy();
    expect(screen.getByText('Нет открытой заметки')).toBeTruthy();
  });

  it('Ctrl+O (и в русской раскладке) открывает быстрый переход; выбор открывает заметку, «назад» возвращает', async () => {
    const { model } = await renderShell({
      ...WITH_VAULT,
      switcher_search: () => ({ items: [{ name: 'Итоги', path: ['Итоги.md'] }] }),
    });
    await screen.findByText('Нет открытой заметки');
    model.open(['А.md']);

    await fireEvent.keyDown(window, { key: 'щ', code: 'KeyO', ctrlKey: true });
    const input = screen.getByRole('combobox', { name: 'Быстрый переход' });
    await screen.findByRole('option', { name: /Итоги/ });

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(screen.queryByRole('dialog', { name: 'Быстрый переход' })).toBeNull();
    await screen.findByText('Итоги');
    await fireEvent.click(screen.getByRole('button', { name: 'Назад (Alt+←)' }));
    await screen.findByText('А');
  });

  it('кнопка «Быстрый переход» на ленте открывает окно, Esc закрывает', async () => {
    await renderShell({ ...WITH_VAULT, switcher_search: () => ({ items: [] }) });
    await fireEvent.click(await screen.findByRole('button', { name: 'Быстрый переход (Ctrl+O)' }));

    const input = screen.getByRole('combobox', { name: 'Быстрый переход' });
    await fireEvent.keyDown(input, { key: 'Escape' });

    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('пока открыт быстрый переход, Alt+← не листает историю за ним', async () => {
    const { model } = await renderShell({ ...WITH_VAULT, switcher_search: () => ({ items: [] }) });
    await screen.findByText('Нет открытой заметки');
    model.open(['А.md']);
    model.open(['Б.md']);
    await screen.findByText('Б');

    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    await fireEvent.keyDown(screen.getByRole('combobox'), { key: 'ArrowLeft', altKey: true });

    expect(model.currentPath).toEqual(['Б.md']);
  });

  it('без хранилища быстрый переход не открывается', async () => {
    await renderShell({ vault_current: () => ({ api_version: 1, vault: null }) });
    await screen.findByRole('button', { name: 'Хранилище не открыто' });

    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });

    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('назад и вперёд: кнопки и Alt+←/Alt+→; переход сохраняет раскладку', async () => {
    const { calls, model } = await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const back = screen.getByRole('button', { name: 'Назад (Alt+←)' }) as HTMLButtonElement;
    const forward = screen.getByRole('button', { name: 'Вперёд (Alt+→)' }) as HTMLButtonElement;
    expect(back.disabled).toBe(true);

    model.open(['А.md']);
    model.open(['Папка', 'Б.md']);
    await screen.findByText('Б');
    expect(back.disabled).toBe(false);
    expect(forward.disabled).toBe(true);

    await fireEvent.keyDown(window, { key: 'ArrowLeft', altKey: true });
    await screen.findByText('А');
    expect(forward.disabled).toBe(false);

    await fireEvent.click(forward);
    await screen.findByText('Б');

    await waitFor(() => {
      const writes = calls.filter(([command]) => command === 'workspace_write');
      expect(writes.at(-1)?.[1]).toEqual({ request: { workspace: savedLayout(['Папка', 'Б.md']) } });
    });
  });
});
