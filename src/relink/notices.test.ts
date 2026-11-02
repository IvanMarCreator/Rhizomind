// Уведомления об итоге обновления ссылок (0.2-5.1, С2-3.5, С2-5.4, решения
// 115, 121): только по событию `links:updated`; «Показать» — список заметок с
// причиной; «Повторить» — заново только занятые и недоступные заметки.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LinksUpdatedPayload, RelinkFailure, RelinkToken } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import { RelinkNotices, failureRows, notesIn, type RelinkNoticesState } from './notices.ts';

const RETRY: RelinkToken = 'c'.repeat(32);
const MIB = 1024 * 1024;

const busy: RelinkFailure = { path: ['Проекты', 'Альфа', 'План работ.md'], reason: 'busy' };
const io: RelinkFailure = { path: ['Регламенты', 'Регламент доступа.md'], reason: 'io' };
const large: RelinkFailure = { path: ['Входящие', 'Архив 2019.md'], reason: 'too_large' };
const utf8: RelinkFailure = { path: ['Входящие', 'Старые заметки.md'], reason: 'not_utf8' };

function updated(patch: Partial<LinksUpdatedPayload> = {}): LinksUpdatedPayload {
  return { reason: 'rename', updated: 0, failed: [], retry_token: null, ...patch };
}

function model(retry: (token: RelinkToken) => Promise<unknown> = () => Promise.resolve(null)) {
  const retries: RelinkToken[] = [];
  const plain: string[] = [];
  const notices = new RelinkNotices({
    retry: (token) => {
      retries.push(token);
      return retry(token);
    },
    notify: (text) => plain.push(text),
    limitBytes: 10 * MIB,
  });
  let state: RelinkNoticesState = { notices: [], shown: null };
  notices.subscribe((next) => (state = next));
  return { notices, retries, plain, state: () => state };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('«Ссылки обновлены в N заметках»', () => {
  it('Ссылки обновлены в N заметках: одно уведомление с крестиком, без кнопок', () => {
    const { notices, state } = model();

    notices.updated(updated({ updated: 5 }));

    expect(state().notices).toHaveLength(1);
    expect(state().notices[0]).toMatchObject({
      tone: 'success',
      text: 'Ссылки обновлены в 5 заметках',
      detail: null,
      closable: true,
      canShow: false,
      canRetry: false,
    });
  });

  it('число заметок — по правилам русского языка', () => {
    expect(notesIn(1)).toBe('1 заметке');
    expect(notesIn(2)).toBe('2 заметках');
    expect(notesIn(11)).toBe('11 заметках');
    expect(notesIn(21)).toBe('21 заметке');
    expect(notesIn(111)).toBe('111 заметках');
  });

  it('обновлять было нечего и сбоев нет — уведомления нет', () => {
    const { notices, state } = model();

    notices.updated(updated());

    expect(state().notices).toEqual([]);
  });

  it('доделка прерванного обновления — своё уведомление (С2-5.4, «Отката нет»)', () => {
    const { notices, state } = model();

    notices.updated(updated({ reason: 'resumed', updated: 3 }));

    expect(state().notices.map((notice) => notice.text)).toEqual([
      'Обновление ссылок, прерванное при прошлом запуске, завершено',
    ]);
  });

  it('уточнение при повторе имени — «Ссылки уточнены в N заметках»', () => {
    const { notices, state } = model();

    notices.updated(updated({ reason: 'duplicate', updated: 2 }));

    expect(state().notices.map((notice) => notice.text)).toEqual(['Ссылки уточнены в 2 заметках']);
  });

  it('закрывается крестиком', () => {
    const { notices, state } = model();
    notices.updated(updated({ updated: 5 }));

    notices.dismiss(state().notices[0]?.id ?? -1);

    expect(state().notices).toEqual([]);
  });
});

describe('«Не удалось обновить ссылки в N заметках» (решения 115, 121)', () => {
  it('Не удалось обновить ссылки в N заметках — с «Показать» и «Повторить», переименование не отменено', () => {
    const { notices, state } = model();

    notices.updated(updated({ updated: 3, failed: [busy, io, large, utf8], retry_token: RETRY }));

    expect(state().notices.map((notice) => notice.text)).toEqual([
      'Ссылки обновлены в 3 заметках',
      'Не удалось обновить ссылки в 4 заметках',
    ]);
    expect(state().notices[1]).toMatchObject({
      tone: 'warning',
      detail: 'Переименование выполнено.',
      closable: true,
      canShow: true,
      canRetry: true,
    });
  });

  it('при одних too_large/not_utf8 кнопки «Повторить» нет, повтор не поможет', () => {
    const { notices, state } = model();

    notices.updated(updated({ failed: [large, utf8], retry_token: null }));

    expect(state().notices[0]).toMatchObject({
      text: 'Не удалось обновить ссылки в 2 заметках',
      canShow: true,
      canRetry: false,
    });
    expect(state().notices[0]?.detail).toBe(
      'Переименование выполнено. Повтор не поможет: заметки больше лимита или не в UTF-8.',
    );
  });

  it('знак повтора есть, но повторять некого (только too_large/not_utf8) — кнопки нет', () => {
    const { notices, state } = model();

    notices.updated(updated({ failed: [large], retry_token: RETRY }));

    expect(state().notices[0]?.canRetry).toBe(false);
  });

  it('занятые заметки без знака повтора — кнопки нет: повторить нечем', () => {
    const { notices, state } = model();

    notices.updated(updated({ failed: [busy], retry_token: null }));

    expect(state().notices[0]?.canRetry).toBe(false);
  });

  it('уведомление с кнопками не исчезает само', async () => {
    const { notices, state } = model();
    notices.updated(updated({ updated: 1, failed: [busy], retry_token: RETRY }));

    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);

    expect(state().notices.map((notice) => notice.tone)).toEqual(['success', 'warning']);
  });

  it('Повторить пробует заново только занятые и недоступные заметки (busy/io): relink_answer по retry_token, уведомление уходит', async () => {
    const { notices, state, retries } = model();
    notices.updated(updated({ failed: [busy, io, large], retry_token: RETRY }));
    const id = state().notices[0]?.id ?? -1;
    notices.show(id);

    await notices.retry(id);
    await notices.retry(id);

    expect(retries).toEqual([RETRY]);
    expect(state()).toEqual({ notices: [], shown: null });
  });

  it('повтор отклонён ядром (знак просрочен) — уведомление с причиной', async () => {
    const { notices, state, plain } = model(() =>
      Promise.reject(new CoreCallError('relink_answer', { code: 'invalid_argument', message_key: 'errors.invalid_argument' })),
    );
    notices.updated(updated({ failed: [busy], retry_token: RETRY }));

    await notices.retry(state().notices[0]?.id ?? -1);

    expect(plain).toHaveLength(1);
    expect(plain[0]).toContain('Не удалось повторить обновление ссылок.');
  });

  it('итог повтора — новое уведомление по links:updated с reason retry', () => {
    const { notices, state } = model();

    notices.updated(updated({ reason: 'retry', updated: 2, failed: [io], retry_token: RETRY }));

    expect(state().notices.map((notice) => notice.text)).toEqual([
      'Ссылки обновлены в 2 заметках',
      'Не удалось обновить ссылки в 1 заметке',
    ]);
  });

  it('Показать открывает список, закрытие — убирает его; уведомление остаётся', () => {
    const { notices, state } = model();
    notices.updated(updated({ failed: [busy], retry_token: RETRY }));
    const notice = state().notices[0];

    notices.show(notice?.id ?? -1);
    expect(state().shown).toBe(notice);
    notices.hide();

    expect(state().shown).toBeNull();
    expect(state().notices).toEqual([notice]);
  });

  it('крестик у уведомления, чей список открыт, закрывает и список', () => {
    const { notices, state } = model();
    notices.updated(updated({ failed: [busy], retry_token: RETRY }));
    const id = state().notices[0]?.id ?? -1;
    notices.show(id);

    notices.dismiss(id);

    expect(state()).toEqual({ notices: [], shown: null });
  });
});

describe('список «Не удалось обновить ссылки» (макет У-3)', () => {
  it('строка — имя, папка и причина; «будет повторено» только у busy/io', () => {
    expect(failureRows([busy, io, large, utf8, { path: ['Корень.md'], reason: 'io' }], 10 * MIB)).toEqual([
      { path: busy.path, name: 'План работ', folder: 'Проекты / Альфа', label: 'Файл занят другой программой', retried: true },
      { path: io.path, name: 'Регламент доступа', folder: 'Регламенты', label: 'Недоступна для записи', retried: true },
      { path: large.path, name: 'Архив 2019', folder: 'Входящие', label: 'Больше 10 МБ', retried: false },
      { path: utf8.path, name: 'Старые заметки', folder: 'Входящие', label: 'Не в UTF-8', retried: false },
      { path: ['Корень.md'], name: 'Корень', folder: 'корень хранилища', label: 'Недоступна для записи', retried: true },
    ]);
  });
});

describe('ход переписывания (links:progress)', () => {
  it('одно уведомление без крестика, обновляется на месте; итог links:updated его убирает', () => {
    const { notices, state } = model();

    notices.progress({ token: null, done: 3, total: 10 });
    const id = state().notices[0]?.id;
    notices.progress({ token: null, done: 7, total: 10 });

    expect(state().notices).toHaveLength(1);
    expect(state().notices[0]).toMatchObject({ id, tone: 'progress', text: 'Обновление ссылок… 7 из 10', closable: false });

    notices.updated(updated({ updated: 10 }));
    expect(state().notices.map((notice) => notice.text)).toEqual(['Ссылки обновлены в 10 заметках']);
  });

  it('done = total — уведомление о ходе уходит само', () => {
    const { notices, state } = model();
    notices.progress({ token: null, done: 3, total: 10 });

    notices.progress({ token: null, done: 10, total: 10 });

    expect(state().notices).toEqual([]);
  });
});
