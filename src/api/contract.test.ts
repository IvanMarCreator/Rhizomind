// Типы TypeScript против снимков ядра: contracts/v1/*.snap пишет `cargo test`
// (insta, src-tauri/src/commands/contract_tests.rs). Здесь те же примеры
// записаны в типах из types.ts: если тип в Rust изменился, снимок перестаёт
// совпадать с примером; если изменился тип в TypeScript — пример не проходит
// проверку типов (`npm run typecheck`).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { LIMITS } from './limits.ts';
import type {
  CommandName,
  CommandRequest,
  CommandResponse,
  CoreError,
  CoreEvents,
  ErrorCode,
  EditorMode,
  EntryKind,
  EventName,
  FileVersion,
  NameCheck,
  NameProblem,
  NameWarning,
  NewNoteLocation,
  NoteWriteResponse,
  OpenExternalOutcome,
  ReadOnlyReason,
  Settings,
  TextWidth,
  Theme,
  TrashOutcome,
  TreeChange,
  VaultChooseOutcome,
  VaultOpenOutcome,
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
    response: { api_version: 1, vault: { id: vault.id, name: vault.name } },
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
        { path: ['Проекты', 'схема.png'], kind: 'attachment' },
      ],
    },
    errors: [err('no_vault_open'), err('io_failed')],
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
    errors: [err('not_found'), invalidName('forbidden_chars'), invalidName('note_extension')],
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
    request: { dir: ['Проекты'], name: 'Встреча.md', renaming: ['Проекты', 'встреча.md'] },
    response: { status: 'ok', warning: 'unlinkable' },
    errors: [err('invalid_path'), invalidArgument('dir')],
  },
  note_read: {
    command: 'note_read',
    request: { path: ['Проекты', 'Встреча.md'] },
    response: {
      text: '# Встреча\nСм. [[Альфа/План]]\n',
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
      expected_version: version('a', 43),
    },
    response: { outcome: 'conflict_saved', version: version('b', 21), copy_path: conflictCopy },
    errors: [err('read_only'), err('not_found'), invalidArgument('text')],
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
        { insert: 'Альфа/Встреча', name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'], linkable: true },
        { insert: '/Встреча', name: 'Встреча', path: ['Встреча.md'], linkable: true },
        { insert: null, name: '#Встреча 2', path: ['#Встреча 2.md'], linkable: false },
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
  'vault:availability': {
    event: 'vault:availability',
    payload: { available: false },
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
      'enums.snap',
      'limits.snap',
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

// Все варианты каждого перечисления. `satisfies Record<…, true>` требует
// ровно все варианты типа: пропуск или лишний вариант — ошибка проверки типов.
const enums = {
  ErrorCode: {
    not_implemented: true,
    invalid_argument: true,
    no_vault_open: true,
    vault_not_found: true,
    vault_unavailable: true,
    invalid_path: true,
    outside_vault: true,
    not_found: true,
    already_exists: true,
    invalid_name: true,
    not_a_note: true,
    read_only: true,
    scheme_not_allowed: true,
    io_failed: true,
  } satisfies Record<ErrorCode, true>,
  NameProblem: {
    empty: true,
    forbidden_chars: true,
    reserved_name: true,
    trailing_dot_or_space: true,
    too_long: true,
    duplicate: true,
    note_extension: true,
  } satisfies Record<NameProblem, true>,
  NameWarning: { unlinkable: true, hidden: true } satisfies Record<NameWarning, true>,
  EntryKind: { folder: true, note: true, attachment: true } satisfies Record<EntryKind, true>,
  Theme: { system: true, light: true, dark: true } satisfies Record<Theme, true>,
  EditorMode: { live_preview: true, source: true } satisfies Record<EditorMode, true>,
  TextWidth: { column: true, full: true } satisfies Record<TextWidth, true>,
  'NewNoteLocation.kind': { vault_root: true, current_folder: true, folder: true } satisfies Record<
    NewNoteLocation['kind'],
    true
  >,
  'ReadOnlyReason.reason': { too_large: true, invalid_utf8: true } satisfies Record<ReadOnlyReason['reason'], true>,
  'NoteWriteResponse.outcome': { saved: true, conflict_saved: true } satisfies Record<
    NoteWriteResponse['outcome'],
    true
  >,
  'NameCheck.status': { ok: true, rejected: true } satisfies Record<NameCheck['status'], true>,
  'TrashOutcome.outcome': { trashed: true, deleted_permanently: true, cancelled: true } satisfies Record<
    TrashOutcome['outcome'],
    true
  >,
  'VaultChooseOutcome.outcome': { opened: true, cancelled: true } satisfies Record<
    VaultChooseOutcome['outcome'],
    true
  >,
  'VaultOpenOutcome.outcome': { opened: true, focused: true } satisfies Record<VaultOpenOutcome['outcome'], true>,
  'OpenExternalOutcome.outcome': { opened: true, cancelled: true } satisfies Record<
    OpenExternalOutcome['outcome'],
    true
  >,
  'TreeChange.kind': { created: true, modified: true, removed: true, renamed: true } satisfies Record<
    TreeChange['kind'],
    true
  >,
};

describe('перечисления и пределы совпадают с ядром', () => {
  it('варианты каждого перечисления — как в Rust', () => {
    const fromRust = readSnapshot('enums.snap') as Record<string, string[]>;
    const fromTs = Object.fromEntries(
      Object.entries(enums).map(([name, variants]) => [name, Object.keys(variants).sort()]),
    );
    const sortedRust = Object.fromEntries(
      Object.entries(fromRust).map(([name, variants]) => [name, [...variants].sort()]),
    );
    expect(fromTs).toStrictEqual(sortedRust);
  });

  it('пределы параметров — как в Rust', () => {
    expect(readSnapshot('limits.snap')).toStrictEqual(LIMITS);
  });
});
