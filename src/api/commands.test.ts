import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { core } from './commands.ts';
import { CoreCallError } from './core.ts';
import { mockCore } from './testing.ts';
import type { CommandName } from './types.ts';

describe('обёртки команд ядра', () => {
  beforeEach(() => {
    // Подмена ядра из @tauri-apps/api/mocks работает через window.
    vi.stubGlobal('window', globalThis);
  });

  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  it('команда с параметрами передаёт их ядру объектом request', async () => {
    const calls = mockCore({
      note_read: () => ({
        text: 'текст',
        format: { bom: false, line_ending: 'lf' },
        version: { hash: 'a'.repeat(64), size: 10, modified_ms: 1 },
        read_only: null,
      }),
    });

    const note = await core.noteRead({ path: ['Встреча.md'] });

    expect(note.text).toBe('текст');
    expect(calls).toEqual([['note_read', { request: { path: ['Встреча.md'] } }]]);
  });

  it('команда без параметров вызывается без аргументов', async () => {
    const calls = mockCore({ tree_list: () => ({ entries: [] }) });

    await expect(core.treeList()).resolves.toEqual({ entries: [] });
    expect(calls).toEqual([['tree_list', {}]]);
  });

  it('ошибка ядра приходит как CoreCallError с кодом и ключом словаря', async () => {
    mockCore({
      entry_rename: () => {
        throw { code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: 'reserved_name' };
      },
    });

    const failure = await core.entryRename({ path: ['a.md'], new_name: 'CON.md' }).catch((e: unknown) => e);

    expect(failure).toBeInstanceOf(CoreCallError);
    expect((failure as CoreCallError).error).toEqual({
      code: 'invalid_name',
      message_key: 'errors.invalid_name',
      name_problem: 'reserved_name',
    });
  });

  it('неподменённая команда отвечает not_implemented, как заглушки ядра', async () => {
    mockCore({});

    const failure = await core.vaultList().catch((e: unknown) => e);

    expect((failure as CoreCallError).error.code).toBe('not_implemented');
  });

  it('у каждой команды ядра есть обёртка', () => {
    const names: CommandName[] = [
      'vault_list',
      'vault_current',
      'vault_create',
      'vault_open_folder',
      'vault_open',
      'vault_remove_from_list',
      'tree_list',
      'note_create',
      'folder_create',
      'entry_rename',
      'entry_move',
      'entry_trash',
      'name_check',
      'note_read',
      'note_write',
      'links_resolve',
      'links_suggest',
      'switcher_search',
      'settings_read',
      'settings_write',
      'workspace_read',
      'workspace_write',
      'open_external',
    ];
    const camel = (name: string) => name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
    expect(Object.keys(core).sort()).toEqual(names.map(camel).sort());
  });
});
