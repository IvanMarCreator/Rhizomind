// Закрытие окна хранилища (задача 0.7, решение владельца 53): ответ ядру
// после записи ввода; при сбое записи окно не закрывается.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CoreCallError } from '../api/index.ts';
import type { UnsavedNote } from '../editor/sessions.ts';
import { REFUSAL_WINDOW_MS, WindowClosing } from './closing.ts';

const ioFailed = new CoreCallError('note_write', { code: 'io_failed', message_key: 'errors.io_failed' });

function setup(results: UnsavedNote[][]) {
  const answers: string[] = [];
  const notices: string[] = [];
  let call = 0;
  const closing = new WindowClosing({
    flush: () => Promise.resolve(results[Math.min(call++, results.length - 1)] ?? []),
    ready: () => {
      answers.push('ready');
      return Promise.resolve();
    },
    cancel: () => {
      answers.push('cancel');
      return Promise.resolve();
    },
    notify: (text) => notices.push(text),
  });
  return { closing, answers, notices };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('закрытие окна', () => {
  it('ввод записан — ядру «готово», без уведомлений', async () => {
    const { closing, answers, notices } = setup([[]]);
    await closing.closeRequested();
    expect(answers).toEqual(['ready']);
    expect(notices).toEqual([]);
  });

  it('ответ уходит только после записи', async () => {
    let finish: (value: UnsavedNote[]) => void = () => undefined;
    const answers: string[] = [];
    const closing = new WindowClosing({
      flush: () => new Promise((resolve) => (finish = resolve)),
      ready: () => {
        answers.push('ready');
        return Promise.resolve();
      },
      cancel: () => Promise.resolve(),
      notify: () => undefined,
    });
    const done = closing.closeRequested();
    await vi.advanceTimersByTimeAsync(0);
    expect(answers).toEqual([]);
    finish([]);
    await done;
    expect(answers).toEqual(['ready']);
  });

  it('запись не удалась — окно не закрывается, уведомление называет заметку и причину', async () => {
    const { closing, answers, notices } = setup([[{ path: ['Папка', 'План.md'], error: ioFailed }]]);
    await closing.closeRequested();
    expect(answers).toEqual(['cancel']);
    expect(notices).toEqual([
      'Окно не закрыто: не удалось сохранить заметку «План». Не удалось прочитать или записать файл. Текст остаётся в редакторе. Чтобы закрыть окно без сохранения, закройте его ещё раз в течение минуты.',
    ]);
  });

  it('повторное закрытие в течение минуты после отказа закрывает окно без сохранения', async () => {
    const failed = [{ path: ['План.md'], error: ioFailed }];
    const { closing, answers } = setup([failed, failed]);
    await closing.closeRequested();
    await vi.advanceTimersByTimeAsync(REFUSAL_WINDOW_MS - 1000);
    await closing.closeRequested();
    expect(answers).toEqual(['cancel', 'ready']);
  });

  it('после минуты отказ повторяется: окно снова не закрывается молча', async () => {
    const failed = [{ path: ['План.md'], error: ioFailed }];
    const { closing, answers } = setup([failed, failed]);
    await closing.closeRequested();
    await vi.advanceTimersByTimeAsync(REFUSAL_WINDOW_MS + 1000);
    await closing.closeRequested();
    expect(answers).toEqual(['cancel', 'cancel']);
  });

  it('несколько заметок — все по именам', async () => {
    const { closing, notices } = setup([
      [
        { path: ['А.md'], error: ioFailed },
        { path: ['Б.md'], error: ioFailed },
      ],
    ]);
    await closing.closeRequested();
    expect(notices[0]).toContain('не удалось сохранить заметки «А», «Б».');
  });

  it('второй сигнал во время записи не запускает вторую запись', async () => {
    let flushes = 0;
    let finish: (value: UnsavedNote[]) => void = () => undefined;
    const answers: string[] = [];
    const closing = new WindowClosing({
      flush: () => {
        flushes++;
        return new Promise((resolve) => (finish = resolve));
      },
      ready: () => {
        answers.push('ready');
        return Promise.resolve();
      },
      cancel: () => Promise.resolve(),
      notify: () => undefined,
    });
    const first = closing.closeRequested();
    const second = closing.closeRequested();
    finish([]);
    await Promise.all([first, second]);
    expect(flushes).toBe(1);
    expect(answers).toEqual(['ready']);
  });
});
