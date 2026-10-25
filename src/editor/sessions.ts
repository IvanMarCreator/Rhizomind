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

/** `path` лежит в `prefix` или равен ему (заметка или папка). */
function within(path: VaultPath, prefix: VaultPath): boolean {
  return prefix.length <= path.length && prefix.every((part, i) => part === path[i]);
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
        const session: NoteSession = new NoteSession(path, opened, this.#store, this.#hooks(() => session));
        this.#setPane({ kind: 'open', session });
      },
      (error: unknown) => {
        if (token !== this.#showToken) return;
        this.#setPane({ kind: 'failed', path, error });
      },
    );
  }

  /**
   * Заметку или папку переименовали либо переместили (дерево, событие
   * `note:renamed`). Вызывается до того, как окно откроет новый путь: тогда
   * следующий `show(to)` — та же заметка, а не уход с неё, и ввод пишется по
   * новому пути с прежней ожидаемой версией.
   */
  renamed(from: VaultPath, to: VaultPath): void {
    const current = this.#currentSession();
    for (const session of this.#sessions()) {
      if (!within(session.path, from)) continue;
      const key = pathKey(session.path);
      const moved = [...to, ...session.path.slice(from.length)];
      if (this.#unsaved.get(key) === session) this.#unsaved.delete(key);
      if (session !== current && current && pathKey(current.path) === pathKey(moved) && !current.hasUnsavedChanges) {
        // Окно уже открыло новый путь, прочитав файл с диска, а ввод остался в
        // сессии старого пути: показать её вместо свежей.
        current.dispose();
        session.moveTo(moved);
        this.#setPane({ kind: 'open', session });
        continue;
      }
      session.moveTo(moved);
      if (session !== this.#currentSession()) this.#unsaved.set(pathKey(session.path), session);
    }
    if ((this.#pane.kind === 'loading' || this.#pane.kind === 'failed') && within(this.#pane.path, from)) {
      // Чтение по старому пути идёт или не удалось: перечитать по новому.
      const path = [...to, ...this.#pane.path.slice(from.length)];
      this.#pane = { kind: 'empty' };
      this.show(path);
    }
  }

  /**
   * Заметку или папку удалили (дерево, событие `note:deleted`): несохранённый
   * ввод отбрасывается, при закрытии ничего не пишется (решение владельца).
   */
  removed(path: VaultPath): void {
    for (const session of this.#sessions()) {
      if (!within(session.path, path)) continue;
      session.discard();
      this.#unsaved.delete(pathKey(session.path));
    }
  }

  /**
   * Редактор убран со страницы: последняя попытка записать ввод (запросы
   * уходят синхронно), потом таймеры всех заметок снимаются.
   */
  close(): void {
    const sessions = this.#sessions();
    this.#unsaved.clear();
    this.#setPane({ kind: 'empty' });
    for (const session of sessions) {
      void session.flush().finally(() => {
        if (session.hasUnsavedChanges) {
          const status = session.status;
          const message =
            status.kind === 'failed' ? userMessage(status.error) : t('errors.not_found');
          this.#listener.notify(t('editor.closeUnsaved', { name: noteName(session.path), message }));
        }
        session.dispose();
      });
    }
  }

  #sessions(): NoteSession[] {
    const all = new Set(this.#unsaved.values());
    const current = this.#currentSession();
    if (current) all.add(current);
    return [...all];
  }

  /** Записать ввод всех заметок окна (закрытие окна). Запросы уходят синхронно. */
  async flushAll(): Promise<void> {
    await Promise.all(this.#sessions().map((session) => session.flush()));
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

  #hooks(session: () => NoteSession) {
    // Имя — по текущему пути: заметку могли переименовать.
    const name = () => noteName(session().path);
    return {
      onStatus: (status: SaveStatus) => {
        const own = session();
        if (this.#currentSession() === own) this.#listener.onStatus(status);
        // Повтор после сбоя дописал ввод заметки, с которой уже ушли.
        if (status.kind === 'saved' || status.kind === 'deleted') this.#forgetIfDone(own);
      },
      onConflictSaved: (copy: VaultPath) => {
        this.#listener.notify(t('editor.conflictSaved', { name: name(), copy: noteName(copy) }));
      },
      onSaveFailed: (error: unknown) => {
        this.#listener.notify(t('editor.saveFailed', { name: name(), message: userMessage(error) }));
      },
    };
  }

  #setPane(pane: EditorPane): void {
    this.#pane = pane;
    this.#listener.onPane(pane);
    this.#listener.onStatus(pane.kind === 'open' ? pane.session.status : null);
  }
}
