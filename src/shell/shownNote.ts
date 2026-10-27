// Какая заметка показана в окне — сообщается ядру (задача 2.12, решение
// владельца 82): по ней ядро ставит заголовок окна «<заметка> — <хранилище> —
// Rhizomind». Заголовок интерфейс не пишет и написать не может (права
// `core:window:allow-set-title` у окна нет): он сообщает только путь, ядро
// проверяет, что это заметка хранилища окна, и берёт имя из пути само.
import { core } from '../api/index.ts';
import type { VaultPath } from '../api/index.ts';

type Send = (path: VaultPath | null) => Promise<unknown>;

const keyOf = (path: VaultPath | null): string => JSON.stringify(path);

export class ShownNote {
  readonly #send: Send;
  /** Последнее, что ушло ядру; сначала «заметки нет» — заголовок окна ядро ставит таким при создании. */
  #sent = keyOf(null);
  #wanted: VaultPath | null = null;
  #sending = false;

  constructor(send: Send = (path) => core.windowShownNote({ path })) {
    this.#send = send;
  }

  /**
   * Показана заметка `path` (`null` — заметки нет). Тот же путь повторно не
   * шлётся (ввод текста, повторное открытие). Вызовы идут по одному: пока
   * ядро отвечает, копится только последний путь — заголовки не обгоняют
   * друг друга.
   */
  report(path: VaultPath | null): void {
    this.#wanted = path;
    if (!this.#sending) void this.#flush();
  }

  async #flush(): Promise<void> {
    this.#sending = true;
    try {
      while (keyOf(this.#wanted) !== this.#sent) {
        const path = this.#wanted;
        try {
          await this.#send(path);
        } catch {
          // Ядро отказало (заметку только что удалили или переименовали) —
          // заголовок без заметки, а не прежнее имя. Сбой и здесь — заголовок
          // не главное, окно работает дальше.
          if (path !== null) await this.#send(null).catch(() => undefined);
        }
        this.#sent = keyOf(path);
      }
    } finally {
      this.#sending = false;
    }
  }
}
