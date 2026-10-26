// История переходов окна (С3.5): «назад» и «вперёд» между открытыми заметками.
// Шаг истории — путь в хранилище по компонентам. Модель без фреймворка:
// оболочка окна подписывается на изменения и рисует кнопки сама.
import type { VaultPath } from '../../api/index.ts';

/** Сколько шагов хранится по умолчанию; старые отбрасываются. */
export const HISTORY_LIMIT = 100;

function samePath(a: VaultPath, b: VaultPath): boolean {
  return a.length === b.length && a.every((part, i) => part === b[i]);
}

/** `path` — это `prefix` или лежит внутри него. */
function startsWith(path: VaultPath, prefix: VaultPath): boolean {
  return prefix.length <= path.length && prefix.every((part, i) => part === path[i]);
}

export class NavigationHistory {
  #entries: VaultPath[] = [];
  /** Индекс текущего шага; -1 — история пуста. */
  #index = -1;
  readonly #limit: number;

  constructor(limit = HISTORY_LIMIT) {
    this.#limit = Math.max(1, limit);
  }

  get current(): VaultPath | null {
    return this.#entries[this.#index] ?? null;
  }

  get canGoBack(): boolean {
    return this.#index > 0;
  }

  get canGoForward(): boolean {
    return this.#index < this.#entries.length - 1;
  }

  /** Переход к заметке: путь вперёд отбрасывается. Та же заметка — не шаг. */
  visit(path: VaultPath): void {
    const current = this.current;
    if (current && samePath(current, path)) return;
    this.#entries = this.#entries.slice(0, this.#index + 1);
    this.#entries.push([...path]);
    if (this.#entries.length > this.#limit) this.#entries.splice(0, this.#entries.length - this.#limit);
    this.#index = this.#entries.length - 1;
  }

  /** Шаг назад; `null`, если некуда. */
  back(): VaultPath | null {
    if (!this.canGoBack) return null;
    this.#index -= 1;
    return this.current;
  }

  /** Шаг вперёд; `null`, если некуда. */
  forward(): VaultPath | null {
    if (!this.canGoForward) return null;
    this.#index += 1;
    return this.current;
  }

  /** Заметка или папка переименована либо перемещена: пути внутри меняются. */
  rename(from: VaultPath, to: VaultPath): void {
    this.#entries = this.#entries.map((entry) =>
      startsWith(entry, from) ? [...to, ...entry.slice(from.length)] : entry,
    );
  }

  /**
   * Заметка или папка удалена: её шаги (и шаги заметок внутри папки) уходят из
   * истории; соседние одинаковые шаги склеиваются. Если удалена текущая —
   * текущей становится ближайшая предыдущая.
   */
  remove(path: VaultPath): void {
    const kept: VaultPath[] = [];
    let index = -1;
    this.#entries.forEach((entry, i) => {
      if (startsWith(entry, path)) return;
      const last = kept[kept.length - 1];
      if (!last || !samePath(last, entry)) kept.push(entry);
      if (i <= this.#index) index = kept.length - 1;
    });
    this.#entries = kept;
    this.#index = kept.length === 0 ? -1 : Math.max(index, 0);
  }
}

export type HistoryAction = 'back' | 'forward';
