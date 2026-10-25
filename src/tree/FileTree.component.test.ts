// Дерево файлов (С3.2) на подмене команд ядра: по пункту спецификации на блок.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, TreeEntry, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import FileTree from './FileTree.svelte';
import { FakeVault } from './fakeVault.ts';
import { FileTreeState, type TreeHost } from './tree.svelte.ts';

const folder = (...path: string[]): TreeEntry => ({ kind: 'folder', path });
const note = (...path: string[]): TreeEntry => ({ kind: 'note', path });
const file = (...path: string[]): TreeEntry => ({ kind: 'attachment', path });

const SAMPLE = [
  folder('Проекты'),
  folder('Проекты', 'Альфа'),
  note('Проекты', 'Альфа', 'Встреча.md'),
  file('Проекты', 'Альфа', 'схема-процесса.png'),
  folder('Входящие'),
  note('Заметка 10.md'),
  note('Заметка 2.md'),
  file('ТЗ.pdf'),
  folder('.rhizomind'),
  note('.черновик.md'),
];

function fakeHost(overrides: Partial<TreeHost> = {}): TreeHost {
  return {
    currentPath: null,
    confirmDelete: true,
    open: vi.fn(),
    entryRenamed: vi.fn(),
    entryRemoved: vi.fn(),
    notify: vi.fn(),
    ...overrides,
  };
}

async function setup(entries: TreeEntry[] = SAMPLE, options: { host?: Partial<TreeHost>; mocks?: CoreMocks } = {}) {
  const vault = new FakeVault(entries);
  const calls = mockCore({ ...vault.mocks(), ...options.mocks });
  const host = fakeHost(options.host);
  const state = new FileTreeState(host);
  render(FileTree, { tree: state });
  const tree = screen.getByRole('tree', { name: 'Файлы хранилища' });
  await waitFor(() => expect(state.loaded).toBe(true));
  return { vault, calls, host, state, tree };
}

const rowNames = (tree: HTMLElement) =>
  within(tree)
    .queryAllByRole('treeitem')
    .map((row) => row.getAttribute('aria-label'));

const row = (name: string) => screen.getByRole('treeitem', { name });

const commands = (calls: RecordedCall[], name: string) =>
  calls.filter(([command]) => command === name).map(([, args]) => (args as { request?: unknown }).request);

async function expand(name: string) {
  const item = row(name);
  if (item.getAttribute('aria-expanded') !== 'true') await fireEvent.click(item);
}

function nameField(): HTMLInputElement {
  return screen.getByRole('textbox', { name: 'Новое имя' }) as HTMLInputElement;
}

async function type(value: string) {
  await fireEvent.input(nameField(), { target: { value } });
}

afterEach(() => {
  // Сначала размонтировать (дерево отписывается от событий), потом снять подмену.
  cleanup();
  clearMocks();
});

describe('С3.2: все файлы хранилища, скрытые не видны', () => {
  it('заметки без .md, вложения с меткой типа; .rhizomind и имена с точкой скрыты', async () => {
    const { tree } = await setup();

    expect(rowNames(tree)).toEqual(['Входящие', 'Проекты', 'Заметка 2', 'Заметка 10', 'ТЗ']);
    expect(within(row('ТЗ')).getByText('PDF')).toBeTruthy();
    await expand('Проекты');
    await expand('Альфа');
    expect(rowNames(tree)).toContain('схема-процесса');
    expect(within(row('схема-процесса')).getByText('PNG')).toBeTruthy();
  });

  it('пустое хранилище — подсказка', async () => {
    await setup([]);
    expect(screen.getByText('В хранилище пока нет файлов.')).toBeTruthy();
  });

  it('ошибка чтения дерева — уведомление с текстом из словаря', async () => {
    const failure: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    mockCore({
      tree_list: () => {
        throw failure;
      },
    });
    const host = fakeHost();
    render(FileTree, { tree: new FileTreeState(host) });
    await waitFor(() => expect(host.notify).toHaveBeenCalledWith('Не удалось прочитать или записать файл.'));
  });
});

describe('С3.2: сортировка — папки сверху, естественный порядок', () => {
  it('«Заметка 2» перед «Заметка 10», папки перед файлами на каждом уровне', async () => {
    const { tree } = await setup([
      folder('Б'),
      note('Б', 'Шаг 10.md'),
      note('Б', 'Шаг 9.md'),
      folder('Б', 'Яблоки'),
      note('а.md'),
      folder('Я'),
    ]);
    await expand('Б');
    expect(rowNames(tree)).toEqual(['Б', 'Яблоки', 'Шаг 9', 'Шаг 10', 'Я', 'а']);
  });
});

describe('открытие', () => {
  it('клик по заметке открывает её; по папке — раскрывает; вложение не открывается', async () => {
    const { host } = await setup();

    await fireEvent.click(row('Заметка 2'));
    expect(host.open).toHaveBeenCalledWith(['Заметка 2.md']);

    await fireEvent.click(row('ТЗ'));
    expect(host.open).toHaveBeenCalledTimes(1);
    expect(row('ТЗ').getAttribute('title')).toBe(
      'В этой версии открываются только заметки. Файл можно переименовать, переместить или удалить.',
    );

    expect(row('Проекты').getAttribute('aria-expanded')).toBe('false');
    await fireEvent.click(row('Проекты'));
    expect(row('Проекты').getAttribute('aria-expanded')).toBe('true');
    expect(row('Альфа')).toBeTruthy();
  });

  it('открытая заметка отмечена в дереве', async () => {
    await setup(SAMPLE, { host: { currentPath: ['Заметка 10.md'] } });
    expect(row('Заметка 10').getAttribute('aria-current')).toBe('page');
    expect(row('Заметка 2').getAttribute('aria-current')).toBeNull();
  });

  it('клавиши: стрелки двигают выбор, → раскрывает, Enter открывает', async () => {
    const { tree, host } = await setup();
    tree.focus();
    await fireEvent.keyDown(tree, { key: 'ArrowDown' });
    await fireEvent.keyDown(tree, { key: 'ArrowDown' });
    expect(row('Проекты').getAttribute('aria-selected')).toBe('true');
    await fireEvent.keyDown(tree, { key: 'ArrowRight' });
    expect(row('Проекты').getAttribute('aria-expanded')).toBe('true');
    await fireEvent.keyDown(tree, { key: 'ArrowLeft' });
    expect(row('Проекты').getAttribute('aria-expanded')).toBe('false');
    await fireEvent.keyDown(tree, { key: 'ArrowDown' });
    await fireEvent.keyDown(tree, { key: 'Enter' });
    expect(host.open).toHaveBeenCalledWith(['Заметка 2.md']);
  });
});

describe('С3.2: кнопки при наведении и создание как в Obsidian', () => {
  it('наведение на папку показывает «Новая заметка» и «Новая папка»; на файл — нет', async () => {
    await setup();
    const projects = row('Проекты');
    expect(within(projects).queryByRole('button', { name: 'Новая заметка' })).toBeNull();

    await fireEvent.mouseEnter(projects);
    expect(within(projects).getByRole('button', { name: 'Новая заметка' })).toBeTruthy();
    expect(within(projects).getByRole('button', { name: 'Новая папка' })).toBeTruthy();

    await fireEvent.mouseLeave(projects);
    expect(within(projects).queryByRole('button', { name: 'Новая заметка' })).toBeNull();

    await fireEvent.mouseEnter(row('Заметка 2'));
    expect(within(row('Заметка 2')).queryByRole('button', { name: 'Новая заметка' })).toBeNull();
  });

  it('«Новая заметка» на папке: «Без названия» в этой папке, заметка открыта, поле имени открыто', async () => {
    const { calls, host, tree } = await setup();
    await fireEvent.mouseEnter(row('Проекты'));
    await fireEvent.click(within(row('Проекты')).getByRole('button', { name: 'Новая заметка' }));

    await waitFor(() => expect(nameField()).toBeTruthy());
    expect(commands(calls, 'note_create')).toEqual([{ dir: ['Проекты'], name: 'Без названия.md' }]);
    expect(host.open).toHaveBeenCalledWith(['Проекты', 'Без названия.md']);
    expect(nameField().value).toBe('Без названия');
    expect(document.activeElement).toBe(nameField());
    // Папка раскрыта, новая заметка в ней.
    expect(row('Проекты').getAttribute('aria-expanded')).toBe('true');
    expect(within(tree).getAllByRole('treeitem').length).toBeGreaterThan(5);
  });

  it('наведение на пустое место дерева: кнопки создают в корне; занятое имя — следующее свободное', async () => {
    const { calls } = await setup([note('Без названия.md'), folder('Без названия')]);
    const empty = screen.getByTestId('tree-root-zone');
    await fireEvent.mouseEnter(empty);

    await fireEvent.click(within(empty).getByRole('button', { name: 'Новая заметка' }));
    await waitFor(() => expect(nameField()).toBeTruthy());
    expect(commands(calls, 'note_create')).toEqual([{ dir: [], name: 'Без названия 1.md' }]);
    await fireEvent.keyDown(nameField(), { key: 'Escape' });

    await fireEvent.mouseEnter(empty);
    await fireEvent.click(within(empty).getByRole('button', { name: 'Новая папка' }));
    await waitFor(() => expect(nameField().value).toBe('Без названия 1'));
    expect(commands(calls, 'folder_create')).toEqual([{ dir: [], name: 'Без названия 1' }]);
  });

  it('ядро отказало «занято» (дерево отстало от диска) — берётся следующее имя', async () => {
    let attempts = 0;
    const { calls } = await setup([], {
      mocks: {
        note_create: ({ dir, name }) => {
          attempts += 1;
          if (attempts === 1) {
            throw { code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: 'duplicate' } satisfies CoreError;
          }
          return { path: [...dir, name] };
        },
      },
    });
    await fireEvent.mouseEnter(screen.getByTestId('tree-root-zone'));
    await fireEvent.click(screen.getByRole('button', { name: 'Новая заметка' }));
    await waitFor(() => expect(nameField()).toBeTruthy());
    expect(commands(calls, 'note_create').map((r) => (r as { name: string }).name)).toEqual([
      'Без названия.md',
      'Без названия 1.md',
    ]);
  });
});

describe('С3.2: контекстное меню', () => {
  it('на папке: новая заметка, новая папка, переименовать, удалить; на файле — без создания', async () => {
    await setup();
    await fireEvent.contextMenu(row('Проекты'));
    let menu = screen.getByRole('menu', { name: 'Действия с файлами' });
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent?.trim())).toEqual([
      'Новая заметка',
      'Новая папка',
      'Переименовать F2',
      'Удалить Delete',
    ]);
    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();

    await fireEvent.contextMenu(row('ТЗ'));
    menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent?.trim())).toEqual([
      'Переименовать F2',
      'Удалить Delete',
    ]);
  });

  it('на пустом месте: создание в корне', async () => {
    const { calls } = await setup();
    await fireEvent.contextMenu(screen.getByTestId('tree-root-zone'));
    const menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent?.trim())).toEqual([
      'Новая заметка',
      'Новая папка',
    ]);
    await fireEvent.click(within(menu).getByRole('menuitem', { name: 'Новая папка' }));
    await waitFor(() => expect(nameField()).toBeTruthy());
    expect(commands(calls, 'folder_create')).toEqual([{ dir: [], name: 'Без названия' }]);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('пункт «Переименовать» открывает поле имени, «Удалить» — подтверждение', async () => {
    await setup();
    await fireEvent.contextMenu(row('Заметка 2'));
    await fireEvent.click(screen.getByRole('menuitem', { name: /Переименовать/ }));
    expect(nameField().value).toBe('Заметка 2');
    await fireEvent.keyDown(nameField(), { key: 'Escape' });

    await fireEvent.contextMenu(row('Заметка 2'));
    await fireEvent.click(screen.getByRole('menuitem', { name: /Удалить/ }));
    expect(screen.getByRole('dialog', { name: 'Удалить «Заметка 2»?' })).toBeTruthy();
  });

  it('клавиша контекстного меню открывает меню выбранной строки', async () => {
    const { tree } = await setup();
    await fireEvent.click(row('ТЗ'));
    await fireEvent.keyDown(tree, { key: 'ContextMenu' });
    expect(screen.getByRole('menu')).toBeTruthy();
  });
});

describe('С3.2: переименование (F2) с проверкой имени командой name_check', () => {
  async function startRename(name: string) {
    const ctx = await setup();
    await fireEvent.click(row(name));
    await fireEvent.keyDown(ctx.tree, { key: 'F2' });
    return ctx;
  }

  it('F2: поле с именем без расширения; Enter — entry_rename с .md, новое имя в дереве', async () => {
    const { calls, host, tree } = await startRename('Заметка 2');
    expect(nameField().value).toBe('Заметка 2');

    await type('Встреча 03.11');
    await waitFor(() =>
      expect(commands(calls, 'name_check').at(-1)).toEqual({
        dir: [],
        name: 'Встреча 03.11.md',
        renaming: ['Заметка 2.md'],
      }),
    );
    await fireEvent.keyDown(nameField(), { key: 'Enter' });

    await waitFor(() => expect(screen.queryByRole('textbox')).toBeNull());
    expect(commands(calls, 'entry_rename')).toEqual([{ path: ['Заметка 2.md'], new_name: 'Встреча 03.11.md' }]);
    expect(host.entryRenamed).toHaveBeenCalledWith(['Заметка 2.md'], ['Встреча 03.11.md']);
    expect(rowNames(tree)).toContain('Встреча 03.11');
    expect(rowNames(tree)).not.toContain('Заметка 2');
  });

  it.each([
    ['План: работ', 'В имени нельзя использовать символы \\ / : * ? " < > |'],
    ['CON', 'Это имя зарезервировано в Windows (CON, PRN, AUX, NUL, COM1, LPT1 и подобные).'],
    ['заметка 10', 'В этой папке уже есть элемент с таким именем.'],
  ])('«%s» — пояснение под полем, Enter не срабатывает', async (value, message) => {
    const { calls } = await startRename('Заметка 2');
    await type(value);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent?.trim()).toBe(message);
    expect(nameField().getAttribute('aria-invalid')).toBe('true');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    expect(commands(calls, 'entry_rename')).toEqual([]);
    expect(nameField()).toBeTruthy();
  });

  it('папка «Итог.» — точка в конце запрещена (у заметки после имени идёт .md)', async () => {
    const { calls, tree } = await setup();
    await fireEvent.click(row('Входящие'));
    await fireEvent.keyDown(tree, { key: 'F2' });
    await type('Итог.');
    const alert = await screen.findByRole('alert');
    expect(alert.textContent?.trim()).toBe('Имя не может заканчиваться точкой или пробелом.');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    expect(commands(calls, 'entry_rename')).toEqual([]);
  });

  it.each([
    ['#идея', 'На эту заметку нельзя будет сослаться.'],
    ['.скрытая', 'Имя с точкой в начале: элемент будет скрыт в дереве файлов.'],
  ])('«%s» — предупреждение, но переименовать можно', async (value, warning) => {
    const { calls } = await startRename('Заметка 2');
    await type(value);
    const status = await screen.findByRole('status');
    await waitFor(() => expect(status.textContent?.trim()).toBe(warning));
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await waitFor(() => expect(commands(calls, 'entry_rename')).toHaveLength(1));
  });

  it('смена только регистра разрешена (переименование самого себя)', async () => {
    const { calls } = await startRename('Заметка 2');
    await type('заметка 2');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await waitFor(() =>
      expect(commands(calls, 'entry_rename')).toEqual([{ path: ['Заметка 2.md'], new_name: 'заметка 2.md' }]),
    );
  });

  it('Esc отменяет; имя без изменений — без вызова ядра', async () => {
    const { calls } = await startRename('Заметка 2');
    await type('Другое');
    await fireEvent.keyDown(nameField(), { key: 'Escape' });
    expect(screen.queryByRole('textbox')).toBeNull();

    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await waitFor(() => expect(screen.queryByRole('textbox')).toBeNull());
    expect(commands(calls, 'entry_rename')).toEqual([]);
  });

  it('уход фокуса сохраняет допустимое имя', async () => {
    const { calls } = await startRename('Заметка 2');
    await type('Новое');
    await fireEvent.blur(nameField());
    await waitFor(() => expect(commands(calls, 'entry_rename')).toHaveLength(1));
  });

  it('вложение: правится имя без расширения, расширение сохраняется', async () => {
    const { calls } = await startRename('ТЗ');
    expect(nameField().value).toBe('ТЗ');
    await type('ТЗ-версия-2');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await waitFor(() =>
      expect(commands(calls, 'entry_rename')).toEqual([{ path: ['ТЗ.pdf'], new_name: 'ТЗ-версия-2.pdf' }]),
    );
  });

  it('переименование папки: путь открытой заметки внутри меняется у оболочки', async () => {
    const { host } = await startRename('Проекты');
    await type('Работа');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await waitFor(() => expect(host.entryRenamed).toHaveBeenCalledWith(['Проекты'], ['Работа']));
  });

  it('ядро отклонило имя при сохранении — пояснение под полем, поле остаётся', async () => {
    const { calls } = await setup(SAMPLE, {
      mocks: {
        entry_rename: () => {
          throw { code: 'already_exists', message_key: 'errors.already_exists' } satisfies CoreError;
        },
      },
    });
    await fireEvent.click(row('Заметка 2'));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await type('Новое');
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent?.trim()).toBe('Файл или папка с таким именем уже есть.');
    expect(nameField()).toBeTruthy();
    expect(commands(calls, 'entry_rename')).toHaveLength(1);
  });

  it('двойной клик по строке тоже открывает поле имени (Э4)', async () => {
    await setup();
    await fireEvent.dblClick(row('ТЗ'));
    expect(nameField().value).toBe('ТЗ');
  });

  it('устаревший ответ name_check не затирает свежий', async () => {
    const answers: ((value: unknown) => void)[] = [];
    await setup(SAMPLE, {
      mocks: {
        name_check: () => new Promise((resolve) => answers.push(resolve as (value: unknown) => void)) as never,
      },
    });
    await fireEvent.click(row('Заметка 2'));
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await type('а:');
    await type('аб');
    await waitFor(() => expect(answers).toHaveLength(2));
    answers[1]?.({ status: 'ok', warning: null });
    answers[0]?.({ status: 'rejected', problem: 'forbidden_chars', message_key: 'names.forbidden_chars' });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});

describe('С3.2: перемещение перетаскиванием', () => {
  async function drag(source: HTMLElement, target: HTMLElement) {
    await fireEvent.dragStart(source);
    await fireEvent.dragEnter(target);
    await fireEvent.dragOver(target);
    await fireEvent.drop(target);
    await fireEvent.dragEnd(source);
  }

  it('заметку на папку: entry_move, дерево и оболочка узнают новый путь', async () => {
    const { calls, host, tree } = await setup();
    await drag(row('Заметка 2'), row('Входящие'));
    await waitFor(() => expect(commands(calls, 'entry_move')).toEqual([{ path: ['Заметка 2.md'], new_dir: ['Входящие'] }]));
    await waitFor(() => expect(host.entryRenamed).toHaveBeenCalledWith(['Заметка 2.md'], ['Входящие', 'Заметка 2.md']));
    expect(rowNames(tree)).not.toContain('Заметка 2');
    await expand('Входящие');
    expect(rowNames(tree)).toContain('Заметка 2');
  });

  it('на файл — в папку этого файла; на пустое место — в корень', async () => {
    const { calls } = await setup();
    await expand('Проекты');
    await expand('Альфа');
    await drag(row('ТЗ'), row('Встреча'));
    await waitFor(() =>
      expect(commands(calls, 'entry_move')).toEqual([{ path: ['ТЗ.pdf'], new_dir: ['Проекты', 'Альфа'] }]),
    );
    await drag(row('схема-процесса'), screen.getByTestId('tree-root-zone'));
    await waitFor(() => expect(commands(calls, 'entry_move')).toHaveLength(2));
    expect(commands(calls, 'entry_move')[1]).toEqual({ path: ['Проекты', 'Альфа', 'схема-процесса.png'], new_dir: [] });
  });

  it('в ту же папку и папку внутрь себя — ничего не происходит', async () => {
    const { calls } = await setup();
    await expand('Проекты');
    await drag(row('Заметка 2'), row('Заметка 10'));
    await drag(row('Проекты'), row('Альфа'));
    await drag(row('Проекты'), row('Проекты'));
    expect(commands(calls, 'entry_move')).toEqual([]);
  });

  it('ошибка ядра — уведомление', async () => {
    const { host } = await setup([folder('А'), note('А', 'x.md'), note('x.md')]);
    await drag(row('x'), row('А'));
    await waitFor(() => expect(host.notify).toHaveBeenCalledWith('Файл или папка с таким именем уже есть.'));
  });
});

describe('С3.2: удаление в корзину', () => {
  it('Delete → подтверждение → entry_trash; элемент исчезает, оболочка закрывает заметку', async () => {
    const { calls, host, tree } = await setup();
    await fireEvent.click(row('Заметка 2'));
    await fireEvent.keyDown(tree, { key: 'Delete' });

    const dialog = screen.getByRole('dialog', { name: 'Удалить «Заметка 2»?' });
    expect(within(dialog).getByText('Файл будет перемещён в корзину.')).toBeTruthy();
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Удалить' }));

    await waitFor(() => expect(commands(calls, 'entry_trash')).toEqual([{ path: ['Заметка 2.md'] }]));
    await waitFor(() => expect(rowNames(tree)).not.toContain('Заметка 2'));
    expect(host.entryRemoved).toHaveBeenCalledWith(['Заметка 2.md']);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('папка — свой текст; «Отмена» и Esc ничего не удаляют', async () => {
    const { calls, tree } = await setup();
    await fireEvent.click(row('Входящие'));
    await fireEvent.keyDown(tree, { key: 'Delete' });
    const dialog = screen.getByRole('dialog', { name: 'Удалить папку «Входящие»?' });
    expect(within(dialog).getByText('Папка и всё, что в ней, будут перемещены в корзину.')).toBeTruthy();
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Отмена' }));
    expect(screen.queryByRole('dialog')).toBeNull();

    await fireEvent.keyDown(tree, { key: 'Delete' });
    await fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(commands(calls, 'entry_trash')).toEqual([]);
  });

  it('подтверждение выключено в настройках — сразу в корзину', async () => {
    const { calls, tree } = await setup(SAMPLE, { host: { confirmDelete: false } });
    await fireEvent.click(row('ТЗ'));
    await fireEvent.keyDown(tree, { key: 'Delete' });
    expect(screen.queryByRole('dialog')).toBeNull();
    await waitFor(() => expect(commands(calls, 'entry_trash')).toEqual([{ path: ['ТЗ.pdf'] }]));
  });

  it('диск без корзины: ядро спросило само; удалено безвозвратно — уведомление; отказ — элемент на месте', async () => {
    const { vault, host, tree } = await setup(SAMPLE, { host: { confirmDelete: false } });
    vault.trashOutcome = { outcome: 'deleted_permanently' };
    await fireEvent.click(row('ТЗ'));
    await fireEvent.keyDown(tree, { key: 'Delete' });
    await waitFor(() => expect(host.notify).toHaveBeenCalledWith('«ТЗ.pdf» удалено безвозвратно: на этом диске нет корзины.'));
    expect(rowNames(tree)).not.toContain('ТЗ');

    vault.trashOutcome = { outcome: 'cancelled' };
    await fireEvent.click(row('Заметка 2'));
    await fireEvent.keyDown(tree, { key: 'Delete' });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(rowNames(tree)).toContain('Заметка 2');
    expect(host.entryRemoved).toHaveBeenCalledTimes(1);
  });
});

describe('С3.6: обновление по событию vault:changed', () => {
  it('создание, удаление, переименование снаружи видны в дереве', async () => {
    const { tree } = await setup();
    await emitCoreEvent('vault:changed', {
      rescan: false,
      changes: [
        { kind: 'created', path: ['Новая.md'], entry_kind: 'note' },
        { kind: 'removed', path: ['ТЗ.pdf'] },
        { kind: 'renamed', from: ['Заметка 2.md'], to: ['Входящие', 'Заметка 2.md'] },
        { kind: 'modified', path: ['Заметка 10.md'] },
      ],
    });
    await waitFor(() => expect(rowNames(tree)).toEqual(['Входящие', 'Проекты', 'Заметка 10', 'Новая']));
  });

  it('rescan: true — дерево перечитывается командой tree_list', async () => {
    const { vault, calls, tree } = await setup();
    vault.entries.clear();
    vault.entries.set('Только.md', note('Только.md'));
    await emitCoreEvent('vault:changed', { rescan: true, changes: [] });
    await waitFor(() => expect(rowNames(tree)).toEqual(['Только']));
    expect(commands(calls, 'tree_list')).toHaveLength(2);
  });

  it('переименование неизвестного элемента — тоже перечитать', async () => {
    const { calls } = await setup();
    await emitCoreEvent('vault:changed', { rescan: false, changes: [{ kind: 'renamed', from: ['нет.md'], to: ['да.md'] }] });
    await waitFor(() => expect(commands(calls, 'tree_list')).toHaveLength(2));
  });

  it('после размонтирования дерево отписывается от события', async () => {
    const vault = new FakeVault(SAMPLE);
    const calls = mockCore(vault.mocks());
    const state = new FileTreeState(fakeHost());
    const { unmount } = render(FileTree, { tree: state });
    await waitFor(() => expect(state.loaded).toBe(true));
    // Подписка асинхронна: дождаться её, затем размонтировать.
    await waitFor(() => expect(calls.some(([command]) => command === 'plugin:event|listen')).toBe(true));
    unmount();
    await emitCoreEvent('vault:changed', { rescan: true, changes: [] });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(commands(calls, 'tree_list')).toHaveLength(1);
  });
});

describe('путь в хранилище не собирается из строки', () => {
  it('имена с пробелами, точками и кириллицей уходят ядру компонентами', async () => {
    const path: VaultPath = ['Папка. v2', 'Имя с  пробелами.md'];
    const { calls, host } = await setup([folder('Папка. v2'), note(...path)]);
    await expand('Папка. v2');
    // Имя для чтения с экрана — с одним пробелом (так его нормализует Testing Library).
    await fireEvent.click(row('Имя с пробелами'));
    expect(host.open).toHaveBeenCalledWith(path);
    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'Delete' });
    await fireEvent.click(screen.getByRole('button', { name: 'Удалить' }));
    await waitFor(() => expect(commands(calls, 'entry_trash')).toEqual([{ path }]));
  });
});
