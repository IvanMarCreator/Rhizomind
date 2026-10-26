// Переход между заметками: прежняя сохраняется сразу, несохранённый ввод
// не теряется ни при сбое записи, ни при быстрых переходах туда и обратно.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CoreError, FileVersion, NoteReadResponse, NoteWriteRequest, NoteWriteResponse, VaultPath } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import type { NoteStore, SaveStatus } from './session.ts';
import { CLOSE_FLUSH_DEADLINE_MS, EditorSessions, MISSING_GRACE_MS, SaveTooSlow, type EditorPane } from './sessions.ts';

const A = ['А.md'];
const B = ['Папка', 'Б.md'];
const v1: FileVersion = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };
const v2: FileVersion = { hash: 'b'.repeat(64), size: 2, modified_ms: 2 };

const key = (path: VaultPath) => path.join('/');

/** Версия файла по номеру поколения: 1 — v1, 2 — v2, дальше — свои. */
function versionOf(generation: number): FileVersion {
  if (generation === 1) return v1;
  if (generation === 2) return v2;
  return { hash: String(generation).padStart(64, '0'), size: generation, modified_ms: generation };
}

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
  /** Поколение файла: растёт с каждой записью; нет записи — 1 (v1). */
  generations = new Map<string, number>();

  #generation(name: string): number {
    return this.generations.get(name) ?? 1;
  }

  read(path: VaultPath): Promise<NoteReadResponse> {
    this.reads.push(key(path));
    const answer = (): NoteReadResponse => {
      const text = this.files.get(key(path));
      if (text === undefined) throw new CoreCallError('note_read', { code: 'not_found', message_key: 'errors.not_found' });
      return { text, version: versionOf(this.#generation(key(path))), read_only: null };
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
      if (!this.files.has(key(request.path))) {
        throw new CoreCallError('note_write', { code: 'not_found', message_key: 'errors.not_found' });
      }
      // Как ядро: ожидаемая версия не совпала — версия с диска копией рядом.
      const name = key(request.path);
      const generation = this.#generation(name);
      const conflict = request.expected_version.hash !== versionOf(generation).hash;
      const copy = [...request.path.slice(0, -1), `${(request.path.at(-1) ?? '').replace(/\.md$/, '')} (внешняя версия).md`];
      if (conflict) this.files.set(key(copy), this.files.get(name) ?? '');
      this.files.set(name, request.text);
      this.generations.set(name, generation + 1);
      const version = versionOf(generation + 1);
      return conflict
        ? ({ outcome: 'conflict_saved', version, copy_path: copy } as const)
        : ({ outcome: 'saved', version } as const);
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

describe('переименование и удаление (сигналы от дерева и событий ядра)', () => {
  type Open = Extract<EditorPane, { kind: 'open' }>;

  it('переименование открытой заметки — не уход: буфер переходит на новый путь, запись по новому пути с прежней версией', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('правка');
    const renamed = ['Папка', 'А2.md'];
    store.files.set(key(renamed), 'текст А');
    store.files.delete(key(A));

    sessions.renamed(A, renamed);
    sessions.show(renamed);
    expect(store.writes).toEqual([]);
    expect(store.reads).toEqual(['А.md']);
    expect((pane() as Open).session.text).toBe('правка');

    await vi.advanceTimersByTimeAsync(1000);
    expect(store.writes).toEqual([{ path: renamed, text: 'правка', expected_version: v1 }]);
  });

  it('переименование папки переносит заметки внутри неё, в том числе несохранённые', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(B);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('Б2');

    sessions.renamed(['Папка'], ['Новая']);
    sessions.show(['Новая', 'Б.md']);
    store.files.set('Новая/Б.md', 'текст Б');
    await sessions.flushAll();

    expect(store.writes.map((w) => w.path)).toEqual([['Новая', 'Б.md']]);
  });

  it('запись в полёте ушла по старому пути и получила not_found — повтор по новому пути, а не «удалена»', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const session = (pane() as Open).session;
    session.edit('текст');
    const flight = session.flush();
    // Файл переименовали, пока запрос был в пути.
    store.files.delete(key(A));
    store.files.set('А2.md', 'текст А');
    sessions.renamed(A, ['А2.md']);
    await flight;
    await vi.advanceTimersByTimeAsync(0);

    expect(store.writes.map((w) => w.path)).toEqual([A, ['А2.md']]);
    expect(store.files.get('А2.md')).toBe('текст');
    expect(session.status.kind).toBe('saved');
  });

  it('удалённая заметка не сохраняется при закрытии — ввод отбрасывается (решение владельца)', async () => {
    const { sessions, pane, store, statuses } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('несохранённое');

    sessions.removed(A);
    sessions.show(null);
    await sessions.flushAll();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(store.writes).toEqual([]);
    expect(statuses.at(-1)).toBeNull();
  });

  it('удаление папки отбрасывает и отложенные записи заметок внутри неё', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(B);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('Б2');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const before = store.writes.length;

    sessions.removed(['Папка']);
    store.failWrites = null;
    await vi.advanceTimersByTimeAsync(10_000);
    await sessions.flushAll();

    expect(store.writes.length).toBe(before);
    sessions.show(B);
    expect(store.reads.at(-1)).toBe('Папка/Б.md');
  });
});

describe('редактор убран со страницы', () => {
  it('close: последняя попытка записи, потом повторов после сбоя больше нет', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Extract<EditorPane, { kind: 'open' }>).session.edit('ввод');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };

    sessions.close();
    expect(store.writes).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(60_000);

    expect(store.writes).toHaveLength(1);
    expect(pane()).toEqual({ kind: 'empty' });
  });
});

describe('ревью 2.4: переименование, о котором окно ещё не знает', () => {
  type Open = Extract<EditorPane, { kind: 'open' }>;

  it('(а) внешнее переименование: автосохранение раньше события — ввод держится и пишется по новому пути', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const session = (pane() as Open).session;
    session.edit('мой ввод');
    store.files.set('А2.md', store.files.get(key(A)) ?? '');
    store.files.delete(key(A));

    await vi.advanceTimersByTimeAsync(1000);
    expect(session.status.kind).toBe('missing');
    expect(session.text).toBe('мой ввод');

    sessions.renamed(A, ['А2.md']);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.files.get('А2.md')).toBe('мой ввод');
    expect(store.writes.at(-1)).toEqual({ path: ['А2.md'], text: 'мой ввод', expected_version: v1 });
  });

  it('(б) окно открыло новый путь раньше renamed(): ввод не теряется, после сигнала показан и записан', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('мой ввод');
    store.files.set('А2.md', store.files.get(key(A)) ?? '');
    store.files.delete(key(A));

    sessions.show(['А2.md']);
    await vi.advanceTimersByTimeAsync(0);
    expect((pane() as Open).session.text).toBe('текст А');

    sessions.renamed(A, ['А2.md']);
    await vi.advanceTimersByTimeAsync(0);

    expect((pane() as Open).session.text).toBe('мой ввод');
    expect(store.files.get('А2.md')).toBe('мой ввод');
  });

  it('renamed() при области «не удалось открыть» перечитывает по новому пути', async () => {
    const { sessions, pane, store } = setup();
    sessions.show(['Старое.md']);
    await vi.advanceTimersByTimeAsync(0);
    expect(pane().kind).toBe('failed');
    store.files.set('Новое.md', 'найдено');

    sessions.renamed(['Старое.md'], ['Новое.md']);
    await vi.advanceTimersByTimeAsync(0);

    expect(openText(pane())).toBe('найдено');
  });

  it('close: неудачная последняя запись — уведомление, а не тишина', async () => {
    const { sessions, pane, store, notices } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('ввод');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };
    (pane() as Open).session.flush();
    await vi.advanceTimersByTimeAsync(0);
    notices.length = 0;

    sessions.close();
    await vi.advanceTimersByTimeAsync(0);

    expect(notices).toEqual(['Ввод в заметке «А» не сохранён: Не удалось прочитать или записать файл.']);
  });
});

describe('повторное ревью 2.4: чтение нового пути отвечает раньше повторной записи', () => {
  type Open = Extract<EditorPane, { kind: 'open' }>;

  it('ответ на чтение уступает сессии с вводом: на экране ввод, следующая правка без конфликта', async () => {
    const store = new FakeStore();
    const { sessions, pane, notices } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const original = (pane() as Open).session;
    original.edit('мой ввод');
    store.files.set('А2.md', store.files.get(key(A)) ?? '');
    store.files.delete(key(A));

    store.holdReads = true;
    sessions.show(['А2.md']);
    await vi.advanceTimersByTimeAsync(0);
    expect(original.status.kind).toBe('missing');
    sessions.renamed(A, ['А2.md']);
    // Ядро прочитало файл до повторной записи — в ответе старый текст.
    store.pendingReads.get('А2.md')?.();
    await vi.advanceTimersByTimeAsync(0);

    expect((pane() as Open).session).toBe(original);
    expect(openText(pane())).toBe('мой ввод');
    expect(store.files.get('А2.md')).toBe('мой ввод');

    original.edit('мой ввод 2');
    await vi.advanceTimersByTimeAsync(1000);
    expect(store.files.get('А2.md')).toBe('мой ввод 2');
    expect(notices.filter((n) => n.includes('Сохранены обе версии'))).toEqual([]);
  });

  it('подменное хранилище ловит гонку версий: запись со старой версией — conflict_saved', async () => {
    const store = new FakeStore();
    const { sessions, pane, notices } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    store.generations.set(key(A), 5);

    (pane() as Open).session.edit('x');
    await sessions.flushAll();

    expect(notices).toEqual([
      'Заметка «А» изменилась на диске одновременно с вашими правками. Сохранены обе версии: ваша — в заметке, версия с диска — рядом, в «А (внешняя версия)».',
    ]);
  });
});

describe('закрытие окна (задача 0.7): flushForClose', () => {
  type Open = Extract<EditorPane, { kind: 'open' }>;

  it('записывает ввод всех заметок окна и сообщает, что не записано ничего', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('А, ввод');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };
    sessions.show(B);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('Б, ввод');
    store.failWrites = null;

    const unsaved = await sessions.flushForClose();

    expect(unsaved).toEqual([]);
    expect(store.files.get(key(A))).toBe('А, ввод');
    expect(store.files.get(key(B))).toBe('Б, ввод');
  });

  it('ждёт ответа записи, ушедшей раньше (автосохранение в полёте), и дописывает ввод после неё', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    const session = (pane() as Open).session;
    session.edit('первое');
    void session.flush();
    session.edit('первое и второе');

    expect(await sessions.flushForClose()).toEqual([]);
    expect(store.files.get(key(A))).toBe('первое и второе');
  });

  it('запись не удалась — заметка в ответе с причиной, текст остаётся в памяти', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('ввод');
    store.failWrites = { code: 'io_failed', message_key: 'errors.io_failed' };

    const unsaved = await sessions.flushForClose();

    expect(unsaved.map((note) => note.path)).toEqual([A]);
    expect(unsaved[0]?.error).toBeInstanceOf(CoreCallError);
    expect((pane() as Open).session.text).toBe('ввод');
  });

  it('файла нет (переименован снаружи, событие ещё в пути): ждёт сигнала и пишет по новому пути', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('мой ввод');
    store.files.set('А2.md', 'текст А');
    store.files.delete(key(A));

    const result = sessions.flushForClose();
    await vi.advanceTimersByTimeAsync(300);
    sessions.renamed(A, ['А2.md']);
    await vi.advanceTimersByTimeAsync(300);

    expect(await result).toEqual([]);
    expect(store.files.get('А2.md')).toBe('мой ввод');
  });

  it('файла нет и сигнала не было — после короткого ожидания заметка в ответе', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('мой ввод');
    store.files.delete(key(A));

    const result = sessions.flushForClose();
    await vi.advanceTimersByTimeAsync(MISSING_GRACE_MS);

    expect((await result).map((note) => note.path)).toEqual([A]);
  });

  it('удалённая заметка не мешает закрытию: её ввод отброшен (решение владельца)', async () => {
    const { sessions, pane } = setup();
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('ввод');
    sessions.removed(A);

    expect(await sessions.flushForClose()).toEqual([]);
  });
});

describe('ревью 0.7: ожидание при закрытии ограничено временем, а не кругами', () => {
  type Open = Extract<EditorPane, { kind: 'open' }>;

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] });
  });

  it('файла нет: пока ждём сигнала о переименовании, запись не повторяется', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('мой ввод');
    store.files.delete(key(A));

    const result = sessions.flushForClose();
    await vi.advanceTimersByTimeAsync(MISSING_GRACE_MS + 500);
    await result;

    expect(store.writes).toHaveLength(1);
  });

  it('запись зависла (медленный диск): к крайнему сроку — ответ «не записано», а не ожидание без конца', async () => {
    const store = new FakeStore();
    const { sessions, pane } = setup(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('мой ввод');
    store.write = () => new Promise(() => undefined);

    let unsaved: Awaited<ReturnType<EditorSessions['flushForClose']>> | null = null;
    void sessions.flushForClose().then((result) => (unsaved = result));
    await vi.advanceTimersByTimeAsync(CLOSE_FLUSH_DEADLINE_MS - 100);
    expect(unsaved).toBeNull();
    await vi.advanceTimersByTimeAsync(200);

    expect(unsaved).not.toBeNull();
    const notes = unsaved as unknown as { path: string[]; error: unknown }[];
    expect(notes.map((note) => note.path)).toEqual([A]);
    expect(notes[0]?.error).toBeInstanceOf(SaveTooSlow);
  });

  it('крайний срок — с запасом до предела ядра (10 с)', () => {
    expect(CLOSE_FLUSH_DEADLINE_MS).toBeLessThanOrEqual(8000);
    expect(MISSING_GRACE_MS).toBeLessThan(CLOSE_FLUSH_DEADLINE_MS);
  });
});
