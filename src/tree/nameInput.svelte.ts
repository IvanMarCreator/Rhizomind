// Поле нового имени (С3.2): проверка при вводе командой `name_check` (правило
// одно — в ядре), пояснение или предупреждение под полем из словаря,
// сохранение только допустимого имени. Общее для строки дерева и заголовка
// заметки (Э4, Э4б).
import { core } from '../api/index.ts';
import type { EntryKind, NameCheck, VaultPath } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { hasMessage, t, type MessageKey } from '../i18n/index.ts';
import { joinName } from './model.ts';

export interface NameInputOptions {
  /** Что переименовывается: от этого зависит текст предупреждения `unlinkable`. */
  kind: EntryKind;
  /** Папка, в которой лежит элемент. */
  dir: VaultPath;
  /** Правится имя без расширения; расширение (`.md`, `.png`) добавляется само. */
  stem: string;
  ext: string;
  /** Путь переименовываемого элемента. */
  renaming: VaultPath;
  /** Сохранение полного имени; `null` — успех, иначе текст отказа. */
  submit: (fullName: string) => Promise<string | null>;
}

/** `unlinkable` (С3.2) сказано про заметку; для папки и файла — свои тексты. */
const UNLINKABLE: Record<EntryKind, MessageKey> = {
  note: 'names.warning.unlinkable',
  folder: 'names.warning.unlinkable_folder',
  attachment: 'names.warning.unlinkable_attachment',
};

export type CommitResult = 'saved' | 'unchanged' | 'rejected';

export class NameInput {
  value = $state('');
  check = $state.raw<NameCheck | null>(null);
  /** Отказ ядра при сохранении или ошибка проверки. */
  failure = $state<string | null>(null);
  busy = $state(false);

  readonly #options: NameInputOptions;
  #seq = 0;

  constructor(options: NameInputOptions) {
    this.#options = options;
    this.value = options.stem;
  }

  get fullName(): string {
    return joinName(this.value, this.#options.ext);
  }

  get changed(): boolean {
    return this.value !== this.#options.stem;
  }

  /** Пояснение об ошибке под полем. */
  get error(): string | null {
    if (this.failure) return this.failure;
    if (this.check?.status !== 'rejected') return null;
    const key = `names.${this.check.problem}`;
    return hasMessage(key) ? t(key) : t('errors.invalid_name');
  }

  /** Предупреждение о допустимом имени (`unlinkable`, `hidden`). */
  get warning(): string | null {
    if (this.error || this.check?.status !== 'ok' || !this.check.warning) return null;
    if (this.check.warning === 'unlinkable') return t(UNLINKABLE[this.#options.kind]);
    const key = `names.warning.${this.check.warning}`;
    return hasMessage(key) ? t(key) : null;
  }

  /** Ввод: новое значение и проверка; ответ на устаревший ввод отбрасывается. */
  input(value: string): Promise<NameCheck | null> {
    this.value = value;
    this.failure = null;
    if (!this.changed) {
      this.#seq += 1;
      this.check = null;
      return Promise.resolve(null);
    }
    return this.#check();
  }

  async #check(): Promise<NameCheck | null> {
    const seq = ++this.#seq;
    const { dir, renaming } = this.#options;
    try {
      const result = await core.nameCheck({ dir, name: this.fullName, renaming });
      if (seq === this.#seq) this.check = result;
      return seq === this.#seq ? result : null;
    } catch (error) {
      if (seq === this.#seq) this.failure = userMessage(error);
      return null;
    }
  }

  /** Enter или уход фокуса: проверка свежего значения, затем сохранение. */
  async commit(): Promise<CommitResult> {
    if (!this.changed) return 'unchanged';
    if (this.busy) return 'rejected';
    this.busy = true;
    try {
      // Сохраняется ровно то имя, которое проверило ядро.
      const name = this.fullName;
      const result = await this.#check();
      if (result?.status !== 'ok') return 'rejected';
      this.failure = await this.#options.submit(name);
      return this.failure ? 'rejected' : 'saved';
    } finally {
      this.busy = false;
    }
  }
}
