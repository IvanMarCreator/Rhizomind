// Запись ввода перед переписыванием ссылок (0.2-5.1, С2-5.4 шаг 2, Р7):
// по `links:flush-requested` окно записывает ввод всех своих заметок тем же
// путём, что при закрытии окна, и только потом отвечает `links_flush_done`.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileVersion, NoteReadResponse, NoteWriteRequest, NoteWriteResponse, VaultPath } from '../api/index.ts';
import { CoreCallError } from '../api/index.ts';
import type { NoteStore } from '../editor/session.ts';
import { EditorSessions, type EditorPane, type UnsavedNote } from '../editor/sessions.ts';
import { LinksFlush, type FlushHost } from './flush.ts';

const TOKEN = 'a'.repeat(32);
const A = ['А.md'];
const B = ['Папка', 'Б.md'];
const key = (path: VaultPath) => path.join('/');
const version = (n: number): FileVersion => ({ hash: String(n).padStart(64, '0'), size: n, modified_ms: n });

/** Диск в памяти: запись — с проверкой версии, как у ядра; можно задержать ответ. */
class Store implements NoteStore {
  files = new Map<string, string>([
    [key(A), 'текст А'],
    [key(B), 'текст Б'],
  ]);
  generation = new Map<string, number>();
  fail = false;

  read(path: VaultPath): Promise<NoteReadResponse> {
    const text = this.files.get(key(path)) ?? '';
    return Promise.resolve({ text, version: version(this.generation.get(key(path)) ?? 1), read_only: null });
  }

  write(request: NoteWriteRequest): Promise<NoteWriteResponse> {
    const fail = this.fail;
    // Ответ записи — не сразу: окно должно дождаться его, а не только отправить.
    return new Promise((resolve, reject) =>
      setTimeout(() => {
        if (fail) {
          reject(new CoreCallError('note_write', { code: 'io_failed', message_key: 'errors.io_failed' }));
          return;
        }
        const name = key(request.path);
        const next = (this.generation.get(name) ?? 1) + 1;
        this.files.set(name, request.text);
        this.generation.set(name, next);
        resolve({ outcome: 'saved', version: version(next) });
      }, 50),
    );
  }
}

type Open = Extract<EditorPane, { kind: 'open' }>;

function windowWith(store: Store) {
  let pane: EditorPane = { kind: 'empty' };
  const notices: string[] = [];
  const sessions = new EditorSessions(store, {
    onPane: (next) => (pane = next),
    onStatus: () => undefined,
    notify: (text) => notices.push(text),
  });
  return { sessions, notices, pane: () => pane };
}

function hostFor(flush: () => Promise<readonly UnsavedNote[]>, store?: Store) {
  const answers: { token: string; saved: boolean; disk: Record<string, string> }[] = [];
  const notices: string[] = [];
  const host: FlushHost = {
    flush,
    done: (token, saved) => {
      answers.push({ token, saved, disk: Object.fromEntries(store?.files ?? []) });
      return Promise.resolve();
    },
    notify: (text) => notices.push(text),
  };
  return { host, answers, notices };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('links:flush-requested → links_flush_done', () => {
  it('ввод всех заметок записан до links_flush_done', async () => {
    const store = new Store();
    const { sessions, pane } = windowWith(store);
    sessions.show(A);
    await vi.advanceTimersByTimeAsync(0);
    (pane() as Open).session.edit('А, ввод');
    store.fail = true;
    sessions.show(B);
    await vi.advanceTimersByTimeAsync(100);
    store.fail = false;
    (pane() as Open).session.edit('Б, ввод');
    const { host, answers } = hostFor(() => sessions.flushForLinks(), store);

    const done = new LinksFlush(host).requested({ token: TOKEN });
    await vi.advanceTimersByTimeAsync(10);
    expect(answers).toEqual([]);
    await vi.advanceTimersByTimeAsync(200);
    await done;

    expect(answers).toEqual([{ token: TOKEN, saved: true, disk: { 'А.md': 'А, ввод', 'Папка/Б.md': 'Б, ввод' } }]);
  });

  it('записывать нечего — ответ сразу, без записей', async () => {
    const { host, answers } = hostFor(() => Promise.resolve([]));

    await new LinksFlush(host).requested({ token: TOKEN });

    expect(answers.map(({ token, saved }) => ({ token, saved }))).toEqual([{ token: TOKEN, saved: true }]);
  });

  it('ввод записать не удалось — saved: false (ссылки не переписываются) и уведомление с именем заметки', async () => {
    const unsaved: UnsavedNote[] = [
      { path: A, error: new CoreCallError('note_write', { code: 'io_failed', message_key: 'errors.io_failed' }) },
    ];
    const { host, answers, notices } = hostFor(() => Promise.resolve(unsaved));

    await new LinksFlush(host).requested({ token: TOKEN });

    expect(answers.map(({ saved }) => saved)).toEqual([false]);
    expect(notices).toHaveLength(1);
    expect(notices[0]).toContain('«А»');
    expect(notices[0]).toContain('Ссылки не обновлены');
  });

  it('запись бросила исключение — всё равно ответ ядру: saved: false', async () => {
    const { host, answers } = hostFor(() => Promise.reject(new Error('сбой')));

    await new LinksFlush(host).requested({ token: TOKEN });

    expect(answers.map(({ saved }) => saved)).toEqual([false]);
  });

  it('каждый запрос получает свой ответ со своим знаком', async () => {
    const { host, answers } = hostFor(() => Promise.resolve([]));
    const flush = new LinksFlush(host);

    await Promise.all([flush.requested({ token: TOKEN }), flush.requested({ token: 'b'.repeat(32) })]);

    expect(answers.map(({ token }) => token).sort()).toEqual([TOKEN, 'b'.repeat(32)]);
  });

  it('ответ ядру не дошёл — исключение не выходит наружу', async () => {
    const host: FlushHost = {
      flush: () => Promise.resolve([]),
      done: () => Promise.reject(new Error('мост')),
      notify: () => undefined,
    };

    await expect(new LinksFlush(host).requested({ token: TOKEN })).resolves.toBeUndefined();
  });
});
