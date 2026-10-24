import type { Edition } from './edition';

/**
 * Публичный API ядра Rhizomind.
 *
 * Это единственный способ, которым интерфейс (а в будущем и плагины)
 * может обращаться к данным. Интерфейс не видит файловую систему, Node.js
 * и абсолютные пути: только заметки внутри открытого хранилища.
 *
 * Правила развития:
 * - API версионируется (API_VERSION). Ломающие изменения — только с повышением версии.
 * - Каждый метод — узкое действие над хранилищем. Никаких «выполни произвольный код»,
 *   «прочитай любой файл» или «сделай сетевой запрос».
 * - Плагины Community-редакции получат подмножество этого API, а не больше.
 */
export const API_VERSION = 1;

/** Путь заметки относительно корня хранилища, всегда через «/», например "Проекты/Идея.md". */
export type NotePath = string;

export interface NoteRef {
  readonly path: NotePath;
  /** Имя файла без расширения .md. */
  readonly title: string;
}

export interface VaultInfo {
  /** Имя папки хранилища (без полного пути). */
  readonly name: string;
  readonly noteCount: number;
}

export interface VaultApi {
  /** Показывает системный диалог выбора папки и открывает её как хранилище. */
  open(): Promise<VaultInfo | null>;
  current(): Promise<VaultInfo | null>;
  listNotes(): Promise<NoteRef[]>;
  readNote(path: NotePath): Promise<string>;
  writeNote(path: NotePath, content: string): Promise<void>;
  /** Создаёт заметку в корне хранилища. Если имя занято, добавляет номер. */
  createNote(title: string): Promise<NoteRef>;
}

export interface LinksApi {
  /** Находит заметку по цели вики-ссылки ([[цель]]). */
  resolve(target: string): Promise<NoteRef | null>;
  /** Заметки, которые ссылаются на указанную. */
  backlinks(path: NotePath): Promise<NoteRef[]>;
}

export interface RhizomindApi {
  readonly apiVersion: number;
  readonly edition: Edition;
  readonly vault: VaultApi;
  readonly links: LinksApi;
}
