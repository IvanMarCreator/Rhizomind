// Открытая заметка окна и переходы между заметками (С3.3): при уходе с
// заметки её ввод записывается сразу; заметка, чью запись не удалось
// завершить, остаётся в памяти — при возврате показывается её текст, а не
// файл с диска, и повтор записи продолжается. При закрытии окна —
// `flushForClose()` (задача 0.7): ядро закрывает окно после ответа.
import { CoreCallError, type FileVersion, type VaultPath } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';
import { NoteSession, type NoteStore, type SaveStatus } from './session.ts';

/** Что показывает область заметки. */
export type EditorPane =
  | { kind: 'empty' }
  | { kind: 'loading'; path: VaultPath }
  | { kind: 'open'; session: NoteSession }
  | { kind: 'failed'; path: VaultPath; error: unknown };

/**
 * Сколько при закрытии окна ждать сигнала о переименовании заметки, файла
 * которой нет на диске (`missing`): внешнее переименование могло случиться
 * перед самым закрытием, а событие наблюдателя ещё в пути.
 */
export const MISSING_GRACE_MS = 1500;
const MISSING_POLL_MS = 100;

/**
 * Крайний срок записи при закрытии окна — с запасом до предела ожидания ядра
 * (10 с, `CLOSE_TIMEOUT` в src-tauri/src/windows/closing.rs): не успели —
 * интерфейс сам отвечает «не записано» и окно возвращается, а не ядро
 * уничтожает его по пределу с потерей ввода. Время — по `performance.now()`:
 * таймеры скрытого окна движок может замедлять.
 */
export const CLOSE_FLUSH_DEADLINE_MS = 7000;

/** Запись не завершилась к крайнему сроку закрытия окна (диск не отвечает). */
export class SaveTooSlow extends Error {
  constructor() {
    super('note_write: не завершилась к крайнему сроку закрытия окна');
    this.name = 'SaveTooSlow';
  }
}

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** Ждёт `work` не дольше чем до `deadline` (`performance.now()`); `true` — дождались. */
async function settlesBy(work: Promise<unknown>, deadline: number): Promise<boolean> {
  let done = false;
  void work.then(
    () => (done = true),
    () => (done = true),
  );
  // Короткие шаги, а не один таймер на весь срок: замедленный таймер
  // скрытого окна не продлевает ожидание сверх крайнего срока.
  while (!done && performance.now() < deadline) {
    await Promise.race([work.catch(() => undefined), sleep(Math.min(MISSING_POLL_MS, deadline - performance.now()))]);
  }
  return done;
}

/** Метка «сверка без известной версии» для заметки, которая ещё открывается. */
const RESCANNED: FileVersion = { hash: '', size: -1, modified_ms: -1 };

/** Заметка, ввод которой при закрытии окна записать не удалось. */
export interface UnsavedNote {
  path: VaultPath;
  /** Причина: ошибка записи или `not_found` (файла нет на диске). */
  error: unknown;
}

const notFound = (): CoreCallError =>
  new CoreCallError('note_write', { code: 'not_found', message_key: 'errors.not_found' });

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
  /**
   * `note:changed` пришло, пока заметка открывалась (ключ пути → версия):
   * чтение могло опередить внешнюю запись — после открытия текст сверяется.
   */
  readonly #changedWhileLoading = new Map<string, FileVersion>();

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
    this.#changedWhileLoading.delete(pathKey(path));
    this.#setPane({ kind: 'loading', path });
    this.#store.read(path).then(
      (opened) => {
        if (token !== this.#showToken) return;
        // Пока шло чтение, сюда переехала сессия с вводом (renamed() раньше
        // ответа): показать её — ответ с диска мог опередить её запись.
        const moved = this.#unsaved.get(pathKey(path));
        if (moved) {
          this.#unsaved.delete(pathKey(path));
          this.#setPane({ kind: 'open', session: moved });
          return;
        }
        const session: NoteSession = new NoteSession(path, opened, this.#store, this.#hooks(() => session));
        this.#setPane({ kind: 'open', session });
        const changed = this.#changedWhileLoading.get(pathKey(path));
        this.#changedWhileLoading.delete(pathKey(path));
        if (changed) this.#external(session, changed);
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
   * Заметку изменили снаружи (событие `note:changed`, задача 2.9, С3.6):
   * без несохранённого ввода текст обновляется с диска (курсор сохраняет
   * редактор), с вводом — ввод пишется сразу, ядро сохраняет обе версии.
   * Касается и заметки, с которой ушли, но ввод которой ещё пишется.
   */
  changed(path: VaultPath, version: FileVersion): void {
    const key = pathKey(path);
    if (this.#pane.kind === 'loading' && pathKey(this.#pane.path) === key) this.#changedWhileLoading.set(key, version);
    if (this.#retryFailed(key)) return;
    for (const session of this.#sessions()) {
      if (pathKey(session.path) === key) this.#external(session, version);
    }
  }

  /**
   * Полная сверка с диском (`vault:changed` с `rescan: true`, хранилище снова
   * доступно): события о правке открытой заметки могло не быть — сверяем
   * её с диском, как при `changed()`.
   */
  rescanned(): void {
    if (this.#pane.kind === 'loading') this.#changedWhileLoading.set(pathKey(this.#pane.path), RESCANNED);
    if (this.#pane.kind === 'failed' && this.#retryFailed(pathKey(this.#pane.path))) return;
    for (const session of this.#sessions()) this.#external(session);
  }

  /** Заметка по ключу не открылась — открыть заново (файл появился или изменился). */
  #retryFailed(key: string): boolean {
    if (this.#pane.kind !== 'failed' || pathKey(this.#pane.path) !== key) return false;
    const path = this.#pane.path;
    this.#pane = { kind: 'empty' };
    this.show(path);
    return true;
  }

  #external(session: NoteSession, version?: FileVersion): void {
    const known = version === RESCANNED ? undefined : version;
    session.external(known).then(
      (outcome) => {
        if (outcome.kind !== 'reopen') return;
        // Файл стал только для чтения или перестал им быть: ввода нет
        // (иначе была бы запись), сессия открывается заново.
        const key = pathKey(session.path);
        if (this.#unsaved.get(key) === session) this.#unsaved.delete(key);
        const current = this.#currentSession() === session;
        session.dispose();
        if (!current) return;
        const fresh: NoteSession = new NoteSession(session.path, outcome.opened, this.#store, this.#hooks(() => fresh));
        this.#setPane({ kind: 'open', session: fresh });
      },
      () => undefined,
    );
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
          const message = userMessage(status.kind === 'failed' ? status.error : notFound());
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

  /**
   * Закрытие окна (задача 0.7): записать ввод всех заметок окна и дождаться
   * ответов всех записей — и тех, что ушли раньше, и повторов после
   * переименования (`missing` → `renamed()`), и конфликтов (обе версии
   * сохраняет ядро). Возвращает заметки, ввод которых записать не удалось;
   * их текст остаётся в памяти. Удалённые заметки (`removed()`) не пишутся.
   */
  async flushForClose(): Promise<UnsavedNote[]> {
    const started = performance.now();
    const deadline = started + CLOSE_FLUSH_DEADLINE_MS;
    const missingUntil = started + MISSING_GRACE_MS;
    // Одна попытка записи каждой заметки; дальше — только ожидание: запись
    // заметки без файла повторяется лишь после `renamed()` (`moveTo`).
    let inTime = await settlesBy(this.flushAll(), deadline);
    while (inTime) {
      const sessions = this.#sessions();
      const writing = sessions.filter((session) => session.writing);
      if (writing.length > 0) {
        inTime = await settlesBy(Promise.all(writing.map((session) => session.idle())), deadline);
        continue;
      }
      const missing = sessions.some((session) => session.status.kind === 'missing' && session.hasUnsavedChanges);
      if (!missing || performance.now() >= missingUntil) break;
      // Файла нет: ждём сигнала о переименовании (запись по новому пути).
      await sleep(MISSING_POLL_MS);
    }
    return this.#sessions()
      .filter((session) => session.hasUnsavedChanges)
      .map((session) => {
        const status = session.status;
        if (session.writing) return { path: session.path, error: new SaveTooSlow() };
        return { path: session.path, error: status.kind === 'failed' ? status.error : notFound() };
      });
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
