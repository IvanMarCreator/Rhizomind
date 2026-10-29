// Сохранение одной открытой заметки (С3.3, С3.6).
//
// Правило «байт в байт» делится с ядром (src-tauri/src/commands/note.rs):
// ядро отдаёт текст без BOM с переносами `\n` и при записи само возвращает
// файлу BOM и переносы; если текст не изменился, файл не пишется. Здесь
// только два правила: текст из редактора уходит ядру ровно таким, каким он
// стал после правок пользователя, и без правок запись не отправляется вовсе.
//
// Ни один ввод не теряется: запись по таймеру, по требованию (переход к
// другой заметке, закрытие) и повтор после сбоя. `not_found` при записи —
// не удаление: файл могли переименовать раньше, чем окно об этом узнало;
// текст держится до `moveTo()` (запись по новому пути) или `discard()`.
// Ввод отбрасывается только по `discard()` — заметку удалили (решение
// владельца, инвариант 4).
import { CoreCallError, core } from '../api/index.ts';
import type {
  FileVersion,
  NoteReadResponse,
  NoteWriteRequest,
  NoteWriteResponse,
  ReadOnlyReason,
  VaultPath,
} from '../api/index.ts';

/** Пауза ввода перед автосохранением (С3.3). */
export const AUTOSAVE_DELAY_MS = 1000;
/**
 * Предельный срок автосохранения при непрерывном вводе (решение 91): от
 * первой несохранённой правки — не дольше этого, даже если пауз ≥ 1 с нет.
 */
export const AUTOSAVE_MAX_DELAY_MS = 5000;
/** Повтор записи после сбоя. */
export const RETRY_DELAY_MS = 5000;

/** Чтение и запись заметки; по умолчанию — команды ядра. */
export interface NoteStore {
  read(path: VaultPath): Promise<NoteReadResponse>;
  write(request: NoteWriteRequest): Promise<NoteWriteResponse>;
}

export const coreNoteStore: NoteStore = {
  read: (path) => core.noteRead({ path }),
  write: (request) => core.noteWrite(request),
};

/** Состояние сохранения для индикатора. */
export type SaveStatus =
  /** Текст совпадает с файлом. */
  | { kind: 'saved' }
  /** Есть несохранённый ввод или идёт запись. */
  | { kind: 'pending' }
  /** Запись не удалась; текст в редакторе, повтор по таймеру. */
  | { kind: 'failed'; error: unknown }
  | { kind: 'read_only'; reason: ReadOnlyReason }
  /**
   * Файла по этому пути нет (`not_found`): возможно, его переименовали, а окно
   * ещё не узнало. Текст держится; запись повторится после `moveTo()`.
   */
  | { kind: 'missing' }
  /** Заметку удалили (`discard()`): ввод отброшен, запись прекращена. */
  | { kind: 'deleted' };

export interface SessionHooks {
  onStatus?(status: SaveStatus): void;
  /** Ядро сохранило обе версии: версия с диска — копией `copyPath`. */
  onConflictSaved?(copyPath: VaultPath): void;
  /** Первый сбой записи в серии (следующие подряд не сообщаются). */
  onSaveFailed?(error: unknown): void;
}

/**
 * Итог внешнего изменения (`NoteSession.external`):
 * - `none` — ничего не изменилось (своя запись, тот же файл, сессия закрыта);
 * - `reloaded` — текст обновлён с диска, ввода не было;
 * - `flushed` — был несохранённый ввод: он записан сразу со старой
 *   ожидаемой версией, и ядро сохранило обе версии (конфликт, С3.6);
 * - `reopen` — файл стал только для чтения или перестал им быть: сессию
 *   надо открыть заново ответом `opened`.
 */
export type ExternalOutcome =
  | { kind: 'none' }
  | { kind: 'reloaded' }
  | { kind: 'flushed' }
  | { kind: 'reopen'; opened: NoteReadResponse };

/** Та же версия файла: хэш и размер, как сравнивает ядро (дата не важна). */
export function sameVersion(a: FileVersion, b: FileVersion): boolean {
  return a.hash === b.hash && a.size === b.size;
}

function isNotFound(error: unknown): boolean {
  return error instanceof CoreCallError && error.error.code === 'not_found';
}

/** Итог одной записи: записано, не удалось, повторить (путь сменился в полёте). */
type SaveOutcome = 'saved' | 'failed' | 'retry';

export class NoteSession {
  readonly readOnly: ReadOnlyReason | null;

  readonly #store: NoteStore;
  readonly #hooks: SessionHooks;
  #path: VaultPath;
  /** Текст, который сейчас в файле (по последнему ответу ядра). */
  #saved: string;
  /** Текст в редакторе. */
  #current: string;
  #version: FileVersion;
  #status: SaveStatus;
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** Когда появилась первая ещё не ушедшая в запись правка (`Date.now()`); `null` — таких нет. */
  #pendingSince: number | null = null;
  #inFlight: Promise<SaveOutcome> | null = null;
  #failing = false;
  #disposed = false;
  /** Подписчики на текст, пришедший с диска (редактор на экране). */
  readonly #externalText = new Set<(text: string) => void>();
  /** Внешние изменения обрабатываются по одному, в порядке прихода. */
  #externalQueue: Promise<unknown> = Promise.resolve();

  constructor(path: VaultPath, opened: NoteReadResponse, store: NoteStore, hooks: SessionHooks = {}) {
    this.#path = path;
    this.readOnly = opened.read_only;
    this.#store = store;
    this.#hooks = hooks;
    this.#saved = opened.text;
    this.#current = opened.text;
    this.#version = opened.version;
    this.#status = opened.read_only ? { kind: 'read_only', reason: opened.read_only } : { kind: 'saved' };
  }

  /** Путь заметки; после переименования — новый. */
  get path(): VaultPath {
    return this.#path;
  }

  /**
   * Заметку переименовали или переместили (в приложении или снаружи): дальше
   * пишем по новому пути с прежней ожидаемой версией (контракт `note_write`).
   * Это не уход с заметки — ничего не записывается сразу.
   */
  moveTo(path: VaultPath): void {
    this.#path = path;
    // Запись не нашла файл по старому пути — повторить по новому.
    if (this.#status.kind === 'missing' && !this.#disposed && !this.#inFlight) void this.#save();
  }

  /**
   * Заметку удалили: несохранённый ввод отбрасывается (решение владельца,
   * инвариант 4), записей больше не будет.
   */
  discard(): void {
    this.#cancelTimer();
    this.#disposed = true;
    this.#setStatus({ kind: 'deleted' });
  }

  /** Текст в редакторе (при открытии — текст файла). */
  get text(): string {
    return this.#current;
  }

  /** Размер файла в байтах по последней известной версии. */
  get size(): number {
    return this.#version.size;
  }

  get status(): SaveStatus {
    return this.#status;
  }

  /** Запись в полёте (ответ ядра ещё не пришёл). */
  get writing(): boolean {
    return this.#inFlight !== null;
  }

  /** Дождаться ответов записей в полёте, не начиная новых. */
  async idle(): Promise<void> {
    while (this.#inFlight) await this.#inFlight;
  }

  /** Есть ввод, которого ещё нет в файле, или идёт запись. */
  get hasUnsavedChanges(): boolean {
    if (this.#status.kind === 'deleted' || this.readOnly) return false;
    return this.#inFlight !== null || this.#current !== this.#saved;
  }

  /** Правка пользователя: новый текст редактора. Запись — через 1 с паузы. */
  edit(text: string): void {
    if (this.readOnly || this.#status.kind === 'deleted' || this.#disposed) return;
    this.#current = text;
    // Файла нет: пишем только после moveTo() или по требованию.
    if (this.#status.kind === 'missing') return;
    if (text === this.#saved && !this.#inFlight) {
      this.#pendingSince = null;
      this.#cancelTimer();
      this.#failing = false;
      this.#setStatus({ kind: 'saved' });
      return;
    }
    if (this.#status.kind !== 'failed') this.#setStatus({ kind: 'pending' });
    // Пауза 1 с, но не позже 5 с от первой несохранённой правки (решение 91).
    const now = Date.now();
    this.#pendingSince ??= now;
    this.#schedule(Math.max(0, Math.min(AUTOSAVE_DELAY_MS, this.#pendingSince + AUTOSAVE_MAX_DELAY_MS - now)));
  }

  /**
   * Записать сейчас (переход к другой заметке, закрытие). Если записи нет в
   * полёте, запрос уходит синхронно, до первого `await`. Завершается, когда
   * текст записан, запись не удалась или файл удалён.
   */
  async flush(): Promise<void> {
    this.#cancelTimer();
    for (;;) {
      if (this.#inFlight) {
        await this.#inFlight;
        continue;
      }
      if (!this.hasUnsavedChanges || this.#disposed) return;
      if ((await this.#save()) === 'failed') return;
    }
  }

  /** Снять таймеры; ввод после этого не пишется. Перед этим — `flush()`. */
  dispose(): void {
    this.#disposed = true;
    this.#cancelTimer();
    this.#externalText.clear();
  }

  /**
   * Подписка на текст, пришедший с диска после внешнего изменения: редактор
   * заменяет им документ. Возвращает отписку.
   */
  onExternalText(listener: (text: string) => void): () => void {
    this.#externalText.add(listener);
    return () => this.#externalText.delete(listener);
  }

  /**
   * Файл заметки изменился снаружи (событие `note:changed` с версией с
   * диска; без версии — после полной сверки, `vault:changed` с `rescan`).
   * Инвариант 4, С3.6:
   * - сначала дожидаемся записей в полёте: событие могло прийти о своей же
   *   записи, а версия — ответом на неё;
   * - версия совпала с известной — ничего;
   * - есть несохранённый ввод — он записывается сразу со старой ожидаемой
   *   версией: ядро видит конфликт и сохраняет обе версии (ваша — в
   *   заметке, с диска — копией рядом);
   * - ввода нет — текст и версия с диска; если, пока шло чтение, появился
   *   ввод, прочитанное не показывается, а ввод пишется (снова обе версии).
   * Вызовы выполняются по одному, в порядке прихода.
   */
  external(version?: FileVersion): Promise<ExternalOutcome> {
    const run = this.#externalQueue.then(() => this.#external(version));
    this.#externalQueue = run.catch(() => undefined);
    return run;
  }

  async #external(version: FileVersion | undefined): Promise<ExternalOutcome> {
    if (this.#gone()) return { kind: 'none' };
    await this.idle();
    if (this.#gone()) return { kind: 'none' };
    // Файла не было (флешку вынули, внешний редактор убрал файл), теперь он
    // снова есть: ввод пишется со старой версией — изменённый файл ядро
    // сохранит копией; если файла всё ещё нет — снова «нет файла».
    if (this.#status.kind === 'missing') return this.#flushBoth();
    if (version && sameVersion(version, this.#version)) return { kind: 'none' };
    if (this.hasUnsavedChanges) return this.#flushBoth();
    const before = this.#current;
    let opened: NoteReadResponse;
    try {
      opened = await this.#store.read(this.#path);
    } catch {
      // Файла уже нет или он недоступен: об удалении и переименовании
      // скажут свои события; текст в редакторе остаётся.
      return { kind: 'none' };
    }
    // Пока шло чтение, заметку могли удалить или закрыть.
    if (this.#gone()) return { kind: 'none' };
    // Ввод, пока шло чтение: он важнее прочитанного — пишем, ядро сохранит обе версии.
    if (this.#current !== before || this.hasUnsavedChanges) return this.#flushBoth();
    if (sameVersion(opened.version, this.#version)) return { kind: 'none' };
    if (JSON.stringify(opened.read_only) !== JSON.stringify(this.readOnly)) return { kind: 'reopen', opened };
    this.#version = opened.version;
    this.#saved = opened.text;
    this.#current = opened.text;
    for (const listener of [...this.#externalText]) listener(opened.text);
    return { kind: 'reloaded' };
  }

  /** Сессия закрыта или заметку удалили — внешнее изменение её не касается. */
  #gone(): boolean {
    return this.#disposed || this.#status.kind === 'deleted';
  }

  async #flushBoth(): Promise<ExternalOutcome> {
    await this.flush();
    return { kind: 'flushed' };
  }

  #schedule(ms: number): void {
    this.#cancelTimer();
    this.#timer = setTimeout(() => {
      this.#timer = null;
      // Запись в полёте: следующая уйдёт по её завершении.
      if (!this.#inFlight) void this.#save();
    }, ms);
  }

  #cancelTimer(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  /** Одна запись текущего текста. */
  #save(): Promise<SaveOutcome> {
    const text = this.#current;
    // Всё набранное до этой минуты уходит в запись: срок 5 с — заново.
    this.#pendingSince = null;
    const path = this.#path;
    if (this.#status.kind !== 'failed' && this.#status.kind !== 'missing') this.#setStatus({ kind: 'pending' });
    const flight = this.#store
      .write({ path, text, expected_version: this.#version })
      .then(
        (response) => {
          if (this.#disposed) return 'failed' as const;
          this.#version = response.version;
          this.#saved = text;
          this.#failing = false;
          if (response.outcome === 'conflict_saved') this.#hooks.onConflictSaved?.(response.copy_path);
          return 'saved' as const;
        },
        (error: unknown): SaveOutcome => {
          if (this.#disposed) return 'failed';
          // Запрос ушёл по старому пути, а заметку переименовали: повторить.
          if (isNotFound(error) && path !== this.#path) return 'retry';
          if (!this.#failing) this.#hooks.onSaveFailed?.(error);
          this.#failing = true;
          if (isNotFound(error)) {
            this.#cancelTimer();
            this.#setStatus({ kind: 'missing' });
            return 'failed';
          }
          this.#setStatus({ kind: 'failed', error });
          if (!this.#disposed) this.#schedule(RETRY_DELAY_MS);
          return 'failed';
        },
      )
      .then((outcome) => {
        this.#inFlight = null;
        if (this.#disposed) return outcome;
        if (outcome === 'saved') this.#afterSave();
        else if (outcome === 'retry' && !this.#disposed && this.#timer === null) void this.#save();
        return outcome;
      });
    this.#inFlight = flight;
    return flight;
  }

  #afterSave(): void {
    if (this.#current === this.#saved) {
      this.#setStatus({ kind: 'saved' });
      return;
    }
    // Ввод во время записи: если пауза уже прошла (таймер сработал в полёте),
    // пишем сразу; иначе ждём таймер.
    this.#setStatus({ kind: 'pending' });
    if (this.#timer === null && !this.#disposed) void this.#save();
  }

  #setStatus(status: SaveStatus): void {
    const same = status.kind === this.#status.kind && (status.kind === 'saved' || status.kind === 'pending');
    if (same) return;
    this.#status = status;
    this.#hooks.onStatus?.(status);
  }
}
