// Закрытие окна хранилища (задача 0.7, решение владельца 53). Ядро уже
// скрыло окно и прислало `window:close-requested`; здесь — записать ввод
// всех заметок окна и ответить ядру:
// - всё записано — `window_close_ready`: окно закрывается;
// - запись не удалась — `window_close_cancel`: окно показывается снова с
//   уведомлением, текст остаётся в редакторе (инвариант 4). Повторное
//   закрытие в течение минуты закрывает окно без сохранения — иначе окно на
//   отключённой флешке нельзя было бы закрыть вовсе.
// Ядро ждёт ответа не дольше 10 с (CLOSE_TIMEOUT в
// src-tauri/src/windows/closing.rs), потом закрывает окно само.
import { core } from '../api/index.ts';
import type { UnsavedNote } from '../editor/sessions.ts';
import { noteName } from '../editor/sessions.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';

/** Сколько после отказа повторное закрытие закрывает окно без сохранения. */
export const REFUSAL_WINDOW_MS = 60_000;

export interface ClosingHost {
  /** Записать ввод; вернуть заметки, ввод которых записать не удалось. */
  flush(): Promise<readonly UnsavedNote[]>;
  /** Ответ ядру: окно можно закрыть. */
  ready(): Promise<unknown>;
  /** Ответ ядру: окно показать снова. */
  cancel(): Promise<unknown>;
  notify(text: string): void;
}

/** Ответы ядру — команды API v1. */
export const coreClosing = {
  ready: () => core.windowCloseReady(),
  cancel: () => core.windowCloseCancel(),
};

export class WindowClosing {
  readonly #host: ClosingHost;
  /** Идёт запись по сигналу: второй сигнал её не повторяет. */
  #busy: Promise<void> | null = null;
  /** Когда окно отказалось закрыться из-за сбоя записи. */
  #refusedAt: number | null = null;

  constructor(host: ClosingHost) {
    this.#host = host;
  }

  /** Сигнал ядра `window:close-requested`. */
  closeRequested(): Promise<void> {
    this.#busy ??= this.#close().finally(() => (this.#busy = null));
    return this.#busy;
  }

  async #close(): Promise<void> {
    let unsaved: readonly UnsavedNote[];
    try {
      unsaved = await this.#host.flush();
    } catch (error) {
      unsaved = [{ path: [], error }];
    }
    const refusedRecently = this.#refusedAt !== null && Date.now() - this.#refusedAt < REFUSAL_WINDOW_MS;
    if (unsaved.length === 0 || refusedRecently) {
      this.#refusedAt = null;
      await this.#host.ready().catch(() => undefined);
      return;
    }
    this.#refusedAt = Date.now();
    this.#host.notify(refusalText(unsaved));
    await this.#host.cancel().catch(() => undefined);
  }
}

function refusalText(unsaved: readonly UnsavedNote[]): string {
  const names = unsaved.map((note) => `«${noteName(note.path)}»`).join(', ');
  const message = userMessage(unsaved[0]?.error);
  return unsaved.length === 1
    ? t('window.closeRefused.one', { names, message })
    : t('window.closeRefused.many', { names, message });
}
