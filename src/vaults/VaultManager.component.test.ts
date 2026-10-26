// Окно «Управление хранилищами» (задача 2.8а; макеты Э11а–Э11в) на подмене
// команд ядра.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, VaultListItem, VaultListResponse } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import VaultManager from './VaultManager.svelte';

const WORK: VaultListItem = { id: 'a1', name: 'Рабочая база', path: 'D:\\Заметки\\Рабочая база', is_open: true };
const RULES: VaultListItem = { id: 'b2', name: 'Регламенты отдела', path: 'D:\\Отдел\\Регламенты', is_open: false };

function list(vaults: VaultListItem[]): () => VaultListResponse {
  return () => ({ vaults, last_opened: vaults[0]?.id ?? null });
}

function renderManager(mocks: CoreMocks) {
  const calls = mockCore(mocks);
  const onNotice = vi.fn<(text: string) => void>();
  render(VaultManager, { onNotice });
  return { calls, onNotice };
}

const commands = (calls: [string, unknown][]) => calls.map(([command]) => command);

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('окно «Управление хранилищами»', () => {
  it('первый запуск (Э11б): список пуст, справа — создать и открыть папку', async () => {
    renderManager({ vault_list: list([]) });

    expect(await screen.findByText('Пока нет хранилищ')).toBeTruthy();
    expect(screen.getByText('Создайте новое хранилище или откройте папку с заметками — справа.')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Rhizomind' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Создать' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Открыть папку как хранилище' })).toBeTruthy();
    expect(screen.queryByRole('listitem')).toBeNull();
    expect(screen.getByText('Версия 0.1.0 · редакция Secure')).toBeTruthy();
    expect(screen.getByText(/Работает без сети/)).toBeTruthy();
  });

  it('список (Э11а): имя, путь, метка «открыто» у открытого хранилища', async () => {
    renderManager({ vault_list: list([WORK, RULES]) });

    const [work, rules, ...rest] = await screen.findAllByRole('listitem');
    expect(rest).toHaveLength(0);
    if (!work || !rules) throw new Error('нет строк списка');
    expect(within(work).getByText('Рабочая база')).toBeTruthy();
    expect(within(work).getByText('D:\\Заметки\\Рабочая база')).toBeTruthy();
    expect(within(work).getByText('открыто')).toBeTruthy();
    expect(within(rules).queryByText('открыто')).toBeNull();
    expect(screen.queryByText('Пока нет хранилищ')).toBeNull();
  });

  it('«Открыть» у хранилища — vault_open с его идентификатором', async () => {
    const { calls } = renderManager({ vault_list: list([WORK, RULES]), vault_open: () => ({ outcome: 'opened' }) });

    await fireEvent.click(await screen.findByRole('button', { name: 'Открыть хранилище «Регламенты отдела»' }));

    await waitFor(() => expect(calls).toContainEqual(['vault_open', { request: { id: 'b2' } }]));
  });

  it('щелчок по строке тоже открывает хранилище', async () => {
    const { calls } = renderManager({ vault_list: list([RULES]), vault_open: () => ({ outcome: 'opened' }) });

    await fireEvent.click(await screen.findByText('Регламенты отдела'));

    await waitFor(() => expect(commands(calls)).toContain('vault_open'));
  });

  it('ошибка открытия — уведомление текстом словаря, список перечитывается', async () => {
    const unavailable: CoreError = { code: 'vault_unavailable', message_key: 'errors.vault_unavailable' };
    const { calls, onNotice } = renderManager({
      vault_list: list([RULES]),
      vault_open: () => {
        throw unavailable;
      },
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'Открыть хранилище «Регламенты отдела»' }));

    await waitFor(() =>
      expect(onNotice).toHaveBeenCalledWith('Папка хранилища недоступна. Проверьте, подключён ли диск.'),
    );
    expect(commands(calls).filter((c) => c === 'vault_list')).toHaveLength(2);
  });

  it('«…» → «Убрать из списка»: vault_remove_from_list, файлы не удаляются, список обновлён', async () => {
    let vaults = [WORK, RULES];
    const { calls, onNotice } = renderManager({
      vault_list: () => ({ vaults, last_opened: null }),
      vault_remove_from_list: ({ id }) => {
        vaults = vaults.filter((vault) => vault.id !== id);
        return null;
      },
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'Действия с хранилищем «Регламенты отдела»' }));
    const menu = screen.getByRole('menu');
    expect(within(menu).getByText('Файлы и папка на диске не удаляются')).toBeTruthy();
    await fireEvent.click(within(menu).getByRole('menuitem', { name: /Убрать из списка/ }));

    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(1));
    expect(calls).toContainEqual(['vault_remove_from_list', { request: { id: 'b2' } }]);
    expect(onNotice).toHaveBeenCalledWith('Хранилище «Регламенты отдела» убрано из списка. Файлы на диске остались.');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('Esc закрывает меню «…»', async () => {
    renderManager({ vault_list: list([RULES]) });
    await fireEvent.click(await screen.findByRole('button', { name: 'Действия с хранилищем «Регламенты отдела»' }));

    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });

    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('«Открыть папку как хранилище» — диалог выбора папки в ядре (vault_open_folder)', async () => {
    const { calls, onNotice } = renderManager({
      vault_list: list([]),
      vault_open_folder: () => ({ outcome: 'cancelled' }),
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'Открыть папку как хранилище' }));

    await waitFor(() => expect(commands(calls)).toContain('vault_open_folder'));
    expect(onNotice).not.toHaveBeenCalled();
  });

  describe('создание (Э11в)', () => {
    async function openForm(mocks: CoreMocks) {
      const result = renderManager({ vault_list: list([]), ...mocks });
      await fireEvent.click(await screen.findByRole('button', { name: 'Создать' }));
      const field = screen.getByRole('textbox', { name: 'Имя хранилища' });
      return { ...result, field };
    }

    it('«Создать» открывает форму: имя в фокусе, пустое имя создать нельзя', async () => {
      const { field } = await openForm({});

      expect(screen.getByRole('heading', { name: 'Создать хранилище' })).toBeTruthy();
      expect(document.activeElement).toBe(field);
      const submit = screen.getByRole('button', { name: 'Создать' });
      expect((submit as HTMLButtonElement).disabled).toBe(true);
      expect(screen.getByText(/выберите в системном окне папку/)).toBeTruthy();
    });

    it('имя → «Создать»: vault_create с именем (папку спрашивает ядро)', async () => {
      const vault = { id: 'c3', name: 'Проекты отдела', path: 'D:\\Заметки\\Проекты отдела' };
      const { calls, field } = await openForm({ vault_create: () => ({ outcome: 'opened', vault }) });

      await fireEvent.input(field, { target: { value: 'Проекты отдела' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));

      await waitFor(() => expect(calls).toContainEqual(['vault_create', { request: { name: 'Проекты отдела' } }]));
    });

    it('Enter в поле имени создаёт хранилище', async () => {
      const { calls, field } = await openForm({ vault_create: () => ({ outcome: 'cancelled' }) });

      await fireEvent.input(field, { target: { value: 'Новое' } });
      await fireEvent.keyDown(field, { key: 'Enter' });

      await waitFor(() => expect(commands(calls)).toContain('vault_create'));
    });

    it('недопустимое имя — пояснение под полем, форма остаётся', async () => {
      const reserved: CoreError = {
        code: 'invalid_name',
        message_key: 'errors.invalid_name',
        name_problem: 'reserved_name',
      };
      const { field, onNotice } = await openForm({
        vault_create: () => {
          throw reserved;
        },
      });

      await fireEvent.input(field, { target: { value: 'CON' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));

      const alert = await screen.findByRole('alert');
      expect(alert.textContent).toContain('Это имя зарезервировано в Windows');
      expect(field.getAttribute('aria-invalid')).toBe('true');
      expect(onNotice).not.toHaveBeenCalled();

      // Правка имени убирает пояснение.
      await fireEvent.input(field, { target: { value: 'Работа' } });
      expect(screen.queryByRole('alert')).toBeNull();
    });

    it('папка уже есть — пояснение под полем', async () => {
      const exists: CoreError = { code: 'already_exists', message_key: 'errors.already_exists' };
      const { field } = await openForm({
        vault_create: () => {
          throw exists;
        },
      });

      await fireEvent.input(field, { target: { value: 'Работа' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));

      expect((await screen.findByRole('alert')).textContent).toBe('Файл или папка с таким именем уже есть.');
    });

    it('другая ошибка — уведомление', async () => {
      const failed: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
      const { field, onNotice } = await openForm({
        vault_create: () => {
          throw failed;
        },
      });

      await fireEvent.input(field, { target: { value: 'Работа' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));

      await waitFor(() => expect(onNotice).toHaveBeenCalledWith('Не удалось прочитать или записать файл.'));
    });

    it('«Отмена», «Назад» и Esc возвращают к действиям', async () => {
      await openForm({});
      await fireEvent.click(screen.getByRole('button', { name: 'Отмена' }));
      expect(screen.queryByRole('textbox')).toBeNull();

      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));
      await fireEvent.click(screen.getByRole('button', { name: 'Назад' }));
      expect(screen.queryByRole('textbox')).toBeNull();

      await fireEvent.click(screen.getByRole('button', { name: 'Создать' }));
      await fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
      expect(screen.queryByRole('textbox')).toBeNull();
      expect(screen.getByRole('button', { name: 'Открыть папку как хранилище' })).toBeTruthy();
    });
  });

  it('список не прочитался — уведомление, окно работает', async () => {
    const failed: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    const { onNotice } = renderManager({
      vault_list: () => {
        throw failed;
      },
    });

    await waitFor(() => expect(onNotice).toHaveBeenCalledWith('Не удалось прочитать или записать файл.'));
    expect(screen.getByRole('button', { name: 'Создать' })).toBeTruthy();
  });
});
