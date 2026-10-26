// Переключатель хранилищ слева внизу (Э10б, задача 2.8а) на подмене команд ядра.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, VaultListItem } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import VaultSwitcher from './VaultSwitcher.svelte';

const VAULTS: VaultListItem[] = [
  { id: 'a1', name: 'Рабочая база', path: 'D:\\Заметки\\Рабочая база', is_open: true },
  { id: 'b2', name: 'Регламенты отдела', path: 'D:\\Отдел\\Регламенты', is_open: false },
  { id: 'c3', name: 'личные заметки', path: 'C:\\Заметки', is_open: false },
];

function renderSwitcher(mocks: CoreMocks) {
  const calls = mockCore({ vault_list: () => ({ vaults: VAULTS, last_opened: 'a1' }), ...mocks });
  const onNotice = vi.fn<(text: string) => void>();
  render(VaultSwitcher, { vault: { id: 'a1', name: 'Рабочая база' }, onNotice });
  return { calls, onNotice };
}

async function openMenu() {
  await fireEvent.click(screen.getByRole('button', { name: 'Хранилище «Рабочая база»' }));
  return screen.findByRole('menu', { name: 'Хранилища' });
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('переключатель хранилищ', () => {
  it('показывает имя и букву текущего хранилища; меню закрыто', () => {
    renderSwitcher({});

    const button = screen.getByRole('button', { name: 'Хранилище «Рабочая база»' });
    expect(button.textContent).toContain('Рабочая база');
    expect(button.textContent).toContain('Р');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('меню: все хранилища, текущее отмечено, последним — «Управление хранилищами…»', async () => {
    renderSwitcher({});
    const menu = await openMenu();

    const vaults = await within(menu).findAllByRole('menuitemradio');
    expect(vaults.map((item) => item.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'Р Рабочая база',
      'Р Регламенты отдела',
      'Л личные заметки',
    ]);
    expect(vaults.map((item) => item.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    const items = within(menu).getAllByRole('menuitem');
    expect(items.at(-1)?.textContent?.trim()).toBe('Управление хранилищами…');
    expect(screen.getByRole('button', { name: 'Хранилище «Рабочая база»' }).getAttribute('aria-expanded')).toBe(
      'true',
    );
  });

  it('другое хранилище — vault_open (новое окно или фокус), меню закрывается', async () => {
    const { calls } = renderSwitcher({ vault_open: () => ({ outcome: 'opened' }) });
    const menu = await openMenu();

    await fireEvent.click(await within(menu).findByRole('menuitemradio', { name: /Регламенты отдела/ }));

    await waitFor(() => expect(calls).toContainEqual(['vault_open', { request: { id: 'b2' } }]));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('текущее хранилище — только закрыть меню', async () => {
    const { calls } = renderSwitcher({});
    const menu = await openMenu();

    await fireEvent.click(await within(menu).findByRole('menuitemradio', { name: /Рабочая база/ }));

    expect(screen.queryByRole('menu')).toBeNull();
    expect(calls.map(([command]) => command)).not.toContain('vault_open');
  });

  it('«Управление хранилищами…» — vault_manager_open', async () => {
    const { calls } = renderSwitcher({ vault_manager_open: () => null });
    const menu = await openMenu();

    await fireEvent.click(within(menu).getByRole('menuitem', { name: 'Управление хранилищами…' }));

    await waitFor(() => expect(calls.map(([command]) => command)).toContain('vault_manager_open'));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('ошибка открытия — уведомление текстом словаря', async () => {
    const unavailable: CoreError = { code: 'vault_unavailable', message_key: 'errors.vault_unavailable' };
    const { onNotice } = renderSwitcher({
      vault_open: () => {
        throw unavailable;
      },
    });
    const menu = await openMenu();

    await fireEvent.click(await within(menu).findByRole('menuitemradio', { name: /Регламенты отдела/ }));

    await waitFor(() =>
      expect(onNotice).toHaveBeenCalledWith('Папка хранилища недоступна. Проверьте, подключён ли диск.'),
    );
  });

  it('Esc закрывает меню и возвращает фокус на переключатель; стрелки ходят по пунктам', async () => {
    renderSwitcher({});
    const menu = await openMenu();
    const first = await within(menu).findByRole('menuitemradio', { name: /Рабочая база/ });
    await waitFor(() => expect(document.activeElement).toBe(first));

    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toContain('Регламенты отдела');
    await fireEvent.keyDown(menu, { key: 'ArrowUp' });
    await fireEvent.keyDown(menu, { key: 'ArrowUp' });
    expect(document.activeElement?.textContent).toContain('Управление хранилищами…');

    await fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Хранилище «Рабочая база»' }));
  });

  it('список не прочитался — в меню только «Управление хранилищами…», уведомление', async () => {
    const failed: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    const { onNotice } = renderSwitcher({
      vault_list: () => {
        throw failed;
      },
    });
    const menu = await openMenu();

    await waitFor(() => expect(onNotice).toHaveBeenCalledWith('Не удалось прочитать или записать файл.'));
    expect(within(menu).queryAllByRole('menuitemradio')).toHaveLength(0);
    expect(within(menu).getByRole('menuitem', { name: 'Управление хранилищами…' })).toBeTruthy();
  });
});
