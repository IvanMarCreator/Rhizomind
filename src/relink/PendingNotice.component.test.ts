// Уведомления «нужен ответ» в зоне уведомлений окна (0.2-6.3; С2-3.5
// «в Проводнике», С2-3.6 «Массовое копирование»; решения 110, 111; макеты
// У-1, У-2): без окна поверх работы, без крестика, висят до ответа; ответ —
// `relink_answer` со знаком из `links:pending`.
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, LinksPendingPayload, RelinkToken } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import Notifications from '../shell/Notifications.svelte';
import { RelinkNotices } from './notices.ts';

const TOKEN: RelinkToken = 'd'.repeat(32);
const DUP: RelinkToken = 'f'.repeat(32);

const external: LinksPendingPayload = {
  token: TOKEN,
  reason: 'external_rename',
  notes: 12,
  links: 20,
  renames: [
    { from: ['Встреча.md'], to: ['Планёрка.md'] },
    { from: ['План.md'], to: ['План работ.md'] },
    { from: ['Альфа', 'Итоги.md'], to: ['Альфа', 'Итоги 2026.md'] },
  ],
  renames_total: 3,
};

const duplicate: LinksPendingPayload = { token: DUP, reason: 'duplicate', notes: 48, links: 60, renames: [], renames_total: 0 };

const EXTERNAL_TEXT = 'Переименованы на диске: 3 заметки, на них ссылаются 12 заметок';
const DUPLICATE_TEXT = 'Появились повторы имён: уточнить ссылки в 48 заметках?';

function setup(answer: (token: RelinkToken, apply: boolean) => Promise<unknown> = () => Promise.resolve(null)) {
  const answers: [RelinkToken, boolean][] = [];
  const relink = new RelinkNotices({
    retry: () => Promise.resolve(null),
    answer: (token, apply) => {
      answers.push([token, apply]);
      return answer(token, apply);
    },
    notify: () => undefined,
    limitBytes: 10 * 1024 * 1024,
  });
  render(Notifications, { notices: [], onDismiss: () => undefined, relink });
  return { relink, answers };
}

/** Уведомление по его тексту. */
const notice = (text: string) => screen.getByText(text).closest('.notice') as HTMLElement;

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('уведомление «нужен ответ»', () => {
  it('Переименованы на диске: 3 заметки, на них ссылаются 12 — Обновить ссылки / Не обновлять', async () => {
    const { relink } = setup();
    relink.pending(external);
    await Promise.resolve();

    const box = notice(EXTERNAL_TEXT);
    expect(within(box).getByText('Пока нет ответа, ссылки ведут на старые имена.')).toBeTruthy();
    const buttons = within(box).getAllByRole('button');
    expect(buttons.map((button) => button.textContent?.trim())).toEqual(['Обновить ссылки', 'Не обновлять']);
    // «Обновить ссылки» — основная кнопка (макет У-1).
    expect(buttons[0]?.classList.contains('primary')).toBe(true);
    // Вопрос — не окно поверх работы.
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('Появились повторы имён: уточнить ссылки в N заметках? — Уточнить / Не трогать', async () => {
    const { relink, answers } = setup();
    relink.pending(duplicate);
    await Promise.resolve();

    const box = notice(DUPLICATE_TEXT);
    expect(within(box).getByText('Без уточнения ссылки ведут на самую старую из заметок с этим именем.')).toBeTruthy();
    expect(within(box).getAllByRole('button').map((button) => button.textContent?.trim())).toEqual(['Уточнить', 'Не трогать']);

    await fireEvent.click(within(box).getByRole('button', { name: 'Не трогать' }));

    expect(answers).toEqual([[DUP, false]]);
    expect(screen.queryByText(DUPLICATE_TEXT)).toBeNull();
  });

  it('висит до ответа: крестика нет, через час на месте', async () => {
    vi.useFakeTimers();
    const { relink } = setup();
    relink.pending(external);

    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);

    const box = notice(EXTERNAL_TEXT);
    expect(within(box).queryByRole('button', { name: 'Закрыть уведомление' })).toBeNull();
    expect(within(box).getByRole('button', { name: 'Обновить ссылки' })).toBeTruthy();
  });

  it('одно уведомление на пачку: новый links:pending заменяет прежнее', async () => {
    const { relink } = setup();
    relink.pending({ ...external, notes: 4, renames: [{ from: ['Встреча.md'], to: ['Планёрка.md'] }], renames_total: 1 });
    relink.pending({ ...external, token: 'e'.repeat(32) });
    await Promise.resolve();

    expect(screen.getAllByRole('button', { name: 'Обновить ссылки' })).toHaveLength(1);
    expect(screen.queryByText(/«Встреча» переименована на диске/)).toBeNull();
    expect(notice(EXTERNAL_TEXT)).toBeTruthy();
  });

  it('ответ уходит relink_answer с token; уведомление закрывается', async () => {
    const { relink, answers } = setup();
    relink.pending(external);
    await Promise.resolve();

    await fireEvent.click(within(notice(EXTERNAL_TEXT)).getByRole('button', { name: 'Обновить ссылки' }));

    expect(answers).toEqual([[TOKEN, true]]);
    expect(screen.queryByText(EXTERNAL_TEXT)).toBeNull();
  });

  it('два уведомления одновременно (внешнее переименование и повторы): оба видны, ответы — каждый со своим знаком', async () => {
    const { relink, answers } = setup();
    relink.pending(external);
    relink.pending(duplicate);
    await Promise.resolve();

    const first = notice(EXTERNAL_TEXT);
    const second = notice(DUPLICATE_TEXT);
    expect(first).not.toBe(second);
    expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    await fireEvent.click(within(second).getByRole('button', { name: 'Уточнить' }));
    await fireEvent.click(within(first).getByRole('button', { name: 'Не обновлять' }));

    expect(answers).toEqual([
      [DUP, true],
      [TOKEN, false],
    ]);
  });

  it('пока ответ идёт, кнопки недоступны', async () => {
    let finish = () => {};
    const { relink } = setup(() => new Promise<null>((resolve) => (finish = () => resolve(null))));
    relink.pending(external);
    await Promise.resolve();
    const box = notice(EXTERNAL_TEXT);

    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));

    for (const button of within(box).getAllByRole('button')) expect((button as HTMLButtonElement).disabled).toBe(true);
    finish();
    await vi.waitFor(() => expect(screen.queryByText(EXTERNAL_TEXT)).toBeNull());
  });

  it('invalid_argument — ошибка видна в уведомлении, кнопок ответа нет, крестик закрывает', async () => {
    const stale: CoreError = { code: 'invalid_argument', message_key: 'errors.invalid_argument', field: 'token' };
    const { relink } = setup(() => Promise.reject(new CoreCallError('relink_answer', stale)));
    relink.pending(external);
    await Promise.resolve();

    await fireEvent.click(within(notice(EXTERNAL_TEXT)).getByRole('button', { name: 'Обновить ссылки' }));

    const box = await vi.waitFor(() => {
      const found = notice(EXTERNAL_TEXT);
      expect(within(found).getByRole('alert').textContent).toBe(
        'Ответ не принят: на диске произошли новые изменения. Приложение покажет новое уведомление.',
      );
      return found;
    });
    expect(within(box).queryByRole('button', { name: 'Обновить ссылки' })).toBeNull();
    expect(within(box).queryByRole('button', { name: 'Не обновлять' })).toBeNull();
    // «Пока нет ответа…» больше не верно: ответить уже нельзя.
    expect(within(box).queryByText('Пока нет ответа, ссылки ведут на старые имена.')).toBeNull();

    await fireEvent.click(within(box).getByRole('button', { name: 'Закрыть уведомление' }));
    expect(screen.queryByText(EXTERNAL_TEXT)).toBeNull();
  });
});
