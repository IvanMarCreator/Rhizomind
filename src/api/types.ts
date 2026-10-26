// Типы API ядра v1 (спецификация 0.1, С5.5) — вручную, по типам Rust в
// src-tauri/src/commands/. Совпадение проверяется снимками contracts/v1/*.snap:
// их пишет `cargo test` (insta), а contract.test.ts сравнивает с примерами,
// записанными в этих типах. Имена полей — как в JSON ядра (snake_case).

/** Версия API, которую ожидает этот интерфейс. */
export const API_VERSION = 1;

/** Путь в хранилище по компонентам: `["Проекты", "Встреча.md"]`; корень — `[]`. */
export type VaultPath = string[];

/**
 * Версия файла: хэш blake3, размер, дата изменения. Для интерфейса —
 * непрозрачный знак: хранится из `note_read` и возвращается в `note_write`.
 */
export interface FileVersion {
  hash: string;
  size: number;
  modified_ms: number;
}

/** Идентификатор хранилища в списке хранилищ. */
export type VaultId = string;

export interface VaultSummary {
  id: VaultId;
  name: string;
  /** Полный путь к папке — только для показа. */
  path: string;
}

/** Папка, заметка или вложение (не-Markdown файл). */
export type EntryKind = 'folder' | 'note' | 'attachment';

// ---- Ошибки ---------------------------------------------------------------

export type ErrorCode =
  | 'not_implemented'
  | 'invalid_argument'
  | 'no_vault_open'
  | 'vault_not_found'
  | 'vault_unavailable'
  | 'invalid_path'
  | 'outside_vault'
  | 'not_found'
  | 'already_exists'
  | 'invalid_name'
  | 'not_a_note'
  | 'read_only'
  | 'scheme_not_allowed'
  | 'io_failed'
  | 'nested_vault';

export type NameProblem =
  | 'empty'
  | 'forbidden_chars'
  | 'reserved_name'
  | 'trailing_dot_or_space'
  | 'too_long'
  | 'duplicate'
  | 'note_extension';

/** Ошибка команды ядра. Текст — из словаря по `message_key` (`errors.<code>`). */
export interface CoreError {
  code: ErrorCode;
  message_key: string;
  /** Для `invalid_argument`: имя параметра. */
  field?: string;
  /** Для `invalid_name`: причина. */
  name_problem?: NameProblem;
}

// ---- Хранилища ------------------------------------------------------------

export interface VaultListItem extends VaultSummary {
  is_open: boolean;
  /** Папка на месте; `false` — диск отключён или папку удалили (задача 2.8б). */
  available: boolean;
}

export interface VaultListResponse {
  vaults: VaultListItem[];
  last_opened: VaultId | null;
}

/** Хранилище окна без пути к папке. */
export interface CurrentVault {
  id: VaultId;
  name: string;
}

export interface VaultCurrentResponse {
  api_version: number;
  /** `null` — окно без хранилища (управление хранилищами). */
  vault: CurrentVault | null;
}

export interface VaultCreateRequest {
  name: string;
  /** Родительская папка — ровно путь из последнего `folder_pick` этого окна. */
  parent: string;
}

/** Ответ `folder_pick`: родительская папка нового хранилища (задача 2.8б). */
export type FolderPickOutcome = { outcome: 'picked'; path: string } | { outcome: 'cancelled' };

export type VaultChooseOutcome = { outcome: 'opened'; vault: VaultSummary } | { outcome: 'cancelled' };

export interface VaultIdRequest {
  id: VaultId;
}

export type VaultOpenOutcome = { outcome: 'opened' } | { outcome: 'focused' };

// ---- Файлы ----------------------------------------------------------------

export interface TreeEntry {
  path: VaultPath;
  kind: EntryKind;
}

export interface TreeListResponse {
  /** Плоский список без порядка; сортирует интерфейс (`Intl.Collator`). */
  entries: TreeEntry[];
}

/** Имя в папке. Имя — полное, как на диске (заметка — с `.md`). */
export interface NameInDirRequest {
  dir: VaultPath;
  name: string;
}

export interface NameCheckRequest {
  dir: VaultPath;
  name: string;
  /**
   * При переименовании — путь самого элемента: не совпадение с собой, смена
   * только регистра разрешена. Можно не передавать; `null` — новый элемент.
   */
  renaming?: VaultPath | null;
}

export interface EntryPathResponse {
  path: VaultPath;
}

export interface EntryRenameRequest {
  path: VaultPath;
  new_name: string;
}

export interface EntryMoveRequest {
  path: VaultPath;
  new_dir: VaultPath;
}

export interface EntryPathRequest {
  path: VaultPath;
}

export type TrashOutcome =
  | { outcome: 'trashed' }
  | { outcome: 'deleted_permanently' }
  | { outcome: 'cancelled' };

/**
 * Предупреждение о допустимом имени; текст — `names.warning.<предупреждение>`.
 * `unlinkable` — на заметку нельзя сослаться: `#`, `[`, `]` или пробел в начале;
 * `hidden` — имя с точкой в начале, элемент будет скрыт в дереве (С3.2).
 */
export type NameWarning = 'unlinkable' | 'hidden';

export type NameCheck =
  | { status: 'ok'; warning: NameWarning | null }
  | { status: 'rejected'; problem: NameProblem; message_key: string };

// ---- Заметка --------------------------------------------------------------
// Правило «байт в байт»: интерфейс работает с текстом с переносами `\n` и без
// BOM. BOM и переносы файла знает только ядро; если текст не изменился, ядро
// файл не пишет. `note_write` по удалённому пути — `not_found`. По
// `note:deleted` интерфейс закрывает заметку с уведомлением и отбрасывает
// несохранённый ввод (решение владельца, С3.6).

export type ReadOnlyReason = { reason: 'too_large'; limit_bytes: number } | { reason: 'invalid_utf8' };

export interface NoteReadRequest {
  path: VaultPath;
}

export interface NoteReadResponse {
  /** Текст без BOM, переносы строк — `\n`. */
  text: string;
  version: FileVersion;
  read_only: ReadOnlyReason | null;
}

export interface NoteWriteRequest {
  path: VaultPath;
  /** Текст с переносами `\n`, без BOM. */
  text: string;
  /** После `note:renamed` — по новому пути с прежней ожидаемой версией. */
  expected_version: FileVersion;
}

export type NoteWriteResponse =
  | { outcome: 'saved'; version: FileVersion }
  | { outcome: 'conflict_saved'; version: FileVersion; copy_path: VaultPath };

// ---- Ссылки и поиск -------------------------------------------------------

export interface LinksResolveRequest {
  from: VaultPath;
  targets: string[];
}

export interface ResolvedLink {
  target: string;
  /** `null` — неразрешённая ссылка. */
  path: VaultPath | null;
}

export interface LinksResolveResponse {
  results: ResolvedLink[];
}

export interface LinksSuggestRequest {
  from: VaultPath;
  query: string;
}

export interface LinkSuggestion {
  /** Что вставить между `[[` и `]]`; `null`, если на заметку нельзя сослаться. */
  insert: string | null;
  name: string;
  path: VaultPath;
  /** `false` — во вставке были бы `#`, `[`, `]` или пробел в начале: заметка показывается помеченной (С3.2). */
  linkable: boolean;
}

export interface LinksSuggestResponse {
  items: LinkSuggestion[];
}

export interface SwitcherSearchRequest {
  query: string;
}

export interface SwitcherItem {
  name: string;
  path: VaultPath;
}

export interface SwitcherSearchResponse {
  items: SwitcherItem[];
}

// ---- Настройки и раскладка ------------------------------------------------

export type Theme = 'system' | 'light' | 'dark';
export type EditorMode = 'live_preview' | 'source';
export type TextWidth = 'column' | 'full';
export type NewNoteLocation =
  | { kind: 'vault_root' }
  | { kind: 'current_folder' }
  | { kind: 'folder'; path: VaultPath };

/** Настройки хранилища (версию формата файла ставит ядро). */
export interface Settings {
  theme: Theme;
  editor_mode: EditorMode;
  text_width: TextWidth;
  font_size: number;
  new_note_location: NewNoteLocation;
  confirm_delete: boolean;
  max_edit_size_bytes: number;
}

export interface SettingsReadResponse {
  settings: Settings;
}

export interface SettingsWriteRequest {
  settings: Settings;
}

/** Раскладка окна: модель раскладки v1 принадлежит интерфейсу; ядро хранит объект как есть. */
export type Workspace = { [key: string]: unknown };

export interface WorkspaceReadResponse {
  workspace: Workspace | null;
}

export interface WorkspaceWriteRequest {
  workspace: Workspace;
}

// ---- Внешние ссылки -------------------------------------------------------

export interface OpenExternalRequest {
  url: string;
}

export type OpenExternalOutcome = { outcome: 'opened' } | { outcome: 'cancelled' };

// ---- Карта команд и событий -----------------------------------------------

/**
 * Все команды API v1: запрос (`null` — без параметров) и ответ (`null` —
 * пустой ответ). Ключи — имена команд ядра (src-tauri/src/commands/list.rs).
 */
export interface CoreCommands {
  vault_list: { request: null; response: VaultListResponse };
  vault_current: { request: null; response: VaultCurrentResponse };
  vault_create: { request: VaultCreateRequest; response: VaultChooseOutcome };
  vault_open_folder: { request: null; response: VaultChooseOutcome };
  vault_open: { request: VaultIdRequest; response: VaultOpenOutcome };
  vault_remove_from_list: { request: VaultIdRequest; response: null };
  /** Окно «Управление хранилищами» (задача 2.8а). */
  vault_manager_open: { request: null; response: null };
  /** Выбор родительской папки в системном окне ядра; только окно управления (задача 2.8б). */
  folder_pick: { request: null; response: FolderPickOutcome };
  tree_list: { request: null; response: TreeListResponse };
  note_create: { request: NameInDirRequest; response: EntryPathResponse };
  folder_create: { request: NameInDirRequest; response: EntryPathResponse };
  entry_rename: { request: EntryRenameRequest; response: EntryPathResponse };
  entry_move: { request: EntryMoveRequest; response: EntryPathResponse };
  entry_trash: { request: EntryPathRequest; response: TrashOutcome };
  name_check: { request: NameCheckRequest; response: NameCheck };
  note_read: { request: NoteReadRequest; response: NoteReadResponse };
  note_write: { request: NoteWriteRequest; response: NoteWriteResponse };
  links_resolve: { request: LinksResolveRequest; response: LinksResolveResponse };
  links_suggest: { request: LinksSuggestRequest; response: LinksSuggestResponse };
  switcher_search: { request: SwitcherSearchRequest; response: SwitcherSearchResponse };
  settings_read: { request: null; response: SettingsReadResponse };
  settings_write: { request: SettingsWriteRequest; response: null };
  workspace_read: { request: null; response: WorkspaceReadResponse };
  workspace_write: { request: WorkspaceWriteRequest; response: null };
  open_external: { request: OpenExternalRequest; response: OpenExternalOutcome };
}

export type CommandName = keyof CoreCommands;
export type CommandRequest<K extends CommandName> = CoreCommands[K]['request'];
export type CommandResponse<K extends CommandName> = CoreCommands[K]['response'];

// ---- События --------------------------------------------------------------

export type TreeChange =
  | { kind: 'created'; path: VaultPath; entry_kind: EntryKind }
  | { kind: 'modified'; path: VaultPath }
  | { kind: 'removed'; path: VaultPath }
  | { kind: 'renamed'; from: VaultPath; to: VaultPath };

export interface VaultChangedPayload {
  /** `true` — была полная сверка с диском: перечитать дерево `tree_list`. */
  rescan: boolean;
  changes: TreeChange[];
}

export interface NoteChangedPayload {
  path: VaultPath;
  version: FileVersion;
}

export interface NoteRenamedPayload {
  from: VaultPath;
  to: VaultPath;
}

export interface NoteDeletedPayload {
  path: VaultPath;
}

export interface NoteConflictSavedPayload {
  path: VaultPath;
  copy_path: VaultPath;
}

/**
 * Папка хранилища недоступна (`false`: отключили флешку или сетевой диск,
 * папку переименовали снаружи) или снова доступна (`true`). Пока недоступна,
 * заметки не закрываются, запись отклоняется ошибкой ядра. После `true`
 * приходит `vault:changed` с `rescan: true`. Расширение контракта v1 (задача 1.7).
 */
export interface VaultAvailabilityPayload {
  available: boolean;
}

/**
 * Все события ядра → интерфейс и их данные. Ядро отправляет их только окну
 * своего хранилища; `note:*` — для любой заметки хранилища, интерфейс
 * фильтрует открытые сам.
 */
export interface CoreEvents {
  'vault:changed': VaultChangedPayload;
  'note:changed': NoteChangedPayload;
  'note:renamed': NoteRenamedPayload;
  'note:deleted': NoteDeletedPayload;
  'note:conflict-saved': NoteConflictSavedPayload;
  'vault:availability': VaultAvailabilityPayload;
}

export type EventName = keyof CoreEvents;
