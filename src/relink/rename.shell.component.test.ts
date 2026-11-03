// Переименование с обновлением ссылок в окне целиком (0.2-5.2; С2-3.5
// «в приложении», «Открытые заметки», инвариант 4): окно вопроса — у оболочки
// окна (переименование и из дерева, и из заголовка заметки), ответ меняет
// настройку хранилища, набранный в открытой заметке текст не теряется.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { EntryRenameRequest, FileVersion, Settings, SettingsWriteRequest, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';
import { FakeVault } from '../tree/fakeVault.ts';

// В jsdom нет раскладки: у Range нет размеров, а CodeMirror меряет текст при фокусе.
if (typeof Range.prototype.getClientRects !== 'function') {
  const empty = { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, toJSON: () => ({}) };
  Object.assign(Range.prototype, {
    getClientRects: () => Object.assign([], { item: () => null }) as unknown as DOMRectList,
    getBoundingClientRect: () => empty as DOMRect,
  });
}

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'source',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
  rename_links: 'ask',
};

const TOKEN = 'a'.repeat(32);
const OPEN: VaultPath = ['Начало.md'];
const TARGET: VaultPath = ['Встреча.md'];

/** Файлы «на диске» с версиями — как у ядра: `note:changed` несёт новую версию. */
class Disk {
  files = new Map<string, string>();
  #generation = new Map<string, number>();

  version(name: string): FileVersion {
    const n = this.#generation.get(name) ?? 1;
    return { hash: String(n).padStart(64, '0'), size: n, modified_ms: n };
  }

  set(path: VaultPath, text: string): FileVersion {
    const name = path.join('/');
    this.files.set(name, text);
    this.#generation.set(name, (this.#generation.get(name) ?? 1) + 1);
    return this.version(name);
  }

  get(path: VaultPath): string {
    return this.files.get(path.join('/')) ?? '';
  }
}

/**
 * `entry_rename` с `links: update`, как у ядра (С2-5.4): `links:flush-requested`
 * → ждать `links_flush_done` → переименовать → переписать → `note:changed` и
 * `links:updated`.
 */
function coreRename(disk: Disk, flushed: { done: Promise<void>; resolve: () => void }) {
  return async (request: EntryRenameRequest) => {
    const to = [...request.path.slice(0, -1), request.new_name];
    if (request.links !== 'update') return { path: to };
    expect(request.token).toBe(TOKEN);
    await emitCoreEvent('links:flush-requested', { token: TOKEN });
    await flushed.done;
    const stem = (path: VaultPath) => (path.at(-1) ?? '').replace(/\.md$/, '');
    const version = disk.set(OPEN, disk.get(OPEN).replaceAll(`[[${stem(request.path)}]]`, `[[${stem(to)}]]`));
    await emitCoreEvent('note:changed', { path: OPEN, version });
    await emitCoreEvent('links:updated', {
      reason: 'rename',
      updated: 1,
      failed: [],
      retry_token: null,
      renames: [{ from: request.path, to }],
      renames_total: 1,
    });
    return { path: to, relink: { updated: 1, failed: [], retry_token: null } };
  };
}

async function openWindow(disk: Disk, settings: Partial<Settings> = {}) {
  const vault = new FakeVault([
    { kind: 'note', path: OPEN },
    { kind: 'note', path: TARGET },
  ]);
  let resolve = () => {};
  const flushed = { done: new Promise<void>((r) => (resolve = r)), resolve: () => resolve() };
  const mocks: CoreMocks = {
    ...vault.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...SETTINGS, ...settings } }),
    settings_write: () => null,
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    note_read: ({ path }) => ({ text: disk.get(path), version: disk.version(path.join('/')), read_only: null }),
    note_write: ({ path, text }) => ({ outcome: 'saved', version: disk.set(path, text) }),
    links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: null })) }),
    relink_preview: () => ({ status: 'ready', notes: 1, links: 1, token: TOKEN }),
    links_flush_done: () => {
      flushed.resolve();
      return null;
    },
    entry_rename: coreRename(disk, flushed),
  };
  const calls = mockCore(mocks);
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(OPEN);
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  await waitFor(() => expect(view.state.doc.toString()).toBe(disk.get(OPEN)));
  return { calls, model, view };
}

const index = (calls: RecordedCall[], command: string) => calls.findIndex(([name]) => name === command);

async function renameInTree(name: string, value: string) {
  const row = await screen.findByRole('treeitem', { name });
  await fireEvent.click(row);
  // Щелчок по заметке открывает её; переименовать — F2 в дереве.
  await fireEvent.keyDown(screen.getByRole('tree', { name: 'Файлы хранилища' }), { key: 'F2' });
  const field = screen.getByRole('textbox', { name: 'Новое имя' });
  await fireEvent.input(field, { target: { value } });
  await fireEvent.keyDown(field, { key: 'Enter' });
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('переименование с обновлением ссылок в окне', () => {
  it('инвариант 4: открыта заметка, набран текст, переименование с обновлением ссылок — набранный текст сохраняется', async () => {
    const disk = new Disk();
    disk.set(OPEN, 'см. [[Встреча]]\n');
    disk.set(TARGET, 'текст встречи\n');
    const { calls, view, model } = await openWindow(disk);
    view.dispatch({ changes: { from: view.state.doc.length, insert: 'набранное' }, userEvent: 'input.type' });

    // Переименовывается другая заметка — из дерева клавишами, без щелчка:
    // открытой остаётся заметка с набранным текстом.
    const tree = screen.getByRole('tree', { name: 'Файлы хранилища' });
    tree.focus();
    await fireEvent.keyDown(tree, { key: 'Home' });
    await fireEvent.keyDown(tree, { key: 'F2' });
    const field = screen.getByRole('textbox', { name: 'Новое имя' });
    await fireEvent.input(field, { target: { value: 'Планёрка' } });
    await fireEvent.keyDown(field, { key: 'Enter' });

    const box = await screen.findByRole('dialog', { name: 'На «Встреча» ссылается 1 заметка. Обновить ссылки?' });
    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));

    await waitFor(() => expect(view.state.doc.toString()).toBe('см. [[Планёрка]]\nнабранное'));
    expect(disk.get(OPEN)).toBe('см. [[Планёрка]]\nнабранное');
    // Ввод записан до ответа ядру.
    expect(index(calls, 'note_write')).toBeGreaterThan(-1);
    expect(index(calls, 'note_write')).toBeLessThan(index(calls, 'links_flush_done'));
    expect(calls[index(calls, 'links_flush_done')]).toEqual(['links_flush_done', { request: { token: TOKEN, saved: true } }]);
    // Итог — уведомлением по `links:updated`; открытая заметка осталась открытой.
    expect(await screen.findByText('Ссылки обновлены в 1 заметке')).toBeTruthy();
    expect(model.currentPath).toEqual(OPEN);
  });

  it('Всегда обновлять — без окна, уведомление «Ссылки обновлены в 1 заметке»', async () => {
    const disk = new Disk();
    disk.set(OPEN, 'см. [[Встреча]]\n');
    const { calls } = await openWindow(disk, { rename_links: 'update' });
    await renameInTree('Встреча', 'Планёрка');

    expect(await screen.findByText('Ссылки обновлены в 1 заметке')).toBeTruthy();
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(calls.find(([name]) => name === 'entry_rename')?.[1]).toMatchObject({ request: { links: 'update', token: TOKEN } });
  });

  it('«Больше не спрашивать» + «Обновить ссылки» — в .rhizomind/settings.json записано rename_links: update', async () => {
    const disk = new Disk();
    disk.set(OPEN, 'см. [[Встреча]]\n');
    const { calls, model } = await openWindow(disk);
    await renameInTree('Встреча', 'Планёрка');
    const box = await screen.findByRole('dialog');
    await fireEvent.click(within(box).getByRole('checkbox', { name: 'Больше не спрашивать' }));
    await fireEvent.click(within(box).getByRole('button', { name: 'Обновить ссылки' }));

    await waitFor(() => {
      const write = calls.filter(([name]) => name === 'settings_write').at(-1)?.[1] as { request: SettingsWriteRequest } | undefined;
      expect(write?.request.settings.rename_links).toBe('update');
    });
    expect(model.settings.rename_links).toBe('update');
  });

  it('пока открыто окно вопроса, горячие клавиши окна не действуют', async () => {
    const disk = new Disk();
    disk.set(OPEN, 'см. [[Встреча]]\n');
    await openWindow(disk);
    await renameInTree('Встреча', 'Планёрка');
    await screen.findByRole('dialog');
    await fireEvent.keyDown(window, { key: 'o', code: 'KeyO', ctrlKey: true });
    expect(screen.queryByRole('dialog', { name: 'Быстрый переход' })).toBeNull();
  });
});
