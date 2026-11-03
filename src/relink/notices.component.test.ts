// Уведомления об итоге обновления ссылок в зоне уведомлений окна и список
// «Не удалось обновить ссылки» (0.2-5.1, макеты У-1, У-3; решения 115, 121).
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LinksUpdatedPayload, RelinkToken, VaultPath } from '../api/index.ts';
import Notifications from '../shell/Notifications.svelte';
import { RelinkNotices } from './notices.ts';

const RETRY: RelinkToken = 'c'.repeat(32);

function setup(payload: LinksUpdatedPayload) {
  const retries: RelinkToken[] = [];
  const opened: VaultPath[] = [];
  const relink = new RelinkNotices({
    retry: (token) => {
      retries.push(token);
      return Promise.resolve(null);
    },
    answer: () => Promise.resolve(null),
    notify: () => undefined,
    limitBytes: 10 * 1024 * 1024,
  });
  relink.updated(payload);
  render(Notifications, { notices: [], onDismiss: () => undefined, relink, onOpenNote: (path: VaultPath) => opened.push(path) });
  return { relink, retries, opened };
}

const failed: LinksUpdatedPayload = {
  reason: 'rename',
  updated: 3,
  failed: [
    { path: ['Проекты', 'Альфа', 'План работ.md'], reason: 'busy' },
    { path: ['Входящие', 'Архив 2019.md'], reason: 'too_large' },
  ],
  retry_token: RETRY,
  renames: [],
  renames_total: 0,
};

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('зона уведомлений: итог обновления ссылок', () => {
  it('Ссылки обновлены в N заметках', () => {
    setup({ reason: 'rename', updated: 5, failed: [], retry_token: null, renames: [], renames_total: 0 });

    const notice = screen.getByText('Ссылки обновлены в 5 заметках').closest('.notice') as HTMLElement;
    expect(within(notice).getByRole('button', { name: 'Закрыть уведомление' })).toBeTruthy();
    expect(within(notice).queryByRole('button', { name: 'Показать' })).toBeNull();
  });

  it('Не удалось обновить ссылки в N заметках — Показать открывает список с причиной у каждой заметки, щелчок открывает заметку', async () => {
    const { opened } = setup(failed);
    const notice = screen.getByText('Не удалось обновить ссылки в 2 заметках').closest('.notice') as HTMLElement;
    expect(within(notice).getByText('Переименование выполнено.')).toBeTruthy();

    await fireEvent.click(within(notice).getByRole('button', { name: 'Показать' }));

    const dialog = screen.getByRole('dialog', { name: 'Не удалось обновить ссылки в 2 заметках' });
    const rows = within(dialog).getAllByRole('button', { name: /^Открыть/ });
    expect(rows.map((row) => row.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'План работ Проекты / Альфа Файл занят другой программой будет повторено',
      'Архив 2019 Входящие Больше 10 МБ не повторяется',
    ]);

    await fireEvent.click(rows[1] as HTMLElement);

    expect(opened).toEqual([['Входящие', 'Архив 2019.md']]);
    expect(screen.queryByRole('dialog')).toBeNull();
    // Уведомление висит до закрытия (решение 121).
    expect(screen.getByText('Не удалось обновить ссылки в 2 заметках')).toBeTruthy();
  });

  it('Повторить пробует заново только занятые и недоступные заметки (busy/io): relink_answer с retry_token', async () => {
    const { retries } = setup(failed);
    const notice = screen.getByText('Не удалось обновить ссылки в 2 заметках').closest('.notice') as HTMLElement;

    await fireEvent.click(within(notice).getByRole('button', { name: 'Повторить' }));

    expect(retries).toEqual([RETRY]);
    expect(screen.queryByText('Не удалось обновить ссылки в 2 заметках')).toBeNull();
  });

  it('Повторить в списке — тот же повтор, список закрывается', async () => {
    const { retries } = setup(failed);
    await fireEvent.click(screen.getByRole('button', { name: 'Показать' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('«Повторить» пробует заново только занятые и недоступные заметки.')).toBeTruthy();

    await fireEvent.click(within(dialog).getByRole('button', { name: 'Повторить' }));

    expect(retries).toEqual([RETRY]);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('при одних too_large/not_utf8 кнопки «Повторить» нет — ни в уведомлении, ни в списке', async () => {
    setup({
      reason: 'rename',
      updated: 0,
      failed: [
        { path: ['Входящие', 'Архив 2019.md'], reason: 'too_large' },
        { path: ['Входящие', 'Старые заметки.md'], reason: 'not_utf8' },
      ],
      retry_token: null,
      renames: [],
      renames_total: 0,
    });

    expect(screen.queryByRole('button', { name: 'Повторить' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Показать' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).queryByRole('button', { name: 'Повторить' })).toBeNull();
    expect(within(dialog).getByText('Уменьшите заметки или сохраните их в UTF-8 и обновите ссылки вручную.')).toBeTruthy();
  });

  it('conflict (Р20): «Изменилась, пока шло обновление — проверьте ссылки вручную», без «Повторить»; в списке — «старое → новое»', async () => {
    setup({
      reason: 'rename',
      updated: 3,
      failed: [{ path: ['Проекты', 'План.md'], reason: 'conflict' }],
      retry_token: null,
      renames: [{ from: ['Встреча.md'], to: ['Архив', 'Планёрка.md'] }],
      renames_total: 1,
    });

    const notice = screen.getByText('Не удалось обновить ссылки в 1 заметке').closest('.notice') as HTMLElement;
    expect(within(notice).queryByRole('button', { name: 'Повторить' })).toBeNull();
    expect(within(notice).getByText(/заметки изменились, пока шло обновление/)).toBeTruthy();
    await fireEvent.click(within(notice).getByRole('button', { name: 'Показать' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('«Встреча» → «Планёрка»')).toBeTruthy();
    const row = within(dialog).getByRole('button', { name: /^Открыть/ });
    expect(row.textContent?.replace(/\s+/g, ' ').trim()).toBe(
      'План Проекты Изменилась, пока шло обновление — проверьте ссылки вручную не повторяется',
    );
    expect(within(dialog).queryByRole('button', { name: 'Повторить' })).toBeNull();
    expect(within(dialog).getByText('Откройте эти заметки и проверьте ссылки вручную.')).toBeTruthy();
  });

  it('список: фокус на «Закрыть», Esc закрывает', async () => {
    setup(failed);
    await fireEvent.click(screen.getByRole('button', { name: 'Показать' }));
    const dialog = screen.getByRole('dialog');
    expect(document.activeElement).toBe(within(dialog).getByRole('button', { name: 'Закрыть' }));

    await fireEvent.keyDown(dialog, { key: 'Escape' });

    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('уведомление с кнопками не исчезает само', async () => {
    vi.useFakeTimers();
    setup(failed);

    await vi.advanceTimersByTimeAsync(10 * 60 * 1000);

    expect(screen.getByText('Не удалось обновить ссылки в 2 заметках')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Повторить' })).toBeTruthy();
  });

  it('крестик закрывает уведомление о сбое', async () => {
    setup(failed);
    const notice = screen.getByText('Не удалось обновить ссылки в 2 заметках').closest('.notice') as HTMLElement;

    await fireEvent.click(within(notice).getByRole('button', { name: 'Закрыть уведомление' }));

    expect(screen.queryByText('Не удалось обновить ссылки в 2 заметках')).toBeNull();
  });
});
