// Переход между заметками: прежняя сохраняется сразу, несохранённый ввод
// не теряется ни при сбое записи, ни при быстрых переходах туда и обратно.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, FileVersion, NoteReadResponse, NoteWriteRequest, NoteWriteResponse, VaultPath } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import type { NoteStore, SaveStatus } from './session.ts';
import { EditorSessions, type EditorPane } from './sessions.ts';

const A = ['А.md'];
const B = ['Папка', 'Б.md'];
const v1: FileVersion = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };
const v2: FileVersion = { hash: 'b'.repeat(64), size: 2, modified_ms: 2 };

const key = (path: VaultPath) => path.join('/');

class FakeStore implements NoteStore {
  files = new Map<string, string>([
    [key(A), 'текст А'],
    [key(B), 'текст Б'],
  ]);
  reads: string[] = [];
  writes: NoteWriteRequest[] = [];
  failWrites: CoreError | null = null;
  pendingReads = new Map<string, () => void>();
  holdReads = false;

  read(path: VaultPath): Promise<NoteReadResponse> {
    this.reads.push(key(path));
    const answer = (): NoteReadResponse => {
      const text = this.files.get(key(path));
      if (text === undefined) throw new CoreCallError('note_read', { code: 'not_found', message_key: 'errors.not_found' });
      return { text, version: v1, read_only: null };
    };
    if (!this.holdReads) return Promise.resolve().then(answer);
    return new Promise((resolve, reject) =>
      this.pendingReads.set(key(path), () => {
        try {
          resolve(answer());
        } catch (error) {
          reject(error as Error);
        }
      }),
    );
  }

  write(request: NoteWriteRequest): Promise<NoteWriteResponse> {
    this.writes.push(request);
    const failure = this.failWrites;
    return Promise.resolve().then(() => {
      if (failure) throw new CoreCallError('note_write', failure);
      this.files.set(key(request.path), request.text);
      return { outcome: 'saved', version: v2 } as const;
    });
  }
}

function setup(store = new FakeStore()) {
  const notices: string[] = [];
  const statuses: (SaveStatus['kind'] | null)[] = [];
  let pane: EditorPane = { kind: 'empty' };
  const sessions = new EditorSessions(store, {
    onPane: (next) => (pane = next),
    onStatus: (status) => statuses.push(status?.kind ?? null),
    notify: (text) => notices.push(text),
  });
  return { store, sessions, notices, statuses, pane: () => pane };
}

function openText(pane: EditorPane): string | null {
  return pane.kind === 'open' ? pane.session.text : null;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('переход между заметками', () => {
  it('открывает заметку текстом из ядра; индикатор — «сохранено»', async () => {
    const { sessions, pane, statuses } = setup();

    sessions.show(A);
    expect(pane()).toEqual({ kind: 'loading', path: A });
    await vi.advanceTimersByTimeAsync(0);

    expect(openText(pane())).toBe('текст А');
    expect(statuses.at(-1)).toBe('saved');
  });

  it('при переходе несохранённый ввод прежней заметки записывается сразу, без ожидания таймера', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const session = pane().kind === 'open' ? (pane() as { session: { edit(t: string): void } }).session : null;
    session?.edit('текст А, правка');

    sessions.show(B);
    expect(store.writes.map((w) => [key(w.path), w.text])).toEqual([['А.md', 'текст А, правка']]);
    await vi.advanceTimersByTimeAsync(0);

    expect(openText(pane())).toBe('текст Б');
    expect(store.files.get(key(A))).toBe('текст А, правка');
  });

  it('та же заметка ещё раз — ничего не перечитывается', async () => {
    const { sessions, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    sessions.show([...A]);
    await vi.advanceTimersByTimeAsync(0);

    expect(store.reads).toEqual(['А.md']);
  });

  it('быстрые переходы: показывается последняя заметка, а не та, что прочиталась позже', async () => {
    const store = new FakeStore();
    store.holdReads = true;
    const { sessions, pane } = setup(store);

    sessions.show(A);
    sessions.show(B);
    store.pendingReads.get(key(B))?.();
    await vi.advanceTimersByTimeAsync(0);
    store.pendingReads.get(key(A))?.();
    await vi.advanceTimersByTimeAsync(0);

    expect(openText(pane())).toBe('текст Б');
  });

  it('запись не удалась при уходе: при возврате — тот же несохранённый текст, а не файл с диска', async () => {
    const { sessions, pane, store, notices } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    if (pane().kind === 'open') (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('несохранённое');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };

    sessions.show(B);
    await vi.advanceTimersByTimeAsync(0);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);

    expect(openText(pane())).toBe('несохранённое');
    expect(store.reads).toEqual(['А.md', 'Папка/Б.md']);
    expect(notices).toEqual([
      'Не удалось сохранить заметку «А». Не удалось прочитать или записать файл. Текст остаётся в редакторе, сохранение повторится.',
    ]);

    // Повтор после сбоя дописывает текст.
    store.failWrites = null;
    await vi.advanceTimersByTimeAsync(5000);
    expect(store.files.get(key(A))).toBe('несохранённое');
  });

  it('заметку не удалось открыть — понятная ошибка вместо редактора', async () => {
    const { sessions, pane } = setup();

    sessions.show(['Нет такой.md']);
    await vi.advanceTimersByTimeAsync(0);

    expect(pane()).toMatchObject({ kind: 'failed', path: ['Нет такой.md'] });
  });

  it('закрытие вкладки (null): ввод записан, область пуста, индикатора нет', async () => {
    const { sessions, pane, store, statuses } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('итог');

    sessions.show(null);
    await vi.advanceTimersByTimeAsync(0);

    expect(store.files.get(key(A))).toBe('итог');
    expect(pane()).toEqual({ kind: 'empty' });
    expect(statuses.at(-1)).toBeNull();
  });
});

describe('закрытие окна', () => {
  it('flushAll отправляет запись синхронно и дописывает все заметки с несохранённым вводом', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('А2');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };
    sessions.show(B);
    await vi.advanceTimersByTimeAsync(0);
    store.failWrites = null;
    (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('Б2');

    const done = sessions.flushAll();
    expect(store.writes.slice(-2).map((w) => w.text).sort()).toEqual(['А2', 'Б2']);
    await done;

    expect(store.files.get(key(A))).toBe('А2');
    expect(store.files.get(key(B))).toBe('Б2');
  });
});

describe('конфликт изменений', () => {
  it('уведомление по спецификации: обе версии сохранены, имя копии', async () => {
    const store = new FakeStore();
    store.write = (request) => {
      store.writes.push(request);
      return Promise.resolve({
        outcome: 'conflict_saved',
        version: v2,
        copy_path: ['А (внешняя версия 2026-10-25 12-00).md'],
      });
    };
    const { sessions, pane, notices } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('моё');

    await sessions.flushAll();

    expect(notices).toEqual([
      'Заметка «А» изменилась на диске одновременно с вашими правками. Сохранены обе версии: ваша — в заметке, версия с диска — рядом, в «А (внешняя версия 2026-10-25 12-00)».',
    ]);
  });
});
