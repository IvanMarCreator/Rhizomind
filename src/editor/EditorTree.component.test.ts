// Редактор и дерево в одном окне (2.3 + 2.4): переименование и удаление
// открытой заметки из дерева доходят до редактора сигналами `renamed` и
// `removed` раньше смены пути — ввод не пишется по старому пути и не
// пишется в удалённую заметку.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { FileVersion, Settings } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';
import { FakeVault } from '../tree/fakeVault.ts';

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: false,
  max_edit_size_bytes: 10 * 1024 * 1024,
};

const VERSION: FileVersion = { hash: 'a'.repeat(64), size: 9, modified_ms: 1 };

function setup() {
  const vault = new FakeVault([{ kind: 'note', path: ['План.md'] }]);
  const mocks: CoreMocks = {
    ...vault.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    settings_read: () => ({ settings: SETTINGS }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    note_read: () => ({ text: '# План\n', version: VERSION, read_only: null }),
    note_write: () => ({ outcome: 'saved', version: { ...VERSION, hash: 'b'.repeat(64) } }),
  };
  const calls = mockCore(mocks);
  render(Shell, { model: new WindowModel() });
  return { calls };
}

async function openPlan(): Promise<EditorView> {
  await screen.findByRole('treeitem', { name: 'План' });
  await fireEvent.click(screen.getByRole('treeitem', { name: 'План' }));
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return view;
}

const writes = (calls: [string, unknown][]) => calls.filter(([command]) => command === 'note_write');

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('редактор и дерево', () => {
  it('переименование открытой заметки в дереве: ввод пишется по новому пути с прежней версией', async () => {
    const { calls } = setup();
    const view = await openPlan();
    view.dispatch({ changes: { from: view.state.doc.length, insert: 'итог' }, userEvent: 'input.type' });

    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'F2' });
    await fireEvent.input(screen.getByRole('textbox', { name: 'Новое имя' }), { target: { value: 'План работ' } });
    await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Новое имя' }), { key: 'Enter' });
    await screen.findByRole('treeitem', { name: 'План работ' });
    expect(writes(calls)).toEqual([]);

    window.dispatchEvent(new Event('pagehide'));
    await waitFor(() =>
      expect(writes(calls)).toEqual([
        ['note_write', { request: { path: ['План работ.md'], text: '# План\nитог', expected_version: VERSION } }],
      ]),
    );
  });

  it('удаление открытой заметки в дереве: ввод не пишется', async () => {
    const { calls } = setup();
    const view = await openPlan();
    view.dispatch({ changes: { from: 0, insert: 'x' }, userEvent: 'input.type' });

    await fireEvent.keyDown(screen.getByRole('tree'), { key: 'Delete' });
    await screen.findByText('Нет открытой заметки');
    window.dispatchEvent(new Event('pagehide'));
    await new Promise((resolve) => setTimeout(resolve, 1200));

    expect(writes(calls)).toEqual([]);
  });
});
