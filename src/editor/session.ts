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
  #inFlight: Promise<SaveOutcome> | null = null;
  #failing = false;
  #disposed = false;

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
      this.#cancelTimer();
      this.#failing = false;
      this.#setStatus({ kind: 'saved' });
      return;
    }
    if (this.#status.kind !== 'failed') this.#setStatus({ kind: 'pending' });
    this.#schedule(AUTOSAVE_DELAY_MS);
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
