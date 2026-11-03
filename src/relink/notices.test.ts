// Уведомления об итоге обновления ссылок (0.2-5.1, С2-3.5, С2-5.4, решения
// 115, 121): только по событию `links:updated`; «Показать» — список заметок с
// причиной; «Повторить» — заново только занятые и недоступные заметки.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, LinksPendingPayload, LinksUpdatedPayload, RelinkFailure, RelinkToken } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import {
  RelinkNotices,
  failureRows,
  isRetriable,
  notesIn,
  onlyConflicts,
  renamesText,
  type RelinkNoticesState,
} from './notices.ts';

const RETRY: RelinkToken = 'c'.repeat(32);
const MIB = 1024 * 1024;

const busy: RelinkFailure = { path: ['Проекты', 'Альфа', 'План работ.md'], reason: 'busy' };
const io: RelinkFailure = { path: ['Регламенты', 'Регламент доступа.md'], reason: 'io' };
const large: RelinkFailure = { path: ['Входящие', 'Архив 2019.md'], reason: 'too_large' };
const utf8: RelinkFailure = { path: ['Входящие', 'Старые заметки.md'], reason: 'not_utf8' };

function updated(patch: Partial<LinksUpdatedPayload> = {}): LinksUpdatedPayload {
  return { reason: 'rename', updated: 0, failed: [], retry_token: null, renames: [], renames_total: 0, ...patch };
}

type Answer = (token: RelinkToken, apply: boolean) => Promise<unknown>;

function model(retry: (token: RelinkToken) => Promise<unknown> = () => Promise.resolve(null), answer: Answer = () => Promise.resolve(null)) {
  const retries: RelinkToken[] = [];
  const answers: [RelinkToken, boolean][] = [];
  const plain: string[] = [];
  const notices = new RelinkNotices({
    retry: (token) => {
      retries.push(token);
      return retry(token);
    },
    answer: (token, apply) => {
      answers.push([token, apply]);
      return answer(token, apply);
    },
    notify: (text) => plain.push(text),
    limitBytes: 10 * MIB,
  });
  let state: RelinkNoticesState = { notices: [], questions: [], shown: null };
  notices.subscribe((next) => (state = next));
  return { notices, retries, answers, plain, state: () => state };
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
    expect(state()).toEqual({ notices: [], questions: [], shown: null });
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

    expect(state()).toEqual({ notices: [], questions: [], shown: null });
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

describe('0.2-4.6: conflict и имена «старое → новое»', () => {
  it('conflict не повторяется: знак повтора есть, но повторять некого — кнопки нет', () => {
    const notices = new RelinkNotices({
      retry: () => Promise.resolve(),
      answer: () => Promise.resolve(),
      notify: () => undefined,
      limitBytes: 10 * MIB,
    });
    const conflict: RelinkFailure = { path: ['План.md'], reason: 'conflict' };
    notices.updated(updated({ failed: [conflict], retry_token: RETRY }));
    const notice = notices.state.notices[0];
    expect(notice?.canRetry).toBe(false);
    expect(isRetriable('conflict')).toBe(false);
    expect(onlyConflicts([conflict])).toBe(true);
    expect(onlyConflicts([conflict, large])).toBe(false);
  });

  it('renamesText: одно переименование и «и ещё N»', () => {
    const rename = { from: ['Встреча.md'], to: ['Архив', 'Планёрка.md'] };
    expect(renamesText([], 0)).toBeNull();
    expect(renamesText([rename], 1)).toBe('«Встреча» → «Планёрка»');
    expect(renamesText([rename, rename], 25)).toBe('«Встреча» → «Планёрка» и ещё 24');
  });
});

// ---- 0.2-6.3: уведомления «нужен ответ» (С2-3.5 «в Проводнике», С2-3.6
// «Массовое копирование»; решения 110, 111) -----------------------------------

const PENDING: RelinkToken = 'd'.repeat(32);
const NEWER: RelinkToken = 'f'.repeat(32);

function pending(patch: Partial<LinksPendingPayload> = {}): LinksPendingPayload {
  return {
    token: PENDING,
    reason: 'external_rename',
    notes: 12,
    links: 20,
    renames: [
      { from: ['Встреча.md'], to: ['Планёрка.md'] },
      { from: ['План.md'], to: ['План работ.md'] },
      { from: ['Альфа', 'Итоги.md'], to: ['Альфа', 'Итоги 2026.md'] },
    ],
    renames_total: 3,
    ...patch,
  };
}

const staleToken: CoreError = { code: 'invalid_argument', message_key: 'errors.invalid_argument', field: 'token' };

describe('0.2-6.3: внешние переименования — вопрос', () => {
  it('Переименованы на диске: 3 заметки, на них ссылаются 12 — Обновить ссылки / Не обновлять', () => {
    const { notices, state } = model();

    notices.pending(pending());

    expect(state().questions).toEqual([
      expect.objectContaining({
        reason: 'external_rename',
        text: 'Переименованы на диске: 3 заметки, на них ссылаются 12 заметок',
        detail: 'Пока нет ответа, ссылки ведут на старые имена.',
        apply: 'Обновить ссылки',
        skip: 'Не обновлять',
        busy: false,
        error: null,
        stale: false,
      }),
    ]);
    // Знак — не в состоянии для показа.
    expect(JSON.stringify(state())).not.toContain(PENDING);
  });

  it('одна заметка: «Заметка «Встреча» переименована на диске в «Планёрка», на неё ссылаются 4 заметки»', () => {
    const { notices, state } = model();

    notices.pending(pending({ notes: 4, renames: [{ from: ['Встреча.md'], to: ['Планёрка.md'] }], renames_total: 1 }));

    expect(state().questions[0]).toMatchObject({
      text: 'Заметка «Встреча» переименована на диске в «Планёрка», на неё ссылаются 4 заметки',
      detail: null,
    });
  });

  it('одна заметка перенесена в другую папку — «перенесена на диске в «Бета»»; 1 заметка — «ссылается 1 заметка»', () => {
    const { notices, state } = model();

    notices.pending(pending({ notes: 1, renames: [{ from: ['Альфа', 'Встреча.md'], to: ['Бета', 'Встреча.md'] }], renames_total: 1 }));

    expect(state().questions[0]?.text).toBe('Заметка «Встреча» перенесена на диске в «Бета», на неё ссылается 1 заметка');
  });

  it('пачка больше 20: число — по renames_total, а не по длине списка', () => {
    const { notices, state } = model();

    notices.pending(pending({ notes: 21, renames_total: 25 }));

    expect(state().questions[0]?.text).toBe('Переименованы на диске: 25 заметок, на них ссылается 21 заметка');
  });

  it('висит до ответа: без крестика, не исчезает со временем', async () => {
    const { notices, state } = model();
    notices.pending(pending());

    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);

    expect(state().questions).toHaveLength(1);
    expect(state().questions[0]?.closable).toBe(false);
  });

  it('одно уведомление на пачку: новый links:pending с цепочкой заменяет прежний, а не добавляет второй', () => {
    const { notices, state } = model();
    notices.pending(pending({ renames: [{ from: ['Встреча.md'], to: ['Планёрка.md'] }], renames_total: 1, notes: 4 }));

    notices.pending(pending({ token: NEWER, notes: 5, renames: [{ from: ['Встреча.md'], to: ['Совещание.md'] }], renames_total: 1 }));

    expect(state().questions.map((question) => question.text)).toEqual([
      'Заметка «Встреча» переименована на диске в «Совещание», на неё ссылаются 5 заметок',
    ]);
  });

  it('ответ уходит relink_answer с token: «Обновить ссылки» — apply: true, «Не обновлять» — apply: false; уведомление уходит', async () => {
    const { notices, answers, state } = model();
    notices.pending(pending());
    const id = state().questions[0]?.id ?? -1;

    await notices.answer(id, true);

    expect(answers).toEqual([[PENDING, true]]);
    expect(state().questions).toEqual([]);

    notices.pending(pending({ token: NEWER }));
    await notices.answer(state().questions[0]?.id ?? -1, false);
    expect(answers).toEqual([
      [PENDING, true],
      [NEWER, false],
    ]);
    expect(state().questions).toEqual([]);
  });

  it('ответ на заменённый вопрос — по новому знаку; старый знак больше не отправляется', async () => {
    const { notices, answers, state } = model();
    notices.pending(pending());
    const old = state().questions[0]?.id ?? -1;
    notices.pending(pending({ token: NEWER }));

    await notices.answer(old, true);
    expect(answers).toEqual([]);

    await notices.answer(state().questions[0]?.id ?? -1, true);
    expect(answers).toEqual([[NEWER, true]]);
  });

  it('пока ответ идёт, кнопки недоступны, повторный щелчок не шлёт второй ответ', async () => {
    let finish = () => {};
    const { notices, answers, state } = model(undefined, () => new Promise<null>((resolve) => (finish = () => resolve(null))));
    notices.pending(pending());
    const id = state().questions[0]?.id ?? -1;

    const first = notices.answer(id, true);
    expect(state().questions[0]?.busy).toBe(true);
    await notices.answer(id, false);
    finish();
    await first;

    expect(answers).toEqual([[PENDING, true]]);
    expect(state().questions).toEqual([]);
  });

  it('invalid_argument (знак устарел) — уведомление не закрывается молча: текст ошибки, кнопок нет, есть крестик', async () => {
    const { notices, state } = model(undefined, () => Promise.reject(new CoreCallError('relink_answer', staleToken)));
    notices.pending(pending());

    await notices.answer(state().questions[0]?.id ?? -1, true);

    expect(state().questions).toEqual([
      expect.objectContaining({
        stale: true,
        busy: false,
        closable: true,
        error: 'Ответ не принят: на диске произошли новые изменения. Приложение покажет новое уведомление.',
      }),
    ]);
  });

  it('устаревший вопрос заменяется свежим links:pending и убирается итогом links:updated той же причины', async () => {
    const { notices, state } = model(undefined, () => Promise.reject(new CoreCallError('relink_answer', staleToken)));
    notices.pending(pending());
    await notices.answer(state().questions[0]?.id ?? -1, true);

    notices.pending(pending({ token: NEWER }));
    expect(state().questions).toEqual([expect.objectContaining({ stale: false, error: null })]);

    await notices.answer(state().questions[0]?.id ?? -1, true);
    notices.updated(updated({ reason: 'external_rename', failed: [{ path: ['План.md'], reason: 'conflict' }] }));
    expect(state().questions).toEqual([]);
    expect(state().notices.map((notice) => notice.text)).toEqual(['Не удалось обновить ссылки в 1 заметке']);
  });

  it('другая ошибка ответа — текст ошибки, кнопки остаются: можно ответить ещё раз', async () => {
    let fail = true;
    const { notices, answers, state } = model(undefined, () =>
      fail ? Promise.reject(new CoreCallError('relink_answer', { code: 'ipc_failed', message_key: 'errors.ipc_failed', detail: '' })) : Promise.resolve(null),
    );
    notices.pending(pending());
    const id = state().questions[0]?.id ?? -1;

    await notices.answer(id, true);
    expect(state().questions[0]).toMatchObject({ stale: false, busy: false, closable: false });
    expect(state().questions[0]?.error).toMatch(/^Не удалось отправить ответ\./);

    fail = false;
    await notices.answer(id, true);
    expect(answers).toEqual([
      [PENDING, true],
      [PENDING, true],
    ]);
    expect(state().questions).toEqual([]);
  });

  it('крестик есть только у устаревшего вопроса: dismissQuestion у живого вопроса ничего не делает', async () => {
    const { notices, state } = model(undefined, () => Promise.reject(new CoreCallError('relink_answer', staleToken)));
    notices.pending(pending());
    const id = state().questions[0]?.id ?? -1;

    notices.dismissQuestion(id);
    expect(state().questions).toHaveLength(1);

    await notices.answer(id, true);
    notices.dismissQuestion(id);
    expect(state().questions).toEqual([]);
  });

  it('итог «Всегда обновлять»: «Заметка «Встреча» переименована на диске. Ссылки обновлены в 3 заметках»', () => {
    const { notices, state } = model();

    notices.updated(updated({ reason: 'external_rename', updated: 3, renames: [{ from: ['Встреча.md'], to: ['Планёрка.md'] }], renames_total: 1 }));
    notices.updated(updated({ reason: 'external_rename', updated: 12, renames: pending().renames, renames_total: 3 }));

    expect(state().notices.map((notice) => notice.text)).toEqual([
      'Заметка «Встреча» переименована на диске. Ссылки обновлены в 3 заметках',
      'Переименованы на диске: 3 заметки. Ссылки обновлены в 12 заметках',
    ]);
  });
});

describe('0.2-6.3: повторы имён — вопрос', () => {
  it('Появились повторы имён: уточнить ссылки в N заметках? — Уточнить / Не трогать', () => {
    const { notices, state } = model();

    notices.pending(pending({ reason: 'duplicate', notes: 48, links: 60, renames: [], renames_total: 0 }));

    expect(state().questions).toEqual([
      expect.objectContaining({
        reason: 'duplicate',
        text: 'Появились повторы имён: уточнить ссылки в 48 заметках?',
        detail: 'Без уточнения ссылки ведут на самую старую из заметок с этим именем.',
        apply: 'Уточнить',
        skip: 'Не трогать',
        closable: false,
      }),
    ]);
  });

  it('два вопроса одновременно: внешнее переименование и повторы — оба на месте, каждый со своим знаком', async () => {
    const { notices, answers, state } = model();
    notices.pending(pending());
    notices.pending(pending({ token: NEWER, reason: 'duplicate', notes: 48, renames: [], renames_total: 0 }));

    expect(state().questions.map((question) => question.reason)).toEqual(['external_rename', 'duplicate']);

    await notices.answer(state().questions[1]?.id ?? -1, false);
    expect(answers).toEqual([[NEWER, false]]);
    expect(state().questions.map((question) => question.reason)).toEqual(['external_rename']);
  });

  it('одно уведомление на пачку: новый вопрос о повторах заменяет прежний', () => {
    const { notices, state } = model();
    notices.pending(pending({ reason: 'duplicate', notes: 30, renames: [], renames_total: 0 }));

    notices.pending(pending({ token: NEWER, reason: 'duplicate', notes: 48, renames: [], renames_total: 0 }));

    expect(state().questions.map((question) => question.text)).toEqual(['Появились повторы имён: уточнить ссылки в 48 заметках?']);
  });
});

describe('ревью 0.2-5.2, М4: отмена переименования (Ctrl+Z) — свой текст', () => {
  const forward = { from: ['Встреча.md'], to: ['Планёрка.md'] };
  const back = { from: ['Планёрка.md'], to: ['Встреча.md'] };

  it('после Ctrl+Z — «Переименование отменено, ссылки возвращены в N заметках» вместо второго «Ссылки обновлены…»', () => {
    const { notices, state } = model();
    notices.updated(updated({ updated: 2, renames: [forward], renames_total: 1 }));

    notices.undoing(back);
    notices.updated(updated({ updated: 2, renames: [back], renames_total: 1 }));

    expect(state().notices.map((notice) => notice.text)).toEqual(['Переименование отменено, ссылки возвращены в 2 заметках']);
  });

  it('ожидание отмены одноразовое: следующее переименование — обычный текст', () => {
    const { notices, state } = model();
    notices.undoing(back);
    notices.updated(updated({ updated: 1, renames: [back], renames_total: 1 }));

    notices.updated(updated({ updated: 1, renames: [forward], renames_total: 1 }));

    expect(state().notices.map((notice) => notice.text)).toEqual([
      'Переименование отменено, ссылки возвращены в 1 заметке',
      'Ссылки обновлены в 1 заметке',
    ]);
  });

  it('другое переименование, пока ждём отмену, — обычный текст', () => {
    const { notices, state } = model();
    notices.undoing(back);

    notices.updated(updated({ updated: 1, renames: [{ from: ['А.md'], to: ['Б.md'] }], renames_total: 1 }));

    expect(state().notices.map((notice) => notice.text)).toEqual(['Ссылки обновлены в 1 заметке']);
  });
});
