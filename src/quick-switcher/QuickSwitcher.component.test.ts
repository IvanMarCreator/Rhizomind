import { clearMocks } from '@tauri-apps/api/mocks';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, NameCheck, Settings, SwitcherItem, VaultPath } from '../api/index.ts';
import { mockCore, type CommandMock, type CoreMocks } from '../api/testing.ts';
import QuickSwitcher from './QuickSwitcher.svelte';

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
};

const NOTES: SwitcherItem[] = [
  { name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] },
  { name: 'Встреча', path: ['Проекты', 'Бета', 'Встреча.md'] },
  { name: 'Встреча по запуску', path: ['Проекты', 'Альфа', 'Встреча по запуску.md'] },
  { name: 'Итоги', path: ['Итоги.md'] },
];

/** Подменный поиск: заметки, в имени которых есть запрос (пустой — все). */
function search(notes: SwitcherItem[] = NOTES): CommandMock<'switcher_search'> {
  return ({ query }) => ({
    items: notes.filter((note) => note.name.toLowerCase().includes(query.trim().toLowerCase())),
  });
}

function setup(mocks: CoreMocks = {}, currentPath: VaultPath | null = null) {
  const calls = mockCore({ switcher_search: search(), ...mocks });
  const onOpen = vi.fn<(path: VaultPath) => void>();
  const onClose = vi.fn<() => void>();
  const onNotice = vi.fn<(text: string) => void>();
  render(QuickSwitcher, { currentPath, onOpen, onClose, onNotice });
  const input = screen.getByRole('combobox', { name: 'Быстрый переход' }) as HTMLInputElement;
  return { calls, onOpen, onClose, onNotice, input };
}

function options(): HTMLElement[] {
  return within(screen.getByRole('listbox')).queryAllByRole('option');
}

function selected(): HTMLElement | undefined {
  return options().find((option) => option.getAttribute('aria-selected') === 'true');
}

async function type(input: HTMLInputElement, value: string) {
  await fireEvent.input(input, { target: { value } });
}

const searches = (calls: [string, unknown][]) =>
  calls.filter(([command]) => command === 'switcher_search').map(([, args]) => (args as { request: { query: string } }).request.query);

afterEach(() => {
  clearMocks();
});

describe('быстрый переход: поиск', () => {
  it('окно с полем в фокусе; при открытии — заметки по пустому запросу', async () => {
    const { input, calls } = setup();

    expect(screen.getByRole('dialog', { name: 'Быстрый переход' })).toBeTruthy();
    expect(document.activeElement).toBe(input);
    await waitFor(() => expect(options()).toHaveLength(4));
    expect(searches(calls)).toEqual(['']);
    // Пустой запрос — создавать нечего.
    expect(screen.queryByText(/Создать заметку/)).toBeNull();
  });

  it('пункт: имя слева, папки справа; одинаковые имена различаются путём', async () => {
    setup();
    await waitFor(() => expect(options()).toHaveLength(4));

    const [alpha, beta, , root] = options();
    expect(alpha?.querySelector('.name')?.textContent).toBe('Встреча');
    expect(alpha?.querySelector('.folders')?.textContent).toBe('Проекты / Альфа');
    expect(beta?.querySelector('.folders')?.textContent).toBe('Проекты / Бета');
    expect(root?.querySelector('.folders')).toBeNull();
  });

  it('набор текста ищет через ядро и подсвечивает совпадения', async () => {
    const { input, calls } = setup();
    await type(input, 'встр');

    await waitFor(() => expect(options()).toHaveLength(4));
    expect(searches(calls).at(-1)).toBe('встр');
    const marks = [...(options()[0]?.querySelectorAll('mark') ?? [])].map((mark) => mark.textContent);
    expect(marks).toEqual(['Встр']);
  });

  it('нет заметки с таким именем — последний пункт «Создать заметку „…“»', async () => {
    const { input } = setup();
    await type(input, 'встр');

    await waitFor(() => expect(options().at(-1)?.textContent).toContain('Создать заметку „встр“'));
    // Выбран первый найденный, а не создание.
    expect(selected()?.textContent).toContain('Встреча');
  });

  it('заметка с таким именем есть — создание не предлагается', async () => {
    const { input } = setup();
    await type(input, 'итоги');

    await waitFor(() => expect(options()).toHaveLength(1));
    expect(screen.queryByText(/Создать заметку/)).toBeNull();
  });

  it('ничего не найдено — выбран пункт создания', async () => {
    const { input } = setup();
    await type(input, 'Новая мысль');

    await waitFor(() => expect(options()).toHaveLength(1));
    expect(selected()?.textContent).toContain('Создать заметку „Новая мысль“');
  });

  it('ошибка поиска — текст из словаря в окне', async () => {
    const failure: CoreError = { code: 'no_vault_open', message_key: 'errors.no_vault_open' };
    setup({
      switcher_search: () => {
        throw failure;
      },
    });

    expect(await screen.findByText('В этом окне не открыто хранилище.')).toBeTruthy();
  });

  it('запрос длиннее предела обрезается до 256 символов', async () => {
    const { input, calls } = setup();
    await type(input, 'я'.repeat(300));

    await waitFor(() => expect(searches(calls).at(-1)).toHaveLength(256));
  });
});

describe('быстрый переход: клавиатура и мышь', () => {
  it('стрелки двигают выбор по кругу, Enter открывает заметку и закрывает окно', async () => {
    const { input, onOpen, onClose } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));
    expect(selected()).toBe(options()[0]);
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[0]?.id);

    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(selected()).toBe(options()[1]);
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[1]?.id);

    await fireEvent.keyDown(input, { key: 'ArrowUp' });
    await fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(selected()).toBe(options()[3]);
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(selected()).toBe(options()[0]);

    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(onOpen).toHaveBeenCalledWith(['Проекты', 'Бета', 'Встреча.md']);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('Esc закрывает окно без перехода', async () => {
    const { input, onOpen, onClose } = setup();
    await fireEvent.keyDown(input, { key: 'Escape' });

    expect(onClose).toHaveBeenCalledOnce();
    expect(onOpen).not.toHaveBeenCalled();
  });

  it('клик по затемнению закрывает окно, клик внутри — нет', async () => {
    const { onClose } = setup();
    await fireEvent.mouseDown(screen.getByRole('dialog'));
    expect(onClose).not.toHaveBeenCalled();

    await fireEvent.mouseDown(screen.getByTestId('quick-switcher-scrim'));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('клик по пункту открывает заметку', async () => {
    const { onOpen } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));

    await fireEvent.click(options()[3] as HTMLElement);

    expect(onOpen).toHaveBeenCalledWith(['Итоги.md']);
  });

  it('Enter во время набора через IME не открывает заметку', async () => {
    const { input, onOpen } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));

    await fireEvent.keyDown(input, { key: 'Enter', isComposing: true });

    expect(onOpen).not.toHaveBeenCalled();
  });

  it('Enter до ответа ядра ничего не открывает и не создаёт', async () => {
    const { input, onOpen, calls } = setup({ switcher_search: () => new Promise(() => {}) });
    await type(input, 'встр');
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(onOpen).not.toHaveBeenCalled();
    expect(calls.some(([command]) => command === 'note_create')).toBe(false);
  });
});

describe('быстрый переход: Tab дополняет имя (решение 51)', () => {
  it('Tab подставляет полное имя выбранного пункта, поиск идёт по нему, выбор остаётся на пункте', async () => {
    const { input, calls, onOpen } = setup();
    await type(input, 'встр');
    await waitFor(() => expect(options()).toHaveLength(4));
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(selected()?.textContent).toContain('Встреча по запуску');

    await fireEvent.keyDown(input, { key: 'Tab' });

    expect(input.value).toBe('Встреча по запуску');
    expect(document.activeElement).toBe(input);
    expect(searches(calls).at(-1)).toBe('Встреча по запуску');
    await waitFor(() => expect(options()).toHaveLength(1));
    expect(selected()?.textContent).toContain('Встреча по запуску');
    // Имя совпало с заметкой — создание не предлагается.
    expect(screen.queryByText(/Создать заметку/)).toBeNull();

    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledWith(['Проекты', 'Альфа', 'Встреча по запуску.md']);
  });

  it('одинаковые имена: после Tab выбрана та же заметка (по пути), Enter открывает её', async () => {
    const { input, onOpen } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(selected()?.querySelector('.folders')?.textContent).toBe('Проекты / Бета');

    await fireEvent.keyDown(input, { key: 'Tab' });

    expect(input.value).toBe('Встреча');
    await waitFor(() => expect(options()).toHaveLength(3));
    expect(selected()?.querySelector('.folders')?.textContent).toBe('Проекты / Бета');
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledWith(['Проекты', 'Бета', 'Встреча.md']);
  });

  it('Tab на пункте «Создать заметку» ничего не меняет', async () => {
    const { input, calls } = setup();
    await type(input, 'Новая мысль');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку „Новая мысль“'));
    const before = searches(calls).length;

    const event = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    input.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(input.value).toBe('Новая мысль');
    expect(searches(calls)).toHaveLength(before);
    expect(document.activeElement).toBe(input);
  });

  it('Tab и Shift+Tab не уводят фокус из поля; Shift+Tab ничего не подставляет', async () => {
    const { input } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));

    const shiftTab = new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true });
    input.dispatchEvent(shiftTab);
    expect(shiftTab.defaultPrevented).toBe(true);
    expect(input.value).toBe('');

    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    input.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input);
  });

  it('клик по пустому месту окна не забирает фокус у поля', async () => {
    setup();
    const event = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    screen.getByRole('dialog').dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it('подсказка внизу окна: «Tab дополнить»', () => {
    setup();
    expect(screen.getByText('дополнить').closest('span')?.textContent).toBe('Tabдополнить');
  });
});

describe('быстрый переход: косая черта в запросе (решение 50)', () => {
  it('пункт создания неактивен и сразу поясняет; Enter и клик ничего не создают', async () => {
    const { input, calls, onOpen, onClose } = setup({
      settings_read: () => ({ settings: SETTINGS }),
      name_check: () => ({ status: 'ok', warning: null }),
      note_create: ({ dir, name }) => ({ path: [...dir, name] }),
    });
    await type(input, 'Проекты/План');
    await waitFor(() => expect(options()).toHaveLength(1));

    const item = options()[0] as HTMLElement;
    expect(item.getAttribute('aria-disabled')).toBe('true');
    expect(item.textContent).toContain('Создать заметку „Проекты/План“');
    expect(item.textContent).toContain('Косая черта в имени недопустима: сначала создайте папку');

    await fireEvent.keyDown(input, { key: 'Enter' });
    await fireEvent.click(item);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(calls.some(([command]) => command === 'name_check' || command === 'note_create')).toBe(false);
    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('обратная косая черта — так же', async () => {
    const { input } = setup();
    await type(input, 'А\\Б');
    await waitFor(() => expect(options()[0]?.getAttribute('aria-disabled')).toBe('true'));
  });
});

describe('быстрый переход: создание заметки', () => {
  const created: CoreMocks = {
    settings_read: () => ({ settings: SETTINGS }),
    name_check: () => ({ status: 'ok', warning: null }),
    note_create: ({ dir, name }) => ({ path: [...dir, name] }),
  };

  it('Enter на пункте создания: проверка имени, создание в корне, переход к заметке', async () => {
    const { input, calls, onOpen, onClose } = setup(created);
    await type(input, '  Новая мысль ');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку „Новая мысль“'));

    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(onOpen).toHaveBeenCalledWith(['Новая мысль.md']));
    expect(onClose).toHaveBeenCalledOnce();
    const requests = calls.filter(([command]) => command === 'name_check' || command === 'note_create');
    expect(requests).toEqual([
      ['name_check', { request: { dir: [], name: 'Новая мысль.md' } }],
      ['note_create', { request: { dir: [], name: 'Новая мысль.md' } }],
    ]);
  });

  it('настройка «в папке открытой заметки» — заметка рядом с открытой', async () => {
    const { input, onOpen } = setup(
      {
        ...created,
        settings_read: () => ({ settings: { ...SETTINGS, new_note_location: { kind: 'current_folder' } } }),
      },
      ['Проекты', 'Альфа', 'Встреча.md'],
    );
    await type(input, 'План');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку „План“'));

    await fireEvent.click(selected() as HTMLElement);

    await waitFor(() => expect(onOpen).toHaveBeenCalledWith(['Проекты', 'Альфа', 'План.md']));
  });

  it('недопустимое имя — пояснение под полем, заметка не создаётся, окно открыто', async () => {
    const { input, calls, onOpen, onClose } = setup({
      ...created,
      name_check: () => ({ status: 'rejected', problem: 'forbidden_chars', message_key: 'names.forbidden_chars' }),
    });
    await type(input, 'А:Б');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку'));

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByText('В имени нельзя использовать символы \\ / : * ? " < > |')).toBeTruthy();
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(calls.some(([command]) => command === 'note_create')).toBe(false);
    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();

    // Новый ввод убирает пояснение.
    await type(input, 'АБ');
    await waitFor(() => expect(screen.queryByText(/В имени нельзя/)).toBeNull());
  });

  it('имя с предупреждением — заметка создаётся, предупреждение уведомлением', async () => {
    const { input, onOpen, onNotice } = setup({
      ...created,
      name_check: () => ({ status: 'ok', warning: 'unlinkable' }),
    });
    await type(input, '#тег');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку „#тег“'));

    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(onOpen).toHaveBeenCalledWith(['#тег.md']));
    expect(onNotice).toHaveBeenCalledWith('На эту заметку нельзя будет сослаться.');
  });

  it('ошибка создания — текст из словаря под полем, окно открыто', async () => {
    const failure: CoreError = { code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: 'duplicate' };
    const { input, onClose } = setup({
      ...created,
      note_create: () => {
        throw failure;
      },
    });
    await type(input, 'Новая');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку'));

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByText('В этой папке уже есть элемент с таким именем.')).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe('быстрый переход: закрытие во время создания', () => {
  /** Команда, которая отвечает, когда тест скажет. */
  function held<T>() {
    let release: (value: T) => void = () => {};
    const promise = new Promise<T>((resolve) => (release = resolve));
    return { promise, release: (value: T) => release(value) };
  }

  const base: CoreMocks = {
    settings_read: () => ({ settings: SETTINGS }),
    note_create: ({ dir, name }) => ({ path: [...dir, name] }),
  };

  async function startCreate(input: HTMLInputElement) {
    await type(input, 'Новая');
    await waitFor(() => expect(selected()?.textContent).toContain('Создать заметку „Новая“'));
    await fireEvent.keyDown(input, { key: 'Enter' });
  }

  it('Esc во время проверки имени — заметка не создаётся, окно закрывается один раз', async () => {
    const check = held<NameCheck>();
    const { input, calls, onOpen, onClose } = setup({ ...base, name_check: () => check.promise });
    await startCreate(input);
    await waitFor(() => expect(calls.some(([command]) => command === 'name_check')).toBe(true));

    await fireEvent.keyDown(input, { key: 'Escape' });
    check.release({ status: 'ok', warning: 'unlinkable' });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(calls.some(([command]) => command === 'note_create')).toBe(false);
    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('окно убрали (оболочка) во время проверки имени — заметка не создаётся', async () => {
    const check = held<NameCheck>();
    const calls = mockCore({ switcher_search: search(), ...base, name_check: () => check.promise });
    const onOpen = vi.fn<(path: VaultPath) => void>();
    const { unmount } = render(QuickSwitcher, { currentPath: null, onOpen, onClose: () => {}, onNotice: () => {} });
    const input = screen.getByRole('combobox') as HTMLInputElement;
    await startCreate(input);
    await waitFor(() => expect(calls.some(([command]) => command === 'name_check')).toBe(true));

    unmount();
    check.release({ status: 'ok', warning: null });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(calls.some(([command]) => command === 'note_create')).toBe(false);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it('закрыли, пока заметка создаётся, — она создана, но переход и уведомление не выполняются', async () => {
    const created = held<{ path: VaultPath }>();
    const { input, calls, onOpen, onClose, onNotice } = setup({
      ...base,
      name_check: () => ({ status: 'ok', warning: 'unlinkable' }),
      note_create: () => created.promise,
    });
    await startCreate(input);
    await waitFor(() => expect(calls.some(([command]) => command === 'note_create')).toBe(true));

    await fireEvent.mouseDown(screen.getByTestId('quick-switcher-scrim'));
    created.release({ path: ['Новая.md'] });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onOpen).not.toHaveBeenCalled();
    expect(onNotice).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe('быстрый переход: скорость интерфейса', () => {
  it('быстрый набор: в полёте один запрос, на экране — ответ на последний текст', async () => {
    const waiting: (() => void)[] = [];
    const { input, calls } = setup({
      switcher_search: (request) =>
        new Promise((resolve) => waiting.push(() => resolve(search()(request)))),
    });
    // Ответ на пустой запрос при открытии ещё не пришёл.
    for (const value of ['в', 'вс', 'вст', 'итог']) await type(input, value);
    expect(searches(calls)).toEqual(['']);

    waiting.shift()?.();
    await waitFor(() => expect(searches(calls)).toEqual(['', 'итог']));
    // Ответ на пустой запрос устарел и не показан.
    expect(options()).toHaveLength(0);

    waiting.shift()?.();
    await waitFor(() => expect(options()[0]?.textContent).toContain('Итоги'));
    expect(options().at(-1)?.textContent).toContain('Создать заметку „итог“');
  });

  it('запрос уходит в том же обработчике ввода — без дебаунса и таймеров', async () => {
    const { input, calls } = setup();
    await waitFor(() => expect(options()).toHaveLength(4));

    // Событие ввода без ожидания: вызов ядра должен случиться синхронно.
    input.value = 'итог';
    input.dispatchEvent(new Event('input', { bubbles: true }));

    expect(searches(calls)).toEqual(['', 'итог']);
  });
});
