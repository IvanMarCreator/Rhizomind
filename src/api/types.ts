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

/** Обновлять ли ссылки при переименовании или перемещении (0.2); нет поля — `keep`, как в 0.1. */
export type LinksMode = 'keep' | 'update';

export interface EntryRenameRequest {
  path: VaultPath;
  new_name: string;
  /** Нет поля — `keep`. При `update` обязателен `token` из `relink_preview`. */
  links?: LinksMode;
  /** Знак плана из `relink_preview`: только вместе с `links: 'update'`. */
  token?: RelinkToken;
}

export interface EntryMoveRequest {
  path: VaultPath;
  new_dir: VaultPath;
  /** Как у `entry_rename`. */
  links?: LinksMode;
  token?: RelinkToken;
}

/** Ответ `entry_rename` и `entry_move`. */
export interface EntryMovedResponse {
  path: VaultPath;
  /** Итог обновления ссылок — только при `links: 'update'`. */
  relink?: RelinkOutcome;
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

/**
 * Назначения горячих клавиш поверх сочетаний по умолчанию (решение 72):
 * команда → сочетания (`Ctrl+KeyO`; правила — src-tauri/src/commands/hotkeys.rs,
 * реестр команд — src/settings/hotkeys.ts). Пустой список — у команды нет
 * сочетаний; команды нет — сочетания по умолчанию.
 */
export type HotkeyOverrides = { [command: string]: string[] };

/**
 * «При переименовании заметки, на которую есть ссылки» (0.2, С2-3.5):
 * «Всегда спрашивать» или «Всегда обновлять ссылки»; «Никогда» нет (решение 116).
 */
export type RenameLinks = 'ask' | 'update';

/** Настройки хранилища (версию формата файла ставит ядро). */
export interface Settings {
  theme: Theme;
  editor_mode: EditorMode;
  text_width: TextWidth;
  font_size: number;
  new_note_location: NewNoteLocation;
  confirm_delete: boolean;
  max_edit_size_bytes: number;
  hotkeys: HotkeyOverrides;
  /** С 0.2; файл 0.1 без поля ядро читает как `ask`. */
  rename_links: RenameLinks;
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

// ---- Окно -------------------------------------------------------------------

/** Заметка, показанная в окне (`null` — не открыта): по ней ядро ставит заголовок окна (задача 2.12). */
export interface WindowShownNoteRequest {
  path: VaultPath | null;
}

// ---- Буфер обмена ------------------------------------------------------------

/**
 * Текст системного буфера обмена для пункта «Вставить» своего меню (задача
 * 2.19, решение 98): только окно хранилища в фокусе; длиннее предела размера
 * редактируемого файла — `too_large`.
 */
export type ClipboardReadOutcome =
  | { outcome: 'text'; text: string }
  | { outcome: 'empty' }
  | { outcome: 'too_large'; limit_bytes: number }
  | { outcome: 'not_focused' };

// ---- Карта команд и событий -----------------------------------------------

/**
 * Все команды API v1: запрос (`null` — без параметров) и ответ (`null` —
 * пустой ответ). Ключи — имена команд ядра (src-tauri/src/commands/list.rs).
 */
// ---- Индекс и обновление ссылок (0.2 «Связи», С2-5.5) ---------------------

/**
 * Знак плана переписывания ссылок: 32 шестнадцатеричные цифры. Выдаёт только
 * ядро; интерфейс лишь возвращает его. Одноразовый, привязан к окну и плану,
 * живёт 10 минут; неизвестный или просроченный — `invalid_argument` с полем
 * `token`: заново спросить `relink_preview`.
 */
export type RelinkToken = string;

export type IndexState = 'building' | 'ready';

/** Ответ `index_status` и данные `index:status`. `total: 0` — ещё неизвестно. */
export interface IndexStatus {
  state: IndexState;
  done: number;
  total: number;
}

/** Ход сборки в ответе команды, которой нужен готовый индекс. */
export interface IndexProgress {
  done: number;
  total: number;
}

/**
 * Ответ команды, которой нужен готовый индекс: данные или «индекс строится»
 * (окно показывает «Индекс строится… N %» и ждёт `index:status` с `ready`).
 */
export type IndexReply<T> = ({ status: 'ready' } & T) | ({ status: 'index_building' } & IndexProgress);

export interface BacklinksListRequest {
  path: VaultPath;
}

/** Строка с ссылкой — показывается как текст, не как HTML. */
export interface BacklinkLine {
  /** Номер строки, с 1. */
  line: number;
  /** Начало ссылки в строке, единиц UTF-16 — куда поставить курсор. */
  column_utf16: number;
  /** Строка, обрезанная вокруг ссылки до `LIMITS.max_snippet_chars` символов. */
  snippet: string;
  /** Выделение ссылки в `snippet`, единиц UTF-16: [from, to). */
  from_utf16: number;
  to_utf16: number;
}

export interface BacklinkNote {
  from: VaultPath;
  /** Имя без `.md`. */
  name: string;
  /** Всего ссылок в этой заметке (строк — не больше `LIMITS.max_backlink_lines`). */
  links_total: number;
  links: BacklinkLine[];
}

export interface Backlinks {
  /** Всего ссылающихся заметок (в `notes` — не больше `LIMITS.max_backlink_notes`). */
  notes_total: number;
  notes: BacklinkNote[];
}

export interface TagSummary {
  /** Написание для показа, без `#` (самое частое, решение 113). */
  tag: string;
  /** Ключ тега — для `tag_notes`. */
  key: string;
  count: number;
}

export interface Tags {
  tags: TagSummary[];
}

export interface TagNotesRequest {
  key: string;
}

export interface TagNotes {
  /** Всего заметок с тегом (в `notes` — не больше `LIMITS.max_tag_notes`). */
  notes_total: number;
  notes: VaultPath[];
}

/** Переименование или перемещение, которое собирается сделать пользователь. */
export type RelinkPreviewRequest =
  | { op: 'rename'; path: VaultPath; new_name: string }
  | { op: 'move'; path: VaultPath; new_dir: VaultPath };

/** `notes: 0` — переписывать нечего: переименовать сразу, без окна, с `links: 'keep'`. */
export interface RelinkPreview {
  notes: number;
  links: number;
  token: RelinkToken;
}

export interface RelinkAnswerRequest {
  /** Знак из `links:pending` или `retry_token` («Повторить»). */
  token: RelinkToken;
  /** `true` — обновить / уточнить / повторить; `false` — не обновлять / не трогать. */
  apply: boolean;
}

export interface LinksFlushDoneRequest {
  /** Знак из `links:flush-requested`. */
  token: RelinkToken;
  /** `false` — записать ввод не удалось: ссылки не переписываются. */
  saved: boolean;
}

/** Почему заметка не переписана; «Повторить» — только `busy` и `io` (решение 115). */
export type RelinkFailureReason = 'too_large' | 'not_utf8' | 'busy' | 'io';

export interface RelinkFailure {
  path: VaultPath;
  reason: RelinkFailureReason;
}

export interface RelinkOutcome {
  updated: number;
  failed: RelinkFailure[];
  /** Знак для «Повторить»; `null` — повторять нечего. */
  retry_token: RelinkToken | null;
}

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
  entry_rename: { request: EntryRenameRequest; response: EntryMovedResponse };
  entry_move: { request: EntryMoveRequest; response: EntryMovedResponse };
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
  /** Ответ на `window:close-requested`: ввод записан, окно закрывается (задача 0.7). */
  window_close_ready: { request: null; response: null };
  /** Ответ на `window:close-requested`: запись не удалась, окно показывается снова. */
  window_close_cancel: { request: null; response: null };
  /** Показанная заметка: заголовок окна «<заметка> — <хранилище> — Rhizomind» ставит ядро (задача 2.12). */
  window_shown_note: { request: WindowShownNoteRequest; response: null };
  /** Текст буфера обмена — только по щелчку пользователя по «Вставить» (задача 2.19). */
  clipboard_read_text: { request: null; response: ClipboardReadOutcome };
  // Индекс хранилища (0.2)
  index_status: { request: null; response: IndexStatus };
  backlinks_list: { request: BacklinksListRequest; response: IndexReply<Backlinks> };
  tags_list: { request: null; response: IndexReply<Tags> };
  tag_notes: { request: TagNotesRequest; response: IndexReply<TagNotes> };
  // Обновление ссылок (0.2)
  /** Пока индекс строится — `index_building`: окно «Подождите, индекс строится…» (решение 114). */
  relink_preview: { request: RelinkPreviewRequest; response: IndexReply<RelinkPreview> };
  /** Итог приходит событием `links:updated`. */
  relink_answer: { request: RelinkAnswerRequest; response: null };
  links_flush_done: { request: LinksFlushDoneRequest; response: null };
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
 * `window:close-requested`: пользователь закрыл окно хранилища (задача 0.7)
 * или Windows завершает сеанс (задача 0.11). Интерфейс записывает ввод и
 * отвечает `window_close_ready` или `window_close_cancel`.
 */
export interface WindowCloseRequestedPayload {
  /**
   * `true` — выход из системы или выключение Windows (задача 0.11а, решение
   * 79): окно не скрыто и не закрывается, ядро только ждёт записи; правило
   * «повторное закрытие в течение минуты» не действует.
   */
  sessionEnd: boolean;
}

/**
 * Все события ядра → интерфейс и их данные. Ядро отправляет их только окну
 * своего хранилища; `note:*` — для любой заметки хранилища, интерфейс
 * фильтрует открытые сам.
 */
/** `index:changed`: обратные ссылки или теги изменились — перезапросить видимый вид. */
export type IndexChangedPayload = Record<string, never>;

/** `links:flush-requested`: записать ввод всех заметок окна и ответить `links_flush_done`. */
export interface LinksFlushRequestedPayload {
  token: RelinkToken;
}

export interface PathRename {
  from: VaultPath;
  to: VaultPath;
}

export type PendingReason = 'external_rename' | 'duplicate';

/** `links:pending`: уведомление с кнопками, висит до ответа `relink_answer`. */
export interface LinksPendingPayload {
  token: RelinkToken;
  reason: PendingReason;
  notes: number;
  links: number;
  /** Не больше `LIMITS.max_pending_renames`; у `duplicate` — пусто. */
  renames: PathRename[];
  renames_total: number;
}

export type LinksUpdateReason = 'rename' | 'external_rename' | 'duplicate' | 'resumed' | 'retry';

/** `links:updated`: «Ссылки обновлены в N заметках»; `failed` — «Показать», `retry_token` — «Повторить». */
export interface LinksUpdatedPayload {
  reason: LinksUpdateReason;
  updated: number;
  failed: RelinkFailure[];
  retry_token: RelinkToken | null;
}

export interface CoreEvents {
  'vault:changed': VaultChangedPayload;
  'note:changed': NoteChangedPayload;
  'note:renamed': NoteRenamedPayload;
  'note:deleted': NoteDeletedPayload;
  'note:conflict-saved': NoteConflictSavedPayload;
  'vault:availability': VaultAvailabilityPayload;
  'window:close-requested': WindowCloseRequestedPayload;
  'index:status': IndexStatus;
  'index:changed': IndexChangedPayload;
  'links:flush-requested': LinksFlushRequestedPayload;
  'links:pending': LinksPendingPayload;
  'links:updated': LinksUpdatedPayload;
}

export type EventName = keyof CoreEvents;
