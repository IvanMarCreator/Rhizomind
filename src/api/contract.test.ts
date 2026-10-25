// Типы TypeScript против снимков ядра: contracts/v1/*.snap пишет `cargo test`
// (insta, src-tauri/src/commands/contract_tests.rs). Здесь те же примеры
// записаны в типах из types.ts: если тип в Rust изменился, снимок перестаёт
// совпадать с примером; если изменился тип в TypeScript — пример не проходит
// проверку типов (`npm run typecheck`).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import type {
  CommandName,
  CommandRequest,
  CommandResponse,
  CoreError,
  CoreEvents,
  ErrorCode,
  EventName,
  FileVersion,
  NameProblem,
  Settings,
  VaultSummary,
} from './types.ts';

const SNAPSHOT_DIR = join(import.meta.dirname, '..', '..', 'contracts', 'v1');

/** Тело снимка insta: JSON после заголовка `---\n...\n---\n`. */
function readSnapshot(file: string): unknown {
  const text = readFileSync(join(SNAPSHOT_DIR, file), 'utf8').replace(/\r\n/g, '\n');
  const match = /^---\n[\s\S]*?\n---\n([\s\S]*)$/.exec(text);
  if (!match?.[1]) throw new Error(`${file}: нет заголовка insta`);
  return JSON.parse(match[1]);
}

const err = (code: ErrorCode): CoreError => ({ code, message_key: `errors.${code}` });
const invalidName = (problem: NameProblem): CoreError => ({
  code: 'invalid_name',
  message_key: 'errors.invalid_name',
  name_problem: problem,
});
const invalidArgument = (field: string): CoreError => ({
  code: 'invalid_argument',
  message_key: 'errors.invalid_argument',
  field,
});
const version = (digit: string, size: number): FileVersion => ({
  hash: digit.repeat(64),
  size,
  modified_ms: 1_761_382_800_000,
});
const vault: VaultSummary = {
  id: '7f3c9a1e5b2d4c60',
  name: 'Работа',
  path: 'C:\\Users\\ivan\\Documents\\Работа',
};
const settings: Settings = {
  format_version: 1,
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'folder', path: ['Входящие'] },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
};
const conflictCopy = ['Проекты', 'Встреча (внешняя версия 2026-10-25 12-00).md'];

type CommandExample<K extends CommandName> = {
  command: K;
  request: CommandRequest<K>;
  response: CommandResponse<K>;
  errors: CoreError[];
};

const commands: { [K in CommandName]: CommandExample<K> } = {
  vault_list: {
    command: 'vault_list',
    request: null,
    response: { vaults: [{ ...vault, is_open: true }], last_opened: vault.id },
    errors: [err('io_failed')],
  },
  vault_current: {
    command: 'vault_current',
    request: null,
    response: { api_version: 1, vault },
    errors: [],
  },
  vault_create: {
    command: 'vault_create',
    request: { name: 'Работа' },
    response: { outcome: 'opened', vault },
    errors: [invalidArgument('name'), invalidName('reserved_name'), err('already_exists')],
  },
  vault_open_folder: {
    command: 'vault_open_folder',
    request: null,
    response: { outcome: 'cancelled' },
    errors: [err('vault_unavailable')],
  },
  vault_open: {
    command: 'vault_open',
    request: { id: vault.id },
    response: { outcome: 'focused' },
    errors: [err('vault_not_found'), err('vault_unavailable')],
  },
  vault_remove_from_list: {
    command: 'vault_remove_from_list',
    request: { id: vault.id },
    response: null,
    errors: [err('vault_not_found')],
  },
  tree_list: {
    command: 'tree_list',
    request: null,
    response: {
      entries: [
        { path: ['Проекты'], kind: 'folder' },
        { path: ['Проекты', 'Встреча.md'], kind: 'note' },
        { path: ['Проекты', 'схема.png'], kind: 'file' },
      ],
    },
    errors: [err('no_vault_open'), err('not_implemented')],
  },
  note_create: {
    command: 'note_create',
    request: { dir: ['Проекты'], name: 'Без названия.md' },
    response: { path: ['Проекты', 'Без названия.md'] },
    errors: [invalidName('duplicate'), err('invalid_path'), err('outside_vault')],
  },
  folder_create: {
    command: 'folder_create',
    request: { dir: [], name: 'Архив' },
    response: { path: ['Архив'] },
    errors: [invalidName('trailing_dot_or_space')],
  },
  entry_rename: {
    command: 'entry_rename',
    request: { path: ['Проекты', 'Без названия.md'], new_name: 'Встреча 2.md' },
    response: { path: ['Проекты', 'Встреча 2.md'] },
    errors: [err('not_found'), invalidName('forbidden_chars')],
  },
  entry_move: {
    command: 'entry_move',
    request: { path: ['Проекты', 'Встреча 2.md'], new_dir: ['Архив'] },
    response: { path: ['Архив', 'Встреча 2.md'] },
    errors: [err('already_exists'), err('not_found')],
  },
  entry_trash: {
    command: 'entry_trash',
    request: { path: ['Архив', 'Встреча 2.md'] },
    response: { outcome: 'trashed' },
    errors: [err('not_found'), err('io_failed')],
  },
  name_check: {
    command: 'name_check',
    request: { dir: [], name: 'CON.md' },
    response: { status: 'rejected', problem: 'reserved_name', message_key: 'names.reserved_name' },
    errors: [err('invalid_path')],
  },
  note_read: {
    command: 'note_read',
    request: { path: ['Проекты', 'Встреча.md'] },
    response: {
      text: '# Встреча\r\nСм. [[Альфа/План]]\r\n',
      format: { bom: true, line_ending: 'crlf' },
      version: version('a', 43),
      read_only: { reason: 'too_large', limit_bytes: 10 * 1024 * 1024 },
    },
    errors: [err('not_found'), err('not_a_note'), err('invalid_path'), err('outside_vault')],
  },
  note_write: {
    command: 'note_write',
    request: {
      path: ['Проекты', 'Встреча.md'],
      text: '# Встреча\nИтоги\n',
      format: { bom: false, line_ending: 'lf' },
      expected_version: version('a', 43),
    },
    response: { outcome: 'conflict_saved', version: version('b', 21), copy_path: conflictCopy },
    errors: [err('read_only'), err('not_found')],
  },
  links_resolve: {
    command: 'links_resolve',
    request: { from: ['Проекты', 'Встреча.md'], targets: ['Альфа/План', 'Нет такой'] },
    response: {
      results: [
        { target: 'Альфа/План', path: ['Проекты', 'Альфа', 'План.md'] },
        { target: 'Нет такой', path: null },
      ],
    },
    errors: [err('invalid_argument')],
  },
  links_suggest: {
    command: 'links_suggest',
    request: { from: ['Дневник.md'], query: 'встр' },
    response: {
      items: [
        { insert: 'Альфа/Встреча', name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] },
        { insert: '/Встреча', name: 'Встреча', path: ['Встреча.md'] },
      ],
    },
    errors: [err('no_vault_open')],
  },
  switcher_search: {
    command: 'switcher_search',
    request: { query: 'встр альф' },
    response: { items: [{ name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] }] },
    errors: [err('no_vault_open')],
  },
  settings_read: {
    command: 'settings_read',
    request: null,
    response: { settings },
    errors: [err('io_failed')],
  },
  settings_write: {
    command: 'settings_write',
    request: {
      settings: {
        ...settings,
        theme: 'dark',
        editor_mode: 'source',
        text_width: 'full',
        new_note_location: { kind: 'current_folder' },
      },
    },
    response: null,
    errors: [invalidArgument('settings.font_size')],
  },
  workspace_read: {
    command: 'workspace_read',
    request: null,
    response: { workspace: null },
    errors: [err('io_failed')],
  },
  workspace_write: {
    command: 'workspace_write',
    request: { workspace: { format_version: 1, panes: [{ note: ['Проекты', 'Встреча.md'] }] } },
    response: null,
    errors: [invalidArgument('workspace')],
  },
  open_external: {
    command: 'open_external',
    request: { url: 'https://example.org/статья?id=1' },
    response: { outcome: 'opened' },
    errors: [err('scheme_not_allowed')],
  },
};

const events: { [K in EventName]: { event: K; payload: CoreEvents[K] } } = {
  'vault:changed': {
    event: 'vault:changed',
    payload: {
      rescan: false,
      changes: [
        { kind: 'created', path: ['Новая.md'], entry_kind: 'note' },
        { kind: 'modified', path: ['Проекты', 'Встреча.md'] },
        { kind: 'removed', path: ['Старое'] },
        { kind: 'renamed', from: ['План.md'], to: ['Проекты', 'План.md'] },
      ],
    },
  },
  'note:changed': {
    event: 'note:changed',
    payload: { path: ['Проекты', 'Встреча.md'], version: version('c', 50) },
  },
  'note:renamed': {
    event: 'note:renamed',
    payload: { from: ['Проекты', 'Встреча.md'], to: ['Архив', 'Встреча.md'] },
  },
  'note:deleted': {
    event: 'note:deleted',
    payload: { path: ['Архив', 'Встреча.md'] },
  },
  'note:conflict-saved': {
    event: 'note:conflict-saved',
    payload: { path: ['Проекты', 'Встреча.md'], copy_path: conflictCopy },
  },
};

/** Имя файла снимка события: двоеточие на Windows в имени файла недопустимо. */
const eventSnapshotFile = (name: string): string => `event.${name.replace(':', '.')}.snap`;

describe('контракт API v1 совпадает со снимками ядра', () => {
  it.each(Object.values(commands))('команда $command', (example) => {
    expect(readSnapshot(`${example.command}.snap`)).toStrictEqual(example);
  });

  it.each(Object.values(events))('событие $event', (example) => {
    expect(readSnapshot(eventSnapshotFile(example.event))).toStrictEqual(example);
  });

  it('у каждого снимка есть пример в TypeScript, и наоборот', () => {
    const files = readdirSync(SNAPSHOT_DIR)
      .filter((file) => file.endsWith('.snap'))
      .sort();
    const expected = [
      ...Object.keys(commands).map((name) => `${name}.snap`),
      ...Object.keys(events).map(eventSnapshotFile),
    ].sort();
    expect(files).toEqual(expected);
  });

  it('ключ словаря ошибки — errors.<код>', () => {
    for (const example of Object.values(commands)) {
      for (const error of example.errors) {
        expect(error.message_key).toBe(`errors.${error.code}`);
      }
    }
  });
});
