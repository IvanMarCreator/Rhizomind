// Открытая заметка окна и переходы между заметками (С3.3): при уходе с
// заметки её ввод записывается сразу; заметка, чью запись не удалось
// завершить, остаётся в памяти — при возврате показывается её текст, а не
// файл с диска, и повтор записи продолжается. При закрытии окна —
// `flushAll()`.
import type { VaultPath } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';
import { NoteSession, type NoteStore, type SaveStatus } from './session.ts';

/** Что показывает область заметки. */
export type EditorPane =
  | { kind: 'empty' }
  | { kind: 'loading'; path: VaultPath }
  | { kind: 'open'; session: NoteSession }
  | { kind: 'failed'; path: VaultPath; error: unknown };

export interface EditorListener {
  onPane(pane: EditorPane): void;
  /** Состояние сохранения открытой заметки; `null` — заметки нет. */
  onStatus(status: SaveStatus | null): void;
  /** Уведомление внизу окна. */
  notify(text: string): void;
}

/**
 * Ключ пути. Пути приходят от ядра и из раскладки в том виде, как записаны на
 * диске, поэтому сравнение побуквенное (приведение по ключу имени — в ядре).
 */
function pathKey(path: VaultPath): string {
  return JSON.stringify(path);
}

/** Имя заметки для текстов: последний компонент без `.md`. */
export function noteName(path: VaultPath): string {
  return (path.at(-1) ?? '').replace(/\.md$/i, '');
}

export class EditorSessions {
  readonly #store: NoteStore;
  readonly #listener: EditorListener;
  #pane: EditorPane = { kind: 'empty' };
  /** Заметки с вводом, который ещё пишется или не записался. */
  readonly #unsaved = new Map<string, NoteSession>();
  /** Номер последнего перехода: ответ на чтение старого перехода отбрасывается. */
  #showToken = 0;

  constructor(store: NoteStore, listener: EditorListener) {
    this.#store = store;
    this.#listener = listener;
  }

  get pane(): EditorPane {
    return this.#pane;
  }

  /** Показать заметку; `null` — закрыть (пустая область). */
  show(path: VaultPath | null): void {
    const current = this.#currentSession();
    if (path && current && pathKey(current.path) === pathKey(path)) return;
    if (path && this.#pane.kind === 'loading' && pathKey(this.#pane.path) === pathKey(path)) return;
    const token = ++this.#showToken;
    if (current) this.#leave(current);

    if (!path) {
      this.#setPane({ kind: 'empty' });
      return;
    }
    const kept = this.#unsaved.get(pathKey(path));
    if (kept) {
      this.#setPane({ kind: 'open', session: kept });
      return;
    }
    this.#setPane({ kind: 'loading', path });
    this.#store.read(path).then(
      (opened) => {
        if (token !== this.#showToken) return;
        const session: NoteSession = new NoteSession(path, opened, this.#store, this.#hooks(path, () => session));
        this.#setPane({ kind: 'open', session });
      },
      (error: unknown) => {
        if (token !== this.#showToken) return;
        this.#setPane({ kind: 'failed', path, error });
      },
    );
  }

  /** Записать ввод всех заметок окна (закрытие окна). Запросы уходят синхронно. */
  async flushAll(): Promise<void> {
    const sessions = new Set(this.#unsaved.values());
    const current = this.#currentSession();
    if (current) sessions.add(current);
    await Promise.all([...sessions].map((session) => session.flush()));
  }

  #currentSession(): NoteSession | null {
    return this.#pane.kind === 'open' ? this.#pane.session : null;
  }

  /** Уход с заметки: запись сразу; пока ввод не записан, сессия хранится. */
  #leave(session: NoteSession): void {
    if (!session.hasUnsavedChanges) {
      session.dispose();
      return;
    }
    const key = pathKey(session.path);
    this.#unsaved.set(key, session);
    void session.flush().then(() => this.#forgetIfDone(session));
  }

  #forgetIfDone(session: NoteSession): void {
    if (session.hasUnsavedChanges || this.#currentSession() === session) return;
    const key = pathKey(session.path);
    if (this.#unsaved.get(key) === session) this.#unsaved.delete(key);
    session.dispose();
  }

  #hooks(path: VaultPath, session: () => NoteSession) {
    const name = noteName(path);
    return {
      onStatus: (status: SaveStatus) => {
        const own = session();
        if (this.#currentSession() === own) this.#listener.onStatus(status);
        // Повтор после сбоя дописал ввод заметки, с которой уже ушли.
        if (status.kind === 'saved' || status.kind === 'deleted') this.#forgetIfDone(own);
      },
      onConflictSaved: (copy: VaultPath) => {
        this.#listener.notify(t('editor.conflictSaved', { name, copy: noteName(copy) }));
      },
      onSaveFailed: (error: unknown) => {
        this.#listener.notify(t('editor.saveFailed', { name, message: userMessage(error) }));
      },
    };
  }

  #setPane(pane: EditorPane): void {
    this.#pane = pane;
    this.#listener.onPane(pane);
    this.#listener.onStatus(pane.kind === 'open' ? pane.session.status : null);
  }
}
