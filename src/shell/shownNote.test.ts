import { describe, expect, it } from 'vitest';
import type { VaultPath } from '../api/index.ts';
import { ShownNote } from './shownNote.ts';

/** Подменная команда: журнал вызовов, ответ — по требованию теста. */
function fakeSend() {
  const sent: (VaultPath | null)[] = [];
  const pending: { resolve: () => void; reject: (error: unknown) => void }[] = [];
  const send = (path: VaultPath | null) => {
    sent.push(path);
    return new Promise<void>((resolve, reject) => pending.push({ resolve, reject }));
  };
  /** Ответить на следующий ожидающий вызов. */
  const answer = async (error?: unknown) => {
    const next = pending.shift();
    if (!next) throw new Error('нет ожидающего вызова');
    if (error === undefined) next.resolve();
    else next.reject(error);
    // Дать очереди отправить следующий вызов.
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  return { sent, send, answer };
}

describe('показанная заметка → ядро (заголовок окна, решение 82)', () => {
  it('сообщает путь при открытии и переходе; «заметки нет» сначала не шлёт — ядро это знает', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);

    shown.report(null);
    expect(sent).toEqual([]);

    shown.report(['Архив', 'Итоги года.md']);
    await answer();
    shown.report(['План.md']);
    await answer();
    shown.report(null);
    await answer();

    expect(sent).toEqual([['Архив', 'Итоги года.md'], ['План.md'], null]);
  });

  it('тот же путь повторно не шлёт (ввод, повторное открытие той же заметки)', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);

    shown.report(['А.md']);
    await answer();
    shown.report(['А.md']);
    shown.report(['А.md']);

    expect(sent).toEqual([['А.md']]);
  });

  it('вызовы идут по одному; пока ядро отвечает, шлётся только последний путь', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);

    shown.report(['А.md']);
    shown.report(['Б.md']);
    shown.report(['В.md']);
    expect(sent).toEqual([['А.md']]);
    await answer();
    expect(sent).toEqual([['А.md'], ['В.md']]);
    await answer();

    // Вернулись к уже показанному — ничего не шлётся.
    shown.report(['В.md']);
    expect(sent).toHaveLength(2);
  });

  it('ядро отказало (заметки уже нет) — заголовок без заметки, а не прежнее имя', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);

    shown.report(['А.md']);
    await answer();
    shown.report(['Удалена.md']);
    await answer({ code: 'not_found', message_key: 'errors.not_found' });
    await answer();

    expect(sent).toEqual([['А.md'], ['Удалена.md'], null]);
  });

  it('путь после отказа не считается отправленным: явный повтор уходит снова, без зацикливания', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);
    const notFound = { code: 'not_found', message_key: 'errors.not_found' };

    shown.report(['Ещё не в списке.md']);
    await answer(notFound);
    await answer();
    expect(sent).toEqual([['Ещё не в списке.md'], null]);

    // Явный повтор — тот же путь уходит снова (заметка уже появилась в списке).
    shown.report(['Ещё не в списке.md']);
    await answer();
    expect(sent).toEqual([['Ещё не в списке.md'], null, ['Ещё не в списке.md']]);
    // Теперь отправлен успешно — повтор не шлётся.
    shown.report(['Ещё не в списке.md']);
    expect(sent).toHaveLength(3);
  });

  it('после отказа другой путь и возврат к отказанному — оба уходят', async () => {
    const { sent, send, answer } = fakeSend();
    const shown = new ShownNote(send);

    shown.report(['А.md']);
    await answer({ code: 'not_found', message_key: 'errors.not_found' });
    await answer();
    shown.report(null);
    shown.report(['А.md']);
    await answer();

    expect(sent).toEqual([['А.md'], null, ['А.md']]);
  });
});
