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
  /** Путь, на который ядро только что отказало: не повторять его в цикле — только по новому `report`. */
  #refused: string | null = null;
  #sending = false;

  constructor(send: Send = (path) => core.windowShownNote({ path })) {
    this.#send = send;
  }

  /**
   * Показана заметка `path` (`null` — заметки нет). Тот же путь повторно не
   * шлётся (ввод текста, повторное открытие). Вызовы идут по одному: пока
   * ядро отвечает, копится только последний путь — заголовки не обгоняют
   * друг друга. Путь, на который ядро отказало, отправленным не считается:
   * следующий `report` с ним отправит его снова.
   */
  report(path: VaultPath | null): void {
    this.#refused = null;
    this.#wanted = path;
    if (!this.#sending) void this.#flush();
  }

  async #flush(): Promise<void> {
    this.#sending = true;
    try {
      for (;;) {
        const path = this.#wanted;
        const key = keyOf(path);
        if (key === this.#sent || key === this.#refused) break;
        try {
          await this.#send(path);
          this.#sent = key;
        } catch {
          // Ядро отказало (заметку только что удалили или переименовали) —
          // заголовок без заметки, а не прежнее имя. Сбой и здесь — заголовок
          // не главное, окно работает дальше.
          this.#refused = key;
          if (path !== null) {
            await this.#send(null).then(
              () => (this.#sent = keyOf(null)),
              () => undefined,
            );
          }
        }
      }
    } finally {
      this.#sending = false;
    }
  }
}
