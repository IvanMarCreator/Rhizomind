// Обновление ссылок в окне (0.2-5.1, С2-3.5 «Открытые заметки», С2-5.4, Р7):
// по `links:flush-requested` окно пишет ввод и отвечает `links_flush_done`;
// переписанная ядром открытая заметка получает текст одной правкой середины;
// итог — уведомление по `links:updated`.
import { StateEffect } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { FileVersion, NoteReadResponse, Settings, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type RecordedCall } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';

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
  rename_links: 'update',
};

const TOKEN = 'e'.repeat(32);
const NOTE: VaultPath = ['Начало.md'];
const OTHER: VaultPath = ['Проекты', 'План работ.md'];

class Disk {
  files = new Map<string, string>();
  #generation = new Map<string, number>();

  version(name: string): FileVersion {
    const n = this.#generation.get(name) ?? 1;
    return { hash: String(n).padStart(64, '0'), size: n, modified_ms: n };
  }

  read(path: VaultPath): NoteReadResponse {
    const name = path.join('/');
    return { text: this.files.get(name) ?? '', version: this.version(name), read_only: null };
  }

  set(path: VaultPath, text: string): FileVersion {
    const name = path.join('/');
    this.files.set(name, text);
    this.#generation.set(name, (this.#generation.get(name) ?? 1) + 1);
    return this.version(name);
  }
}

async function openWindow(disk: Disk) {
  const calls = mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...SETTINGS } }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [] }),
    note_read: ({ path }) => disk.read(path),
    note_write: ({ path, text }) => ({ outcome: 'saved', version: disk.set(path, text) }),
    links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: null })) }),
    links_flush_done: () => null,
    relink_answer: () => null,
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(NOTE);
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  await waitFor(() => expect(view.state.doc.toString()).toBe(disk.files.get('Начало.md')));
  return { calls, model, view };
}

const named = (calls: RecordedCall[], command: string) => calls.findIndex(([name]) => name === command);

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('окно и обновление ссылок', () => {
  it('ввод всех заметок записан до links_flush_done', async () => {
    const disk = new Disk();
    disk.set(NOTE, 'первая\nсм. [[Встреча]]\nпоследняя');
    const { calls, view } = await openWindow(disk);
    view.dispatch({ changes: { from: view.state.doc.length, insert: ' строка' }, userEvent: 'input.type' });

    await emitCoreEvent('links:flush-requested', { token: TOKEN });

    await waitFor(() => expect(named(calls, 'links_flush_done')).toBeGreaterThan(-1));
    expect(named(calls, 'note_write')).toBeGreaterThan(-1);
    expect(named(calls, 'note_write')).toBeLessThan(named(calls, 'links_flush_done'));
    expect(calls[named(calls, 'links_flush_done')]).toEqual(['links_flush_done', { request: { token: TOKEN, saved: true } }]);
    expect(disk.files.get('Начало.md')).toBe('первая\nсм. [[Встреча]]\nпоследняя строка');
  });

  it('открытая заметка получает текст одной правкой середины, курсор на месте', async () => {
    const disk = new Disk();
    disk.set(NOTE, 'первая\nсм. [[Встреча]]\nпоследняя');
    const { calls, view } = await openWindow(disk);
    const cursor = view.state.doc.length - 2;
    view.dispatch({ selection: { anchor: cursor } });
    const changes: [number, number, string][] = [];
    view.dispatch({
      effects: StateEffect.appendConfig.of(
        EditorView.updateListener.of((update) => {
          for (const tr of update.transactions) {
            tr.changes.iterChanges((from, to, _fromB, _toB, inserted) => changes.push([from, to, inserted.toString()]));
          }
        }),
      ),
    });

    await emitCoreEvent('links:flush-requested', { token: TOKEN });
    await waitFor(() => expect(named(calls, 'links_flush_done')).toBeGreaterThan(-1));
    const version = disk.set(NOTE, 'первая\nсм. [[Планёрка]]\nпоследняя');
    await emitCoreEvent('note:changed', { path: NOTE, version });

    await waitFor(() => expect(view.state.doc.toString()).toBe('первая\nсм. [[Планёрка]]\nпоследняя'));
    // Одна правка — только отличающаяся середина; начало и конец документа не тронуты.
    expect(changes).toHaveLength(1);
    const [from, to, inserted] = changes[0] ?? [0, 0, ''];
    expect(from).toBeGreaterThanOrEqual('первая\nсм. [['.length);
    expect(to).toBeLessThanOrEqual('первая\nсм. [[Встреча'.length);
    expect(inserted.length).toBeLessThanOrEqual('Планёрка'.length);
    // Курсор — на том же месте текста (сдвинут на разницу длин).
    expect(view.state.selection.main.head).toBe(cursor + ('Планёрка'.length - 'Встреча'.length));
    expect(view.state.doc.sliceString(view.state.selection.main.head)).toBe('яя');
    // Ввод уже записан, текст с диска — не ввод: записей после flush нет.
    expect(calls.filter(([name]) => name === 'note_write')).toHaveLength(0);
  });

  it('links:updated — уведомление в окне; Показать → щелчок открывает заметку', async () => {
    const disk = new Disk();
    disk.set(NOTE, 'текст');
    disk.set(OTHER, 'план');
    const { model } = await openWindow(disk);

    await emitCoreEvent('links:updated', {
      reason: 'rename',
      updated: 4,
      failed: [{ path: OTHER, reason: 'busy' }],
      retry_token: 'd'.repeat(32),
      renames: [],
      renames_total: 0,
    });

    expect(await screen.findByText('Ссылки обновлены в 4 заметках')).toBeTruthy();
    const notice = (await screen.findByText('Не удалось обновить ссылки в 1 заметке')).closest('.notice') as HTMLElement;
    await fireEvent.click(within(notice).getByRole('button', { name: 'Показать' }));
    await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: /План работ/ }));

    await waitFor(() => expect(model.currentPath).toEqual(OTHER));
  });

  it('Повторить — relink_answer с retry_token и apply: true', async () => {
    const disk = new Disk();
    disk.set(NOTE, 'текст');
    const { calls } = await openWindow(disk);

    await emitCoreEvent('links:updated', {
      reason: 'rename',
      updated: 0,
      failed: [{ path: OTHER, reason: 'io' }],
      retry_token: 'd'.repeat(32),
      renames: [],
      renames_total: 0,
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Повторить' }));

    await waitFor(() =>
      expect(calls).toContainEqual(['relink_answer', { request: { token: 'd'.repeat(32), apply: true } }]),
    );
  });

  it('0.2-6.3: links:pending — уведомление с кнопками; ответ — relink_answer со знаком из события', async () => {
    const disk = new Disk();
    disk.set(NOTE, 'текст');
    const { calls } = await openWindow(disk);

    await emitCoreEvent('links:pending', {
      token: 'b'.repeat(32),
      reason: 'external_rename',
      notes: 4,
      links: 5,
      renames: [{ from: ['Встреча.md'], to: ['Планёрка.md'] }],
      renames_total: 1,
    });
    const notice = (await screen.findByText('Заметка «Встреча» переименована на диске в «Планёрка», на неё ссылаются 4 заметки')).closest(
      '.notice',
    ) as HTMLElement;
    await fireEvent.click(within(notice).getByRole('button', { name: 'Обновить ссылки' }));

    await waitFor(() => expect(calls).toContainEqual(['relink_answer', { request: { token: 'b'.repeat(32), apply: true } }]));
    await waitFor(() => expect(screen.queryByText(/переименована на диске/)).toBeNull());
  });
});
