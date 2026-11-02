// Очередь записей раскладки окна (0.2-2.2, QA): если сериализация раскладки
// бросила синхронно, очередь не «залипает» — следующие записи уходят.
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Settings } from '../api/index.ts';
import { mockCore } from '../api/testing.ts';
import { WindowModel } from './window.svelte.ts';

const failNext = vi.hoisted(() => ({ value: false }));

vi.mock('../core/layout/layout.ts', async (importOriginal) => {
  const original = await importOriginal<typeof import('../core/layout/layout.ts')>();
  return {
    ...original,
    serializeLayout: (...args: Parameters<typeof original.serializeLayout>) => {
      if (failNext.value) {
        failNext.value = false;
        throw new Error('сбой сериализации');
      }
      return original.serializeLayout(...args);
    },
  };
});

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
  rename_links: 'ask',
};

afterEach(() => clearMocks());

describe('очередь записей раскладки', () => {
  it('сериализация бросила синхронно — уведомление, следующие записи уходят', async () => {
    const written: unknown[] = [];
    mockCore({
      vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
      settings_read: () => ({ settings: SETTINGS }),
      workspace_read: () => ({ workspace: null }),
      workspace_write: ({ workspace }) => {
        written.push(workspace);
        return null;
      },
    });
    const model = new WindowModel();
    await model.start();

    failNext.value = true;
    model.open(['А.md']);
    await vi.waitFor(() => expect(model.notices).toHaveLength(1));
    expect(written).toHaveLength(0);

    model.open(['Б.md']);
    await vi.waitFor(() => expect(written).toHaveLength(1));
  });
});
