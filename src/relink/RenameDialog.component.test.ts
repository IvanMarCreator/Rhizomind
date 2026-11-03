// Окно вопроса при переименовании и перемещении в приложении (0.2-5.2;
// С2-3.5 «в приложении», «Ctrl+Z», С2-3.7; решения 109, 114, 116, Р9; макет
// Д-А и Д-1): дерево спрашивает `relink_preview`, по настройке `rename_links`
// показывает окно или переименовывает сразу, затем `entry_rename`/`entry_move`
// с `links` и знаком плана.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, RenameLinks, TreeEntry, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import FileTree from '../tree/FileTree.svelte';
import NoteTitle from '../tree/NoteTitle.svelte';
import { FakeVault } from '../tree/fakeVault.ts';
import { FileTreeState, type TreeHost } from '../tree/tree.svelte.ts';
import RenameDialog from './RenameDialog.svelte';

const folder = (...path: string[]): TreeEntry => ({ kind: 'folder', path });
const note = (...path: string[]): TreeEntry => ({ kind: 'note', path });

const ENTRIES = [
  folder('Проекты'),
  folder('Проекты', 'Альфа'),
  note('Проекты', 'Альфа', 'План.md'),
  folder('Бета'),
  note('Встреча.md'),
  note('Дневник.md'),
  note('Итоги.md'),
  note('Без ссылок.md'),
];

const LINKED = {
  'Дневник.md': 'Обсудили [[Встреча]] и [[Проекты/Альфа/План]].\n',
  'Итоги.md': 'См. [[Встреча#Итоги|итоги]]\n',
};

interface Options {
  mode?: RenameLinks;
  mocks?: CoreMocks | ((vault: FakeVault) => CoreMocks);
  building?: { done: number; total: number };
}

async function setup(options: Options = {}) {
  const vault = new FakeVault(ENTRIES);
  for (const [name, text] of Object.entries(LINKED)) vault.texts.set(name, text);
  if (options.building) vault.building = options.building;
  const extra = typeof options.mocks === 'function' ? options.mocks(vault) : options.mocks;
  const calls = mockCore({ ...vault.mocks(), ...extra });
  let mode: RenameLinks = options.mode ?? 'ask';
  const setRenameLinks = vi.fn((next: RenameLinks) => {
    mode = next;
  });
  const host: TreeHost = {
    currentPath: null,
    confirmDelete: true,
    get renameLinks() {
      return mode;
    },
    setRenameLinks,
    open: vi.fn(),
    entryRenamed: vi.fn(),
    entryRemoved: vi.fn(),
    notify: vi.fn(),
  };
  const state = new FileTreeState(host);
  render(FileTree, { tree: state });
  render(RenameDialog, { tree: state });
  await waitFor(() => expect(state.loaded).toBe(true));
  return { vault, calls, host, state, setRenameLinks };
}

const commands = (calls: RecordedCall[], name: string) =>
  calls.filter(([command]) => command === name).map(([, args]) => (args as { request?: unknown }).request);

const row = (name: string) => screen.getByRole('treeitem', { name });
const tree = () => screen.getByRole('tree', { name: 'Файлы хранилища' });
const nameField = () => screen.getByRole('textbox', { name: 'Новое имя' }) as HTMLInputElement;
const queryDialog = () => screen.queryByRole('dialog');

/** F2 на строке, новое имя, Enter. */
async function renameRow(name: string, value: string) {
  await fireEvent.click(row(name));
  await fireEvent.keyDown(tree(), { key: 'F2' });
  await fireEvent.input(nameField(), { target: { value } });
  await fireEvent.keyDown(nameField(), { key: 'Enter' });
}

async function expand(name: string) {
  const item = row(name);
  if (item.getAttribute('aria-expanded') !== 'true') await fireEvent.click(item);
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('С2-3.5: переименование в приложении по настройке', () => {
  it('переписывать нечего — без окна', async () => {
    const { calls, host } = await setup();
    await renameRow('Без ссылок', 'Пусто');

    await waitFor(() => expect(row('Пусто')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'relink_preview')).toEqual([{ op: 'rename', path: ['Без ссылок.md'], new_name: 'Пусто.md' }]);
    // Без обновления ссылок — запрос как в 0.1, без поля `links`.
    expect(commands(calls, 'entry_rename')).toEqual([{ path: ['Без ссылок.md'], new_name: 'Пусто.md' }]);
    expect(host.entryRenamed).toHaveBeenCalledWith(['Без ссылок.md'], ['Пусто.md']);
  });

  it('Всегда обновлять — без окна, ссылки обновлены', async () => {
    const { calls, vault } = await setup({ mode: 'update' });
    await renameRow('Встреча', 'Планёрка');

    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    const [request] = commands(calls, 'entry_rename') as { links?: string; token?: string }[];
    expect(request).toMatchObject({ path: ['Встреча.md'], new_name: 'Планёрка.md', links: 'update' });
    expect(request?.token).toMatch(/^[0-9a-f]{32}$/);
    expect(vault.text(['Дневник.md'])).toBe('Обсудили [[Планёрка]] и [[Проекты/Альфа/План]].\n');
    expect(vault.text(['Итоги.md'])).toBe('См. [[Планёрка#Итоги|итоги]]\n');
  });

  it('Всегда спрашивать — окно до переименования; «Обновить ссылки» по умолчанию', async () => {
    const { calls, vault } = await setup();
    await renameRow('Встреча', 'Планёрка');

    const box = await screen.findByRole('dialog', { name: 'На «Встреча» ссылаются 2 заметки. Обновить ссылки?' });
    // До ответа файл не переименован.
    expect(commands(calls, 'entry_rename')).toEqual([]);
    expect(within(box).getByText('Встреча')).toBeTruthy();
    expect(within(box).getByText('Планёрка')).toBeTruthy();
    expect(within(box).getByText('Если не обновить, ссылки останутся на старом имени и станут неразрешёнными.')).toBeTruthy();
    const update = within(box).getByRole('button', { name: 'Обновить ссылки' });
    await waitFor(() => expect(document.activeElement).toBe(update));
    // Enter — кнопка по умолчанию.
    await fireEvent.keyDown(update, { key: 'Enter' });
    await fireEvent.click(update);

    await waitFor(() => expect(queryDialog()).toBeNull());
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(commands(calls, 'entry_rename')).toHaveLength(1);
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ links: 'update' });
    expect(vault.text(['Дневник.md'])).toContain('[[Планёрка]]');
  });

  it('Переименовать без обновления — ссылки не тронуты', async () => {
    const { calls, vault } = await setup();
    await renameRow('Встреча', 'Планёрка');
    await fireEvent.click(await within(await screen.findByRole('dialog')).findByRole('button', { name: 'Переименовать без обновления' }));

    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'entry_rename')).toEqual([{ path: ['Встреча.md'], new_name: 'Планёрка.md' }]);
    expect(vault.text(['Дневник.md'])).toBe(LINKED['Дневник.md']);
  });

  it('Отменить переименование — ничего не изменилось (кнопка, крестик, Esc)', async () => {
    const { calls, vault, host } = await setup();
    for (const close of ['button', 'cross', 'escape'] as const) {
      await renameRow('Встреча', 'Планёрка');
      const box = await screen.findByRole('dialog');
      if (close === 'button') await fireEvent.click(within(box).getByRole('button', { name: 'Отменить переименование' }));
      if (close === 'cross') await fireEvent.click(within(box).getByRole('button', { name: 'Закрыть' }));
      if (close === 'escape') await fireEvent.keyDown(box, { key: 'Escape' });
      await waitFor(() => expect(queryDialog()).toBeNull());
      // Поле имени закрыто, имя прежнее.
      await waitFor(() => expect(screen.queryByRole('textbox', { name: 'Новое имя' })).toBeNull());
      expect(row('Встреча')).toBeTruthy();
    }
    expect(commands(calls, 'entry_rename')).toEqual([]);
    expect(vault.text(['Дневник.md'])).toBe(LINKED['Дневник.md']);
    expect(host.entryRenamed).not.toHaveBeenCalled();
  });

  it('Больше не спрашивать + Обновить — настройка стала update; следующее — без окна', async () => {
    const { calls, setRenameLinks } = await setup();
    await renameRow('Встреча', 'Планёрка');
    const box = await screen.findByRole('dialog');
    await fireEvent.click(within(box).getByRole('checkbox', { name: 'Больше не спрашивать' }));
    expect(within(box).getByText('С ответом «Обновить ссылки» настройка хранилища переключится на «Всегда обновлять ссылки».')).toBeTruthy();
    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));

    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(setRenameLinks).toHaveBeenCalledWith('update');
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ links: 'update' });

    await renameRow('Планёрка', 'Встреча');
    await waitFor(() => expect(row('Встреча')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'entry_rename')[1]).toMatchObject({ links: 'update' });
  });

  it('решение 116: с галочкой «Переименовать без обновления» недоступна, подсказка «Чтобы не обновлять ссылки, снимите галочку»', async () => {
    const { setRenameLinks } = await setup();
    await renameRow('Встреча', 'Планёрка');
    const box = await screen.findByRole('dialog');
    const keep = within(box).getByRole('button', { name: 'Переименовать без обновления' }) as HTMLButtonElement;
    expect(keep.disabled).toBe(false);
    expect(within(box).queryByText('Чтобы не обновлять ссылки, снимите галочку')).toBeNull();

    const check = within(box).getByRole('checkbox', { name: 'Больше не спрашивать' });
    await fireEvent.click(check);
    expect(keep.disabled).toBe(true);
    const hint = within(box).getByText('Чтобы не обновлять ссылки, снимите галочку');
    expect(keep.getAttribute('aria-describedby')).toBe(hint.id);

    // Сняли галочку — снова доступна; настройка не менялась.
    await fireEvent.click(check);
    expect(keep.disabled).toBe(false);
    expect(setRenameLinks).not.toHaveBeenCalled();
  });
});

describe('решение 114: пока индекс строится — «Подождите, индекс строится…»', () => {
  it('Всегда спрашивать: окно ожидания, кнопки недоступны, фокус на «Отменить переименование»; индекс готов — кнопки доступны', async () => {
    const { calls, vault } = await setup({ building: { done: 62, total: 100 } });
    await renameRow('Встреча', 'Планёрка');

    const box = await screen.findByRole('dialog', { name: 'Подождите, индекс строится…' });
    expect(within(box).getByText('Индекс строится… 62 %')).toBeTruthy();
    expect(within(box).getByRole('progressbar').getAttribute('aria-valuenow')).toBe('62');
    expect((within(box).getByRole('button', { name: 'Обновить ссылки' }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(box).getByRole('button', { name: 'Переименовать без обновления' }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(box).getByRole('checkbox', { name: 'Больше не спрашивать' }) as HTMLInputElement).disabled).toBe(true);
    const cancel = within(box).getByRole('button', { name: 'Отменить переименование' });
    await waitFor(() => expect(document.activeElement).toBe(cancel));

    // Ход сборки обновляется по `index:status`.
    await emitCoreEvent('index:status', { state: 'building', done: 80, total: 100 });
    await within(box).findByText('Индекс строится… 80 %');

    // После `ready` индекс ещё сверяется: предпросмотр снова `index_building` — окно ждёт дальше (ревью 0.2-4.6).
    vault.building = { done: 100, total: 100 };
    await emitCoreEvent('index:status', { state: 'ready', done: 100, total: 100 });
    await waitFor(() => expect(commands(calls, 'relink_preview').length).toBeGreaterThanOrEqual(2));
    expect(screen.getByRole('dialog', { name: 'Подождите, индекс строится…' })).toBeTruthy();

    vault.building = null;
    await emitCoreEvent('index:status', { state: 'ready', done: 100, total: 100 });
    await screen.findByRole('dialog', { name: 'На «Встреча» ссылаются 2 заметки. Обновить ссылки?' });
    expect(commands(calls, 'entry_rename')).toEqual([]);
    const update = screen.getByRole('button', { name: 'Обновить ссылки' }) as HTMLButtonElement;
    expect(update.disabled).toBe(false);
    await fireEvent.click(update);
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ links: 'update' });
  });

  it('Всегда обновлять: окно ожидания только с «Отменить переименование»; индекс готов — переименование и обновление идут сами', async () => {
    const { calls, vault } = await setup({ mode: 'update', building: { done: 0, total: 0 } });
    await renameRow('Встреча', 'Планёрка');

    const box = await screen.findByRole('dialog', { name: 'Подождите, индекс строится…' });
    expect(within(box).getByText('Индекс строится…')).toBeTruthy();
    expect(
      within(box).getByText(
        'Переименование и обновление ссылок выполнятся сами, когда индекс будет готов (настройка «Всегда обновлять ссылки»).',
      ),
    ).toBeTruthy();
    expect(within(box).getAllByRole('button').map((button) => button.textContent?.trim() || button.getAttribute('aria-label'))).toEqual([
      'Закрыть',
      'Отменить переименование',
    ]);

    vault.building = null;
    await emitCoreEvent('index:status', { state: 'ready', done: 10, total: 10 });
    await waitFor(() => expect(queryDialog()).toBeNull());
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ links: 'update' });
    expect(vault.text(['Дневник.md'])).toContain('[[Планёрка]]');
  });

  it('отмена во время ожидания — ничего не переименовано, даже когда индекс готов', async () => {
    const { calls, vault } = await setup({ mode: 'update', building: { done: 1, total: 4 } });
    await renameRow('Встреча', 'Планёрка');
    await fireEvent.click(await screen.findByRole('button', { name: 'Отменить переименование' }));
    await waitFor(() => expect(queryDialog()).toBeNull());

    vault.building = null;
    await emitCoreEvent('index:status', { state: 'ready', done: 4, total: 4 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(commands(calls, 'entry_rename')).toEqual([]);
    expect(row('Встреча')).toBeTruthy();
  });
});

describe('Р9: Ctrl+Z после переименования', () => {
  it('Ctrl+Z — обратное переименование без второго окна, ссылки вернулись', async () => {
    const { calls, vault } = await setup();
    await renameRow('Встреча', 'Планёрка');
    await fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Обновить ссылки' }));
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(vault.text(['Итоги.md'])).toBe('См. [[Планёрка#Итоги|итоги]]\n');

    await fireEvent.keyDown(tree(), { key: 'z', code: 'KeyZ', ctrlKey: true });

    await waitFor(() => expect(row('Встреча')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'relink_preview').at(-1)).toEqual({ op: 'rename', path: ['Планёрка.md'], new_name: 'Встреча.md' });
    expect(commands(calls, 'entry_rename').at(-1)).toMatchObject({ path: ['Планёрка.md'], new_name: 'Встреча.md', links: 'update' });
    expect(vault.text(['Дневник.md'])).toBe(LINKED['Дневник.md']);
    expect(vault.text(['Итоги.md'])).toBe(LINKED['Итоги.md']);
  });

  it('ответ «без обновления» действует и на Ctrl+Z: обратно без обновления ссылок', async () => {
    const { calls } = await setup();
    await renameRow('Встреча', 'Планёрка');
    await fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Переименовать без обновления' }));
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());

    await fireEvent.keyDown(tree(), { key: 'z', code: 'KeyZ', ctrlKey: true });
    await waitFor(() => expect(row('Встреча')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'entry_rename').at(-1)).toEqual({ path: ['Планёрка.md'], new_name: 'Встреча.md' });
  });

  it('Ctrl+Z на заголовке заметки — тот же путь', async () => {
    const { calls, state, vault } = await setup();
    const path: VaultPath = ['Встреча.md'];
    render(NoteTitle, { tree: state, path });
    await fireEvent.click(screen.getByRole('button', { name: 'Встреча' }));
    await fireEvent.input(nameField(), { target: { value: 'Планёрка' } });
    await fireEvent.keyDown(nameField(), { key: 'Enter' });
    await fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Обновить ссылки' }));
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ links: 'update' });
    expect(vault.text(['Дневник.md'])).toContain('[[Планёрка]]');
  });
});

describe('папка и перетаскивание — то же окно', () => {
  it('папка — то же окно со своим текстом', async () => {
    const { calls, vault } = await setup();
    await expand('Проекты');
    await renameRow('Альфа', 'Альфа-2026');

    const box = await screen.findByRole('dialog', { name: 'На заметки в папке «Альфа» ссылается 1 заметка. Обновить ссылки?' });
    expect(within(box).getByText('Проекты/Альфа')).toBeTruthy();
    expect(within(box).getByText('Проекты/Альфа-2026')).toBeTruthy();
    expect(
      within(box).getByText(
        'Обновятся ссылки с путём через эту папку и ссылки, которые иначе повели бы не туда. Если не обновить, ссылки останутся на старом имени и станут неразрешёнными.',
      ),
    ).toBeTruthy();
    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));
    await waitFor(() => expect(row('Альфа-2026')).toBeTruthy());
    expect(commands(calls, 'entry_rename')[0]).toMatchObject({ path: ['Проекты', 'Альфа'], new_name: 'Альфа-2026', links: 'update' });
    expect(vault.text(['Дневник.md'])).toContain('[[Проекты/Альфа-2026/План]]');
  });

  it('перетаскивание — то же окно; ответ — entry_move с links', async () => {
    const { calls } = await setup();
    const source = row('Встреча');
    await fireEvent.dragStart(source);
    await fireEvent.dragOver(row('Бета'));
    await fireEvent.drop(row('Бета'));

    const box = await screen.findByRole('dialog', { name: 'На «Встреча» ссылаются 2 заметки. Обновить ссылки?' });
    expect(commands(calls, 'relink_preview')).toEqual([{ op: 'move', path: ['Встреча.md'], new_dir: ['Бета'] }]);
    expect(within(box).getByText('Бета/Встреча')).toBeTruthy();
    expect(commands(calls, 'entry_move')).toEqual([]);
    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));
    await waitFor(() => expect(commands(calls, 'entry_move')).toHaveLength(1));
    expect(commands(calls, 'entry_move')[0]).toMatchObject({ path: ['Встреча.md'], new_dir: ['Бета'], links: 'update' });
  });

  it('перетаскивание и «Отменить» — файл на месте', async () => {
    const { calls } = await setup();
    await fireEvent.dragStart(row('Встреча'));
    await fireEvent.dragOver(row('Бета'));
    await fireEvent.drop(row('Бета'));
    await fireEvent.click(await screen.findByRole('button', { name: 'Отменить переименование' }));
    await waitFor(() => expect(queryDialog()).toBeNull());
    expect(commands(calls, 'entry_move')).toEqual([]);
  });
});

describe('ошибки (ревью 0.2-4.6)', () => {
  it('io_failed при обновлении ссылок: понятный текст под полем — переименование не выполнено, ввод не записан', async () => {
    const failure: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    await setup({
      mode: 'update',
      mocks: {
        entry_rename: () => {
          throw failure;
        },
      },
    });
    await renameRow('Встреча', 'Планёрка');
    const alert = await screen.findByRole('alert');
    expect(alert.textContent?.trim()).toBe(
      'Переименование не выполнено: не удалось записать набранный текст или файл занят. Ничего не изменилось, текст остался в редакторе — попробуйте ещё раз.',
    );
    // Поле остаётся открытым с введённым именем: можно повторить Enter.
    expect(nameField().value).toBe('Планёрка');
    expect(row('Встреча')).toBeTruthy();
  });

  it('знак плана просрочен (окно висело дольше 10 минут) — новый предпросмотр и то же решение без второго окна', async () => {
    let first = true;
    const { calls } = await setup({
      mocks: (vault) => {
        const rename = vault.mocks().entry_rename;
        if (!rename) throw new Error('нет entry_rename');
        return {
          entry_rename: (request) => {
            if (first && request.links === 'update') {
              first = false;
              throw { code: 'invalid_argument', message_key: 'errors.invalid_argument', field: 'token' } satisfies CoreError;
            }
            return rename(request);
          },
        };
      },
    });
    await renameRow('Встреча', 'Планёрка');
    await fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Обновить ссылки' }));
    await waitFor(() => expect(row('Планёрка')).toBeTruthy());
    expect(queryDialog()).toBeNull();
    expect(commands(calls, 'relink_preview')).toHaveLength(2);
    expect(commands(calls, 'entry_rename')).toHaveLength(2);
  });

  it('предпросмотр отказал (имя занято) — текст под полем, окна нет', async () => {
    await setup({
      mocks: {
        relink_preview: () => {
          throw { code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: 'duplicate' } satisfies CoreError;
        },
      },
    });
    await renameRow('Встреча', 'Планёрка');
    const alert = await screen.findByRole('alert');
    expect(alert.textContent?.trim()).toBe('В этой папке уже есть элемент с таким именем.');
    expect(queryDialog()).toBeNull();
  });
});
