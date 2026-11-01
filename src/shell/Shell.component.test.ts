import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { CoreError, Settings } from '../api/index.ts';
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
  hotkeys: {},
};

const WITH_VAULT: CoreMocks = {
  vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
  settings_read: () => ({ settings: SETTINGS }),
  workspace_read: () => ({ workspace: null }),
  workspace_write: () => null,
  tree_list: () => ({ entries: [] }),
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
  // Сначала размонтировать (дерево файлов отписывается от событий), потом снять подмену.
  cleanup();
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
    // Окно настроек (задача 2.8б): шестерёнка активна.
    const settings = within(ribbon).getByRole('button', { name: 'Настройки' });
    expect(settings.getAttribute('aria-disabled')).toBeNull();
    expect(settings.getAttribute('title')).toBe('Настройки');
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

  it('окно без хранилища — «Управление хранилищами»; настройки и раскладка не читаются', async () => {
    const { calls } = await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
      vault_list: () => ({ vaults: [], last_opened: null }),
    });

    expect(await screen.findByText('Пока нет хранилищ')).toBeTruthy();
    expect(screen.queryByTestId('shell')).toBeNull();
    // Подписка на сигнал закрытия окна (задача 0.7) — не обращение к ядру.
    const commands = () => calls.map(([command]) => command).filter((command) => !command.startsWith('plugin:event|'));
    await waitFor(() => expect(commands()).toEqual(['vault_current', 'vault_list']));
  });

  it('уведомления окна управления — внизу окна', async () => {
    const failed: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
      vault_list: () => {
        throw failed;
      },
    });

    const region = screen.getByRole('region', { name: 'Уведомления' });
    expect(await within(region).findByText('Не удалось прочитать или записать файл.')).toBeTruthy();
  });

  it('переключатель хранилищ внизу боковой панели открывает меню хранилищ', async () => {
    await renderShell({
      ...WITH_VAULT,
      vault_list: () => ({
        vaults: [{ id: 'v1', name: 'Рабочая база', path: 'D:\\Рабочая база', is_open: true, available: true }],
        last_opened: 'v1',
      }),
    });
    const sidebar = await screen.findByRole('complementary', { name: 'Боковая панель' });

    await fireEvent.click(within(sidebar).getByRole('button', { name: 'Хранилище «Рабочая база»' }));

    const menu = await within(sidebar).findByRole('menu', { name: 'Хранилища' });
    expect(within(menu).getByRole('menuitem', { name: 'Управление хранилищами…' })).toBeTruthy();
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

    const preview = screen.getByRole('radio', { name: 'Живой предпросмотр' });
    const source = screen.getByRole('radio', { name: 'Исходный текст' });
    expect(preview.getAttribute('aria-checked')).toBe('true');
    await fireEvent.click(source);
    expect(source.getAttribute('aria-checked')).toBe('true');
    expect(preview.getAttribute('aria-checked')).toBe('false');
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
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('Итоги');
    await fireEvent.click(screen.getByRole('button', { name: 'Назад (Alt+←)' }));
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('А');
  });

  it('подсказки берут сочетания из реестра горячих клавиш (задача 2.14)', async () => {
    await renderShell({
      ...WITH_VAULT,
      settings_read: () => ({
        settings: {
          ...SETTINGS,
          hotkeys: {
            'quick_switcher.open': ['Ctrl+KeyP'],
            'history.back': [],
            'history.forward': ['Ctrl+Alt+ArrowRight'],
          },
        },
      }),
      switcher_search: () => ({ items: [] }),
    });

    expect(
      await screen.findByText('Выберите заметку в дереве файлов или откройте быстрый переход: Ctrl+P.'),
    ).toBeTruthy();
    expect(screen.queryByText(/Ctrl\+O/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Назад' }).getAttribute('title')).toBe('Назад');
    expect(screen.getByRole('button', { name: 'Вперёд (Ctrl+Alt+→)' })).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Быстрый переход (Ctrl+P)' }));
    const dialog = screen.getByRole('dialog', { name: 'Быстрый переход' });
    expect(within(dialog).getByText('Ctrl+P')).toBeTruthy();
    expect(within(dialog).queryByText('Ctrl+O')).toBeNull();
  });

  it('у быстрого перехода нет сочетания: подсказки без клавиш (задача 2.14)', async () => {
    await renderShell({
      ...WITH_VAULT,
      settings_read: () => ({ settings: { ...SETTINGS, hotkeys: { 'quick_switcher.open': [] } } }),
      switcher_search: () => ({ items: [] }),
    });

    expect(await screen.findByText('Выберите заметку в дереве файлов или откройте быстрый переход.')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Быстрый переход' }));
    const dialog = screen.getByRole('dialog', { name: 'Быстрый переход' });
    expect(dialog.querySelector('.shortcut')).toBeNull();
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
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('Б');

    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    const event = new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true, cancelable: true });
    screen.getByRole('combobox').dispatchEvent(event);

    expect(model.currentPath).toEqual(['Б.md']);
    // Действие браузера по умолчанию (Alt+← — «назад» в WebView) тоже гасится.
    expect(event.defaultPrevented).toBe(true);
  });

  it('пока открыто подтверждение удаления, Ctrl+O и Alt+←/→ не действуют', async () => {
    const vault = new FakeVault([
      { path: ['А.md'], kind: 'note' },
      { path: ['Б.md'], kind: 'note' },
    ]);
    const { model } = await renderShell({ ...WITH_VAULT, ...vault.mocks(), switcher_search: () => ({ items: [] }) });
    await screen.findByText('Нет открытой заметки');
    model.open(['А.md']);
    model.open(['Б.md']);
    await fireEvent.contextMenu(await screen.findByRole('treeitem', { name: 'А' }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /Удалить/ }));
    expect(screen.getByRole('dialog', { name: 'Удалить «А»?' })).toBeTruthy();

    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    const back = new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(back);

    expect(screen.queryByRole('dialog', { name: 'Быстрый переход' })).toBeNull();
    expect(model.currentPath).toEqual(['Б.md']);
    expect(back.defaultPrevented).toBe(true);
  });

  it('без хранилища быстрый переход не открывается', async () => {
    await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
      vault_list: () => ({ vaults: [], last_opened: null }),
    });
    await screen.findByText('Пока нет хранилищ');

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
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('Б');
    expect(back.disabled).toBe(false);
    expect(forward.disabled).toBe(true);

    await fireEvent.keyDown(window, { key: 'ArrowLeft', altKey: true });
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('А');
    expect(forward.disabled).toBe(false);

    await fireEvent.click(forward);
    await within(screen.getByRole('banner', { name: 'Заметка' })).findByText('Б');

    await waitFor(() => {
      const writes = calls.filter(([command]) => command === 'workspace_write');
      expect(writes.at(-1)?.[1]).toEqual({ request: { workspace: savedLayout(['Папка', 'Б.md']) } });
    });
  });
});

describe('сочетания браузерного движка (решение 94)', () => {
  function press(target: EventTarget, init: KeyboardEventInit): KeyboardEvent {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    return event;
  }

  /** Поле формы или текст заметки вне компонентов — для проверки обработчиков окна. */
  function field(html: string): HTMLElement {
    const host = document.createElement('div');
    host.innerHTML = html;
    document.body.appendChild(host);
    const element = host.firstElementChild as HTMLElement;
    element.focus();
    return element;
  }

  afterEach(() => {
    for (const host of document.body.querySelectorAll('[data-test-field]')) host.parentElement?.remove();
  });

  it('не назначенные Ctrl+P, Ctrl+F, F5, Ctrl+=, Ctrl+U, Ctrl+S, F12 гасятся и ничего не открывают', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');

    for (const init of [
      { key: 'p', code: 'KeyP', ctrlKey: true },
      { key: 'f', code: 'KeyF', ctrlKey: true },
      { key: 'F5', code: 'F5' },
      { key: '=', code: 'Equal', ctrlKey: true },
      { key: 'u', code: 'KeyU', ctrlKey: true },
      { key: 's', code: 'KeyS', ctrlKey: true },
      { key: 'F12', code: 'F12' },
    ]) {
      expect(press(document.body, init).defaultPrevented, init.code).toBe(true);
    }
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('Ctrl+P, назначенное быстрому переходу, открывает его — и из поля формы', async () => {
    await renderShell({
      ...WITH_VAULT,
      settings_read: () => ({ settings: { ...SETTINGS, hotkeys: { 'quick_switcher.open': ['Ctrl+KeyP'] } } }),
      switcher_search: () => ({ items: [] }),
    });
    await screen.findByText(/быстрый переход: Ctrl\+P/);

    const input = field('<input data-test-field aria-label="поле" />');
    expect(press(input, { key: 'з', code: 'KeyP', ctrlKey: true }).defaultPrevented).toBe(true);

    expect(await screen.findByRole('dialog', { name: 'Быстрый переход' })).toBeTruthy();
  });

  it('Ctrl+N без назначения не создаёт заметку и гасится', async () => {
    const { calls } = await renderShell({
      ...WITH_VAULT,
      settings_read: () => ({ settings: { ...SETTINGS, hotkeys: { 'note.new': [] } } }),
    });
    await screen.findByText('Нет открытой заметки');

    expect(press(document.body, { key: 'n', code: 'KeyN', ctrlKey: true }).defaultPrevented).toBe(true);

    expect(calls.map(([command]) => command)).not.toContain('name_check');
  });

  it('клавиши правки текста, Tab и Esc в поле и в тексте заметки не гасятся', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const input = field('<input data-test-field aria-label="поле" />');
    const text = field('<div data-test-field contenteditable="true">текст</div>');

    for (const target of [input, text]) {
      for (const init of [
        { key: 'c', code: 'KeyC', ctrlKey: true },
        { key: 'v', code: 'KeyV', ctrlKey: true },
        { key: 'x', code: 'KeyX', ctrlKey: true },
        { key: 'z', code: 'KeyZ', ctrlKey: true },
        { key: 'y', code: 'KeyY', ctrlKey: true },
        { key: 'a', code: 'KeyA', ctrlKey: true },
        { key: 'ArrowLeft', code: 'ArrowLeft', ctrlKey: true },
        { key: 'Backspace', code: 'Backspace', ctrlKey: true },
        { key: 'Delete', code: 'Delete', ctrlKey: true },
        { key: 'Tab', code: 'Tab' },
        { key: 'Escape', code: 'Escape' },
        { key: 'q', code: 'KeyQ', ctrlKey: true, altKey: true },
      ]) {
        expect(press(target, init).defaultPrevented, `${target.tagName} ${init.code}`).toBe(false);
      }
    }
  });

  it('элемент получает не назначенное сочетание первым: свой обработчик редактора работает', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const text = field('<div data-test-field contenteditable="true">текст</div>');
    let seenPrevented: boolean | null = null;
    text.addEventListener('keydown', (event) => (seenPrevented = event.defaultPrevented));

    const event = press(text, { key: 'f', code: 'KeyF', ctrlKey: true });

    expect(seenPrevented).toBe(false);
    expect(event.defaultPrevented).toBe(true);
  });

  it('в окне «Управление хранилищами» Ctrl+P тоже гасится', async () => {
    await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
      vault_list: () => ({ vaults: [], last_opened: null }),
    });
    await screen.findByText('Пока нет хранилищ');

    expect(press(document.body, { key: 'p', code: 'KeyP', ctrlKey: true }).defaultPrevented).toBe(true);
  });
});

describe('своё контекстное меню (решение 97)', () => {
  function rightClick(target: Element, x = 20, y = 20): MouseEvent {
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: x, clientY: y });
    target.dispatchEvent(event);
    return event;
  }

  afterEach(() => {
    for (const host of document.body.querySelectorAll('[data-test-host]')) host.remove();
  });

  it('в поле ввода — меню правки текста; Esc закрывает и возвращает фокус', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const host = document.createElement('div');
    host.dataset.testHost = '';
    host.innerHTML = '<input aria-label="поле" value="abc" />';
    document.body.appendChild(host);
    const input = host.querySelector('input') as HTMLInputElement;
    input.focus();
    input.setSelectionRange(0, 2);

    rightClick(input);

    const menu = await screen.findByRole('menu', { name: 'Правка текста' });
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'Вырезать Ctrl+X',
      'Копировать Ctrl+C',
      'Вставить Ctrl+V',
      'Выделить всё Ctrl+A',
    ]);
    // В окне хранилища «Вставить» активен (команда ядра, решение 98).
    expect(within(menu).getByRole('menuitem', { name: /Вставить/ }).getAttribute('aria-disabled')).toBeNull();
    expect(document.activeElement?.textContent).toContain('Вырезать');

    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toContain('Копировать');
    await fireEvent.keyDown(menu, { key: 'End' });
    expect(document.activeElement?.textContent).toContain('Выделить всё');

    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(input);
  });

  it('«Выделить всё» из меню выделяет значение поля', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const host = document.createElement('div');
    host.dataset.testHost = '';
    host.innerHTML = '<input aria-label="поле" value="abcdef" />';
    document.body.appendChild(host);
    const input = host.querySelector('input') as HTMLInputElement;

    rightClick(input);
    await fireEvent.click(await screen.findByRole('menuitem', { name: /Выделить всё/ }));

    expect(screen.queryByRole('menu')).toBeNull();
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 6]);
  });

  it('лента, шапка заметки и пустая область: правый щелчок ничего не открывает', async () => {
    await renderShell(WITH_VAULT);
    const empty = await screen.findByText('Нет открытой заметки');

    rightClick(screen.getByRole('toolbar', { name: 'Лента иконок' }));
    rightClick(screen.getByRole('banner', { name: 'Заметка' }));
    rightClick(empty);

    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('щелчок мимо закрывает меню', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const host = document.createElement('div');
    host.dataset.testHost = '';
    host.innerHTML = '<input aria-label="поле" value="abc" />';
    document.body.appendChild(host);

    rightClick(host.querySelector('input') as HTMLInputElement);
    await screen.findByRole('menu', { name: 'Правка текста' });
    await fireEvent.pointerDown(screen.getByRole('toolbar', { name: 'Лента иконок' }));

    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('меню правки текста в быстром переходе (решение 97)', () => {
  it('фокус переходит в меню и после Esc возвращается в поле; окно не закрывается', async () => {
    await renderShell({ ...WITH_VAULT, switcher_search: () => ({ items: [] }) });
    await screen.findByText('Нет открытой заметки');
    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    const input = await screen.findByRole('combobox', { name: 'Быстрый переход' });

    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 5, clientY: 5 }));
    const menu = await screen.findByRole('menu', { name: 'Правка текста' });
    expect(menu.contains(document.activeElement)).toBe(true);

    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Быстрый переход' })).toBeTruthy();
    expect(document.activeElement).toBe(input);
  });
});

describe('«Вставить» из своего меню (решение 98)', () => {
  function fieldWith(value: string): HTMLInputElement {
    const host = document.createElement('div');
    host.dataset.testHost = '';
    host.innerHTML = `<input aria-label="поле" value="${value}" />`;
    document.body.appendChild(host);
    return host.querySelector('input') as HTMLInputElement;
  }

  afterEach(() => {
    for (const host of document.body.querySelectorAll('[data-test-host]')) host.remove();
  });

  it('буфер читается только по щелчку по «Вставить», не при открытии меню', async () => {
    const { calls } = await renderShell({
      ...WITH_VAULT,
      clipboard_read_text: () => ({ outcome: 'text', text: 'XY' }),
    });
    await screen.findByText('Нет открытой заметки');
    const input = fieldWith('ab');
    input.setSelectionRange(1, 1);
    const reads = () => calls.filter(([command]) => command === 'clipboard_read_text').length;

    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 5, clientY: 5 }));
    await screen.findByRole('menu', { name: 'Правка текста' });
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowDown' });
    expect(reads()).toBe(0);

    await fireEvent.click(screen.getByRole('menuitem', { name: /Вставить/ }));

    await waitFor(() => expect(input.value).toBe('aXYb'));
    expect(reads()).toBe(1);
  });

  it('в окне «Управление хранилищами» «Вставить» неактивен (права команды нет)', async () => {
    await renderShell({
      vault_current: () => ({ api_version: 1, vault: null }),
      vault_list: () => ({ vaults: [], last_opened: null }),
    });
    await screen.findByText('Пока нет хранилищ');
    const input = fieldWith('ab');

    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 5, clientY: 5 }));

    const menu = await screen.findByRole('menu', { name: 'Правка текста' });
    expect(within(menu).getByRole('menuitem', { name: /Вставить/ }).getAttribute('aria-disabled')).toBe('true');
  });
});


describe('клавиши меню, когда фокус вернулся на место щелчка (WebKitGTK)', () => {
  it('Esc закрывает меню, стрелка уводит фокус в меню', async () => {
    await renderShell(WITH_VAULT);
    await screen.findByText('Нет открытой заметки');
    const host = document.createElement('div');
    host.dataset.testHost = '';
    host.innerHTML = '<input aria-label="поле" value="abc" />';
    document.body.appendChild(host);
    const input = host.querySelector('input') as HTMLInputElement;

    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 5, clientY: 5 }));
    const menu = await screen.findByRole('menu', { name: 'Правка текста' });
    input.focus();

    const down = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true });
    input.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(true);
    expect(menu.contains(document.activeElement)).toBe(true);

    input.focus();
    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    host.remove();
  });
});
