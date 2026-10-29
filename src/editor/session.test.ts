// Сохранение открытой заметки (С3.3, С3.6): автосохранение через 1 с после
// паузы ввода, сохранение по требованию (переход, закрытие), конфликт
// изменений, ошибки записи, удаление снаружи, только чтение.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, FileVersion, NoteReadResponse, NoteWriteRequest, NoteWriteResponse } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import { AUTOSAVE_DELAY_MS, NoteSession, RETRY_DELAY_MS, type NoteStore, type SaveStatus } from './session.ts';

const PATH = ['Проекты', 'Встреча.md'];

function version(n: number): FileVersion {
  return { hash: String(n).repeat(64).slice(0, 64), size: n, modified_ms: n };
}

function opened(text: string, read_only: NoteReadResponse['read_only'] = null): NoteReadResponse {
  return { text, version: version(1), read_only };
}

/** Подменное хранилище: запись отвечает сразу или ждёт `release()`. */
class FakeStore implements NoteStore {
  writes: NoteWriteRequest[] = [];
  next = 2;
  respond: (request: NoteWriteRequest) => NoteWriteResponse | CoreError = () => ({
    outcome: 'saved',
    version: version(this.next++),
  });
  #held: (() => void)[] = [];
  hold = false;

  read(): Promise<NoteReadResponse> {
    return Promise.reject(new Error('не используется'));
  }

  write(request: NoteWriteRequest): Promise<NoteWriteResponse> {
    this.writes.push(request);
    const answer = () => {
      const response = this.respond(request);
      if ('code' in response) throw new CoreCallError('note_write', response);
      return response;
    };
    if (!this.hold) return Promise.resolve().then(answer);
    return new Promise((resolve, reject) => {
      this.#held.push(() => {
        try {
          resolve(answer());
        } catch (error) {
          reject(error as Error);
        }
      });
    });
  }

  release(): void {
    const held = this.#held;
    this.#held = [];
    for (const go of held) go();
  }
}

function session(store: FakeStore, read = opened('# Встреча\n')) {
  const statuses: SaveStatus['kind'][] = [];
  const conflicts: string[][] = [];
  const failures: unknown[] = [];
  const s = new NoteSession(PATH, read, store, {
    onStatus: (status) => statuses.push(status.kind),
    onConflictSaved: (copy) => conflicts.push(copy),
    onSaveFailed: (error) => failures.push(error),
  });
  return { s, statuses, conflicts, failures };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('автосохранение', () => {
  it('без правок ничего не пишет — ни по таймеру, ни при сохранении по требованию', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    await vi.advanceTimersByTimeAsync(10 * AUTOSAVE_DELAY_MS);
    await s.flush();

    expect(store.writes).toEqual([]);
    expect(s.status.kind).toBe('saved');
  });

  it('пишет через 1 с после последней правки, а не после первой', async () => {
    expect(AUTOSAVE_DELAY_MS).toBe(1000);
    const store = new FakeStore();
    const { s } = session(store);

    s.edit('# Встреча\nа');
    await vi.advanceTimersByTimeAsync(600);
    s.edit('# Встреча\nаб');
    await vi.advanceTimersByTimeAsync(999);
    expect(store.writes).toEqual([]);
    expect(s.status.kind).toBe('pending');

    await vi.advanceTimersByTimeAsync(1);
    expect(store.writes).toEqual([{ path: PATH, text: '# Встреча\nаб', expected_version: version(1) }]);
    expect(s.status.kind).toBe('saved');
  });

  it('следующая запись идёт с версией из ответа ядра', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    s.edit('1');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    s.edit('12');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);

    expect(store.writes.map((w) => [w.text, w.expected_version.size])).toEqual([
      ['1', 1],
      ['12', 2],
    ]);
  });

  it('решение 91: при непрерывном вводе запись не реже чем раз в 5 с', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    // Клавиша каждые 300 мс, 12 с без паузы ≥ 1 с.
    let text = '';
    for (let i = 0; i < 40; i += 1) {
      text += 'x';
      s.edit(text);
      await vi.advanceTimersByTimeAsync(300);
    }

    // Записи на 5-й и 10-й секунде — с тем, что набрано к этому моменту.
    expect(store.writes.length).toBe(2);
    expect(store.writes[0]?.text.length).toBeGreaterThanOrEqual(16);
    expect(store.writes[0]?.text.length).toBeLessThanOrEqual(17);
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(store.writes.at(-1)?.text).toBe(text);
    expect(s.status.kind).toBe('saved');
  });

  it('решение 91: срок 5 с отсчитывается от первой несохранённой правки, а не от прошлой записи', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    s.edit('a');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(store.writes).toHaveLength(1);
    // Долгая пауза, потом снова непрерывный ввод: первая запись — через 5 с от его начала.
    await vi.advanceTimersByTimeAsync(60_000);
    let text = 'a';
    for (let i = 0; i < 16; i += 1) {
      text += 'b';
      s.edit(text);
      await vi.advanceTimersByTimeAsync(300);
    }
    expect(store.writes).toHaveLength(1);
    s.edit(text + 'c');
    await vi.advanceTimersByTimeAsync(300);
    expect(store.writes).toHaveLength(2);
  });

  it('правка и её отмена до таймера — записи нет', async () => {
    const store = new FakeStore();
    const { s } = session(store, opened('abc'));

    s.edit('abcd');
    s.edit('abc');
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_DELAY_MS);

    expect(store.writes).toEqual([]);
    expect(s.status.kind).toBe('saved');
  });

  it('ввод во время записи не теряется: после ответа уходит вторая запись', async () => {
    const store = new FakeStore();
    store.hold = true;
    const { s } = session(store);

    s.edit('a');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(store.writes).toHaveLength(1);
    s.edit('ab');
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_DELAY_MS);
    expect(store.writes).toHaveLength(1);
    expect(s.status.kind).toBe('pending');

    store.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.writes.map((w) => w.text)).toEqual(['a', 'ab']);
    expect(store.writes[1]?.expected_version).toEqual(version(2));
    store.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(s.status.kind).toBe('saved');
  });

  it('индикатор: «сохраняется» при правке, «сохранено» после ответа', async () => {
    const store = new FakeStore();
    const { s, statuses } = session(store);

    s.edit('x');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);

    expect(statuses).toEqual(['pending', 'saved']);
  });
});

describe('сохранение по требованию (переход, закрытие)', () => {
  it('пишет сразу, не дожидаясь таймера, и запрос уходит синхронно', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    s.edit('новый текст');
    const done = s.flush();
    // Запрос уже отправлен — важно при закрытии окна (pagehide).
    expect(store.writes.map((w) => w.text)).toEqual(['новый текст']);
    await done;

    expect(s.status.kind).toBe('saved');
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_DELAY_MS);
    expect(store.writes).toHaveLength(1);
  });

  it('во время записи дожидается её и дописывает последний ввод', async () => {
    const store = new FakeStore();
    store.hold = true;
    const { s } = session(store);

    s.edit('a');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    s.edit('ab');
    const done = s.flush();
    store.release();
    await vi.advanceTimersByTimeAsync(0);
    store.release();
    await done;

    expect(store.writes.map((w) => w.text)).toEqual(['a', 'ab']);
    expect(s.hasUnsavedChanges).toBe(false);
  });
});

describe('конфликт изменений', () => {
  it('ядро сохранило обе версии: сообщение с путём копии, дальше пишем с новой версией', async () => {
    const store = new FakeStore();
    const copy = ['Проекты', 'Встреча (внешняя версия 2026-10-25 12-00).md'];
    store.respond = () => ({ outcome: 'conflict_saved', version: version(7), copy_path: copy });
    const { s, conflicts } = session(store);

    s.edit('мой текст');
    await s.flush();
    expect(conflicts).toEqual([copy]);
    expect(s.status.kind).toBe('saved');

    store.respond = () => ({ outcome: 'saved', version: version(8) });
    s.edit('мой текст 2');
    await s.flush();
    expect(store.writes[1]?.expected_version).toEqual(version(7));
  });
});

describe('ошибки записи', () => {
  const ioFailed: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };

  it('сбой: текст остаётся несохранённым, одно сообщение на серию сбоев, повтор через 5 с', async () => {
    const store = new FakeStore();
    store.respond = () => ioFailed;
    const { s, failures } = session(store);

    s.edit('важное');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(s.status.kind).toBe('failed');
    expect(s.hasUnsavedChanges).toBe(true);
    expect(failures).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(RETRY_DELAY_MS);
    expect(store.writes).toHaveLength(2);
    expect(failures).toHaveLength(1);

    store.respond = () => ({ outcome: 'saved', version: version(3) });
    await vi.advanceTimersByTimeAsync(RETRY_DELAY_MS);
    expect(store.writes.map((w) => w.text)).toEqual(['важное', 'важное', 'важное']);
    expect(s.status.kind).toBe('saved');
    expect(s.hasUnsavedChanges).toBe(false);
  });

  it('сохранение по требованию при сбое завершается, не зацикливаясь', async () => {
    const store = new FakeStore();
    store.respond = () => ioFailed;
    const { s } = session(store);

    s.edit('x');
    await s.flush();

    expect(store.writes).toHaveLength(1);
    expect(s.status.kind).toBe('failed');
  });

  it('not_found при записи — не удаление: текст остаётся, статус «нет файла», без повторов по таймеру', async () => {
    const store = new FakeStore();
    store.respond = () => ({ code: 'not_found', message_key: 'errors.not_found' });
    const { s, failures } = session(store);

    s.edit('x');
    await s.flush();
    s.edit('xy');
    await vi.advanceTimersByTimeAsync(10 * RETRY_DELAY_MS);

    expect(s.status.kind).toBe('missing');
    expect(s.hasUnsavedChanges).toBe(true);
    expect(s.text).toBe('xy');
    expect(failures).toHaveLength(1);
    // Правка во время «нет файла» тоже не пишется вслепую по таймеру.
    expect(store.writes).toHaveLength(1);
  });

  it('после «нет файла» переименование (moveTo) повторяет запись по новому пути с прежней версией', async () => {
    const store = new FakeStore();
    store.respond = (request) =>
      request.path[0] === 'Новое.md'
        ? { outcome: 'saved', version: version(5) }
        : { code: 'not_found', message_key: 'errors.not_found' };
    const { s } = session(store);

    s.edit('ввод');
    await s.flush();
    expect(s.status.kind).toBe('missing');

    s.moveTo(['Новое.md']);
    await vi.advanceTimersByTimeAsync(0);

    expect(store.writes.map((w) => [w.path, w.text, w.expected_version])).toEqual([
      [PATH, 'ввод', version(1)],
      [['Новое.md'], 'ввод', version(1)],
    ]);
    expect(s.status.kind).toBe('saved');
  });

  it('discard (удаление) — ввод отбрасывается, статус «удалена» не перезаписывается ответом записи в полёте', async () => {
    const store = new FakeStore();
    store.hold = true;
    const { s } = session(store);

    s.edit('x');
    void s.flush();
    s.discard();
    store.release();
    await vi.advanceTimersByTimeAsync(0);

    expect(s.status.kind).toBe('deleted');
    expect(s.hasUnsavedChanges).toBe(false);
  });
});

describe('только чтение', () => {
  it.each([
    [{ reason: 'too_large', limit_bytes: 10 * 1024 * 1024 } as const],
    [{ reason: 'invalid_utf8' } as const],
  ])('%o: правки не принимаются и не пишутся', async (reason) => {
    const store = new FakeStore();
    const { s } = session(store, opened('текст', reason));

    s.edit('другое');
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_DELAY_MS);
    await s.flush();

    expect(store.writes).toEqual([]);
    expect(s.status).toEqual({ kind: 'read_only', reason });
    expect(s.text).toBe('текст');
  });
});

describe('закрытие сессии', () => {
  it('dispose снимает таймеры: после него ничего не пишется', async () => {
    const store = new FakeStore();
    const { s } = session(store);

    s.edit('x');
    s.dispose();
    await vi.advanceTimersByTimeAsync(10 * RETRY_DELAY_MS);

    expect(store.writes).toEqual([]);
  });
});
