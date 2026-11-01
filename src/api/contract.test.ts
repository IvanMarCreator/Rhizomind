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
  IndexReply,
  IndexState,
  LinksMode,
  LinksUpdateReason,
  NameCheck,
  NameProblem,
  NameWarning,
  NewNoteLocation,
  NoteWriteResponse,
  ClipboardReadOutcome,
  OpenExternalOutcome,
  PendingReason,
  ReadOnlyReason,
  RelinkFailureReason,
  RelinkPreviewRequest,
  RelinkToken,
  RenameLinks,
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
  hotkeys: { 'quick_switcher.open': ['Ctrl+KeyO', 'Ctrl+KeyP'], 'tree.delete': [] },
  rename_links: 'ask',
};
const token = (digit: string): RelinkToken => digit.repeat(32);
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
    response: { vaults: [{ ...vault, is_open: true, available: true }], last_opened: vault.id },
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
    request: { name: 'Работа', parent: 'C:\\Users\\ivan\\Documents' },
    response: { outcome: 'opened', vault },
    errors: [
      invalidArgument('name'),
      invalidArgument('parent'),
      invalidArgument('window'),
      invalidName('reserved_name'),
      err('already_exists'),
      err('nested_vault'),
    ],
  },
  folder_pick: {
    command: 'folder_pick',
    request: null,
    response: { outcome: 'picked', path: 'C:\\Users\\ivan\\Documents' },
    errors: [err('nested_vault'), err('vault_unavailable'), invalidArgument('window')],
  },
  vault_open_folder: {
    command: 'vault_open_folder',
    request: null,
    response: { outcome: 'cancelled' },
    errors: [err('vault_unavailable'), err('nested_vault'), invalidArgument('window')],
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
    errors: [err('vault_not_found'), invalidArgument('window')],
  },
  vault_manager_open: {
    command: 'vault_manager_open',
    request: null,
    response: null,
    errors: [err('io_failed')],
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
        rename_links: 'update',
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
  window_close_ready: { command: 'window_close_ready', request: null, response: null, errors: [] },
  window_close_cancel: { command: 'window_close_cancel', request: null, response: null, errors: [] },
  clipboard_read_text: {
    command: 'clipboard_read_text',
    request: null,
    response: { outcome: 'text', text: 'Текст из буфера\nвторая строка' },
    errors: [err('no_vault_open'), err('io_failed')],
  },
  // ---- 0.2 «Связи» ----
  index_status: {
    command: 'index_status',
    request: null,
    response: { state: 'building', done: 1_200, total: 10_000 },
    errors: [err('no_vault_open')],
  },
  backlinks_list: {
    command: 'backlinks_list',
    request: { path: ['Проекты', 'Встреча.md'] },
    response: {
      status: 'ready',
      notes_total: 1,
      truncated: false,
      notes: [
        {
          from: ['Дневник', '2026-11-01.md'],
          name: '2026-11-01',
          links_total: 2,
          links: [
            {
              line: 3,
              column_utf16: 9,
              snippet: 'Обсудили [[Встреча|встречу]] и <b>планы</b>',
              from_utf16: 9,
              to_utf16: 28,
            },
            { line: 7, column_utf16: 0, snippet: '![[Встреча#Итоги]]', from_utf16: 0, to_utf16: 18 },
          ],
        },
      ],
    },
    errors: [err('invalid_path'), invalidArgument('path'), err('no_vault_open')],
  },
  tags_list: {
    command: 'tags_list',
    request: null,
    response: {
      status: 'ready',
      tags: [
        { tag: 'Проект', key: 'проект', count: 12 },
        { tag: 'работа/отчёт', key: 'работа/отчёт', count: 3 },
      ],
    },
    errors: [err('no_vault_open')],
  },
  tag_notes: {
    command: 'tag_notes',
    request: { key: 'проект' },
    response: { status: 'ready', notes_total: 2, notes: [['Проекты', 'Альфа.md'], ['Встреча.md']] },
    errors: [invalidArgument('key'), err('no_vault_open')],
  },
  relink_preview: {
    command: 'relink_preview',
    request: { op: 'rename', path: ['Проекты', 'Встреча.md'], new_name: 'Планёрка.md' },
    response: { status: 'ready', notes: 5, links: 7, token: token('a') },
    errors: [invalidName('note_extension'), err('not_found'), err('no_vault_open')],
  },
  relink_answer: {
    command: 'relink_answer',
    request: { token: token('d'), apply: true },
    response: null,
    errors: [invalidArgument('token'), err('no_vault_open')],
  },
  links_flush_done: {
    command: 'links_flush_done',
    request: { token: token('e'), saved: true },
    response: null,
    errors: [invalidArgument('token'), err('no_vault_open')],
  },
  window_shown_note: {
    command: 'window_shown_note',
    request: { path: ['Проекты', 'Встреча.md'] },
    response: null,
    errors: [err('not_found'), err('not_a_note'), err('invalid_path'), err('no_vault_open')],
  },
};

/** Вторые примеры команд — снимки `<команда>.<вариант>.snap` (COMMAND_VARIANTS в Rust). */
const commandVariants: Array<CommandExample<CommandName> & { variant: string }> = [
  {
    command: 'entry_rename',
    variant: 'update',
    request: {
      path: ['Проекты', 'Встреча.md'],
      new_name: 'Планёрка.md',
      links: 'update',
      token: token('a'),
    },
    response: {
      path: ['Проекты', 'Планёрка.md'],
      relink: {
        updated: 4,
        failed: [{ path: ['Архив', 'Старое.md'], reason: 'busy' }],
        retry_token: token('b'),
      },
    },
    errors: [invalidArgument('token'), err('not_found'), invalidName('duplicate')],
  } satisfies CommandExample<'entry_rename'> & { variant: string },
  {
    command: 'entry_move',
    variant: 'update',
    request: { path: ['Альфа', 'Встреча.md'], new_dir: ['Бета'], links: 'update', token: token('c') },
    response: { path: ['Бета', 'Встреча.md'], relink: { updated: 2, failed: [], retry_token: null } },
    errors: [invalidArgument('token'), err('already_exists')],
  } satisfies CommandExample<'entry_move'> & { variant: string },
  {
    command: 'relink_preview',
    variant: 'index_building',
    request: { op: 'move', path: ['Альфа', 'Встреча.md'], new_dir: ['Бета'] },
    response: { status: 'index_building', done: 1_200, total: 10_000 },
    errors: [invalidArgument('new_dir')],
  } satisfies CommandExample<'relink_preview'> & { variant: string },
];

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
  'window:close-requested': {
    event: 'window:close-requested',
    payload: { sessionEnd: true },
  },
  'index:status': {
    event: 'index:status',
    payload: { state: 'ready', done: 10_000, total: 10_000 },
  },
  'index:changed': {
    event: 'index:changed',
    payload: {},
  },
  'links:flush-requested': {
    event: 'links:flush-requested',
    payload: { token: token('f') },
  },
  'links:pending': {
    event: 'links:pending',
    payload: {
      token: token('1'),
      reason: 'external_rename',
      notes: 12,
      links: 15,
      renames: [{ from: ['Встреча.md'], to: ['Архив', 'Встреча.md'] }],
      renames_total: 3,
    },
  },
  'links:progress': {
    event: 'links:progress',
    payload: { token: null, done: 250, total: 1_000 },
  },
  'links:updated': {
    event: 'links:updated',
    payload: {
      reason: 'external_rename',
      updated: 11,
      failed: [
        { path: ['Большая.md'], reason: 'too_large' },
        { path: ['Открыта в Word.md'], reason: 'busy' },
      ],
      retry_token: token('2'),
    },
  },
};

/** Имя файла снимка события: двоеточие на Windows в имени файла недопустимо. */
const eventSnapshotFile = (name: string): string => `event.${name.replace(':', '.')}.snap`;

describe('контракт API v1 совпадает со снимками ядра', () => {
  it.each(Object.values(commands))('команда $command', (example) => {
    expect(readSnapshot(`${example.command}.snap`)).toStrictEqual(example);
  });

  it.each(commandVariants)('команда $command, пример $variant', (example) => {
    expect(readSnapshot(`${example.command}.${example.variant}.snap`)).toStrictEqual(example);
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
      ...commandVariants.map((example) => `${example.command}.${example.variant}.snap`),
    ].sort();
    expect(files).toEqual(expected);
  });

  it('ключ словаря ошибки — errors.<код>', () => {
    for (const example of [...Object.values(commands), ...commandVariants]) {
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
    nested_vault: true,
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
  'ClipboardReadOutcome.outcome': { text: true, empty: true, too_large: true, not_focused: true } satisfies Record<
    ClipboardReadOutcome['outcome'],
    true
  >,
  'TreeChange.kind': { created: true, modified: true, removed: true, renamed: true } satisfies Record<
    TreeChange['kind'],
    true
  >,
  RenameLinks: { ask: true, update: true } satisfies Record<RenameLinks, true>,
  LinksMode: { keep: true, update: true } satisfies Record<LinksMode, true>,
  IndexState: { building: true, ready: true } satisfies Record<IndexState, true>,
  'IndexReply.status': { ready: true, index_building: true } satisfies Record<IndexReply<object>['status'], true>,
  'RelinkPreviewRequest.op': { rename: true, move: true } satisfies Record<RelinkPreviewRequest['op'], true>,
  PendingReason: { external_rename: true, duplicate: true } satisfies Record<PendingReason, true>,
  LinksUpdateReason: {
    rename: true,
    external_rename: true,
    duplicate: true,
    resumed: true,
    retry: true,
  } satisfies Record<LinksUpdateReason, true>,
  RelinkFailureReason: { too_large: true, not_utf8: true, busy: true, io: true } satisfies Record<
    RelinkFailureReason,
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
