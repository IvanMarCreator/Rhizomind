// Обёртки над командами ядра API v1: по функции на команду (С5.5).
// Ошибка любой команды — CoreCallError (core.ts).
import { callCore } from './core.ts';
import type { CommandName, CommandRequest, CommandResponse } from './types.ts';

/** Команды с параметрами и без. */
type WithRequest = { [K in CommandName]: CommandRequest<K> extends null ? never : K }[CommandName];
type WithoutRequest = Exclude<CommandName, WithRequest>;

const withRequest =
  <K extends WithRequest>(command: K) =>
  (request: CommandRequest<K>): Promise<CommandResponse<K>> =>
    // Тип K не сужается до «команды с параметрами» внутри обобщённой функции.
    (callCore as (c: K, r: CommandRequest<K>) => Promise<CommandResponse<K>>)(command, request);

const withoutRequest =
  <K extends WithoutRequest>(command: K) =>
  (): Promise<CommandResponse<K>> =>
    (callCore as (c: K) => Promise<CommandResponse<K>>)(command);

/** Команды ядра. */
export const core = {
  // Хранилища
  vaultList: withoutRequest('vault_list'),
  /** Версия API и хранилище окна; при запуске — через checkApiVersion(). */
  vaultCurrent: withoutRequest('vault_current'),
  /** Родительскую папку пользователь выбирает в диалоге ядра. */
  vaultCreate: withRequest('vault_create'),
  /** Папку пользователь выбирает в диалоге ядра. */
  vaultOpenFolder: withoutRequest('vault_open_folder'),
  vaultOpen: withRequest('vault_open'),
  vaultRemoveFromList: withRequest('vault_remove_from_list'),
  /** Окно «Управление хранилищами»: новое или фокус на открытое. */
  vaultManagerOpen: withoutRequest('vault_manager_open'),
  // Файлы
  treeList: withoutRequest('tree_list'),
  noteCreate: withRequest('note_create'),
  folderCreate: withRequest('folder_create'),
  entryRename: withRequest('entry_rename'),
  entryMove: withRequest('entry_move'),
  entryTrash: withRequest('entry_trash'),
  nameCheck: withRequest('name_check'),
  // Заметка
  noteRead: withRequest('note_read'),
  noteWrite: withRequest('note_write'),
  // Ссылки и поиск
  linksResolve: withRequest('links_resolve'),
  linksSuggest: withRequest('links_suggest'),
  switcherSearch: withRequest('switcher_search'),
  // Настройки и раскладка
  settingsRead: withoutRequest('settings_read'),
  settingsWrite: withRequest('settings_write'),
  workspaceRead: withoutRequest('workspace_read'),
  workspaceWrite: withRequest('workspace_write'),
  // Внешние ссылки
  /** Подтверждение показывает ядро; интерфейс обойти его не может. */
  openExternal: withRequest('open_external'),
} as const;
