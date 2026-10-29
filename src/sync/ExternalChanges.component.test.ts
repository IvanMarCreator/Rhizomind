// Внешние изменения файлов в окне (С3.6, задача 2.9, инвариант 4) на
// подмене ядра: события `note:changed`, `note:renamed`, `note:deleted` и
// полная сверка доходят до редактора, истории и заголовка; ни в одном случае
// не теряется текст. Подменный «диск» ведёт версии и конфликт так же, как
// ядро (src-tauri/src/vault/mod.rs, `write_note`): запись со старой ожидаемой
// версией — текст из редактора в заметке, версия с диска — копией рядом.
import { EditorView } from '@codemirror/view';
import { undo } from '@codemirror/commands';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import type { FileVersion, NoteReadResponse, NoteWriteResponse, Settings, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';
import { FakeVault } from '../tree/fakeVault.ts';

const key = (path: VaultPath) => path.join('/');
const COPY_SUFFIX = ' (внешняя версия 2026-10-27 10-00).md';

/** Тексты заметок и их версии; дерево — FakeVault. */
class Disk {
  readonly texts = new Map<string, string>();
  readonly generations = new Map<string, number>();
  readonly tree: FakeVault;

  constructor(notes: Record<string, string>) {
    for (const [name, text] of Object.entries(notes)) this.texts.set(name, text);
    this.tree = new FakeVault(Object.keys(notes).map((name) => ({ kind: 'note' as const, path: name.split('/') })));
  }

  version(name: string): FileVersion {
    const generation = this.generations.get(name) ?? 1;
    return { hash: `${name}#${generation}`.padEnd(64, '.'), size: generation, modified_ms: generation };
  }

  /** Правка «в другом редакторе»: новое содержимое и версия; возвращает версию для `note:changed`. */
  external(name: string, text: string): FileVersion {
    this.texts.set(name, text);
    this.generations.set(name, (this.generations.get(name) ?? 1) + 1);
    return this.version(name);
  }

  read(path: VaultPath): NoteReadResponse {
    const text = this.texts.get(key(path));
    if (text === undefined) throw { code: 'not_found', message_key: 'errors.not_found' };
    return { text, version: this.version(key(path)), read_only: null };
  }

  write(path: VaultPath, text: string, expected: FileVersion): NoteWriteResponse {
    const name = key(path);
    const disk = this.texts.get(name);
    if (disk === undefined) throw { code: 'not_found', message_key: 'errors.not_found' };
    const conflict = expected.hash !== this.version(name).hash;
    let copy: VaultPath | null = null;
    if (conflict) {
      copy = [...path.slice(0, -1), (path.at(-1) ?? '').replace(/\.md$/, '') + COPY_SUFFIX];
      this.texts.set(key(copy), disk);
      this.tree.entries.set(key(copy), { kind: 'note', path: copy });
    }
    this.texts.set(name, text);
    this.generations.set(name, (this.generations.get(name) ?? 1) + 1);
    const version = this.version(name);
    return copy ? { outcome: 'conflict_saved', version, copy_path: copy } : { outcome: 'saved', version };
  }

  /** Переименование «в Проводнике». */
  rename(from: string, to: string): void {
    const text = this.texts.get(from) ?? '';
    this.texts.delete(from);
    this.texts.set(to, text);
    this.tree.entries.delete(from);
    this.tree.entries.set(to, { kind: 'note', path: to.split('/') });
    this.generations.set(to, this.generations.get(from) ?? 1);
  }

  remove(name: string): void {
    this.texts.delete(name);
    this.tree.entries.delete(name);
  }
}

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: false,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
};

async function openWindow(disk: Disk, first: VaultPath, extra: CoreMocks = {}) {
  const calls = mockCore({
    ...disk.tree.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    settings_read: () => ({ settings: SETTINGS }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    window_shown_note: () => null,
    note_read: ({ path }) => disk.read(path),
    note_write: ({ path, text, expected_version }) => disk.write(path, text, expected_version),
    ...extra,
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(first);
  const view = await editorView();
  return { calls, model, view };
}

async function editorView(): Promise<EditorView> {
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return view;
}

/** Ввод пользователя в позиции `at` (по умолчанию — в конце). */
function type(view: EditorView, insert: string, at = view.state.doc.length) {
  view.dispatch({ changes: { from: at, insert }, userEvent: 'input.type' });
}

const writes = (calls: RecordedCall[]) => calls.filter(([command]) => command === 'note_write');

const headerPath = () =>
  screen
    .getByRole('navigation', { name: 'Путь к заметке' })
    .textContent?.replace(/\s*\/\s*/g, '/')
    .trim();

afterEach(() => {
  cleanup();
  clearMocks();
  delete document.documentElement.dataset.theme;
});

// Прогрев: первое открытие окна в процессе тестов в разы дольше следующих.
beforeAll(async () => {
  await openWindow(new Disk({ 'А.md': 'а' }), ['А.md']);
  cleanup();
  clearMocks();
}, 60_000);

describe('С3.6: открытая заметка изменилась снаружи', () => {
  it('без ввода: текст с диска, курсор остаётся на своём месте, на диск ничего не пишется', async () => {
    const disk = new Disk({ 'Встреча.md': 'один\nдва\nтри\n' });
    const { calls, view } = await openWindow(disk, ['Встреча.md']);
    const cursor = 'один\nдва\n'.length;
    view.dispatch({ selection: { anchor: cursor } });

    const version = disk.external('Встреча.md', 'ноль\nодин\nдва\nтри\n');
    await emitCoreEvent('note:changed', { path: ['Встреча.md'], version });

    await waitFor(() => expect(view.state.doc.toString()).toBe('ноль\nодин\nдва\nтри\n'));
    // Курсор — в начале той же строки «три».
    const head = view.state.selection.main.head;
    expect(view.state.doc.lineAt(head).text).toBe('три');
    expect(head).toBe(view.state.doc.lineAt(head).from);
    await new Promise((resolve) => setTimeout(resolve, 1200));
    expect(writes(calls)).toEqual([]);
    expect(screen.getByText('Сохранено')).toBeTruthy();
  });

  it('обновление с диска не попадает в отмену (Ctrl+Z не возвращает прежний текст файла)', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { view } = await openWindow(disk, ['Встреча.md']);

    await emitCoreEvent('note:changed', { path: ['Встреча.md'], version: disk.external('Встреча.md', 'снаружи\n') });
    await waitFor(() => expect(view.state.doc.toString()).toBe('снаружи\n'));

    expect(undo(view)).toBe(false);
    expect(view.state.doc.toString()).toBe('снаружи\n');
  });

  it('при несохранённом вводе сохраняются обе версии: ваша — в заметке, с диска — копией; уведомление', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { view } = await openWindow(disk, ['Встреча.md']);
    type(view, 'мой ввод\n');

    const version = disk.external('Встреча.md', 'начало\nправка снаружи\n');
    await emitCoreEvent('note:changed', { path: ['Встреча.md'], version });

    // Сразу по событию, не дожидаясь автосохранения (1 с).
    await waitFor(() => expect(disk.texts.get('Встреча.md')).toBe('начало\nмой ввод\n'), { timeout: 500 });
    expect(disk.texts.get(`Встреча${COPY_SUFFIX}`)).toBe('начало\nправка снаружи\n');
    expect(view.state.doc.toString()).toBe('начало\nмой ввод\n');
    expect(await screen.findByText(/Сохранены обе версии/)).toBeTruthy();
  });

  it('событие о своей записи (версия та же) не перечитывает заметку', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { calls, view } = await openWindow(disk, ['Встреча.md']);
    type(view, 'x');
    await waitFor(() => expect(disk.texts.get('Встреча.md')).toBe('начало\nx'), { timeout: 3000 });
    const reads = calls.filter(([command]) => command === 'note_read').length;

    await emitCoreEvent('note:changed', { path: ['Встреча.md'], version: disk.version('Встреча.md') });
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(calls.filter(([command]) => command === 'note_read').length).toBe(reads);
  });

  it('полная сверка с диском (rescan) — открытая заметка сверяется и обновляется', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { view } = await openWindow(disk, ['Встреча.md']);

    disk.external('Встреча.md', 'после сверки\n');
    await emitCoreEvent('vault:changed', { rescan: true, changes: [] });

    await waitFor(() => expect(view.state.doc.toString()).toBe('после сверки\n'));
  });
});

describe('С3.6: открытая заметка переименована или перемещена снаружи', () => {
  it('остаётся открытой под новым именем; ввод пишется в новый файл; «назад» ведёт по новому пути', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n', 'План.md': 'план' });
    const { calls, model, view } = await openWindow(disk, ['План.md']);
    model.open(['Встреча.md']);
    await waitFor(() => expect(headerPath()).toBe('Встреча'));
    const meeting = await editorView();
    type(meeting, 'ввод\n');

    disk.rename('Встреча.md', 'Архив/Встреча 2026.md');
    await emitCoreEvent('vault:changed', {
      rescan: false,
      changes: [{ kind: 'renamed', from: ['Встреча.md'], to: ['Архив', 'Встреча 2026.md'] }],
    });
    await emitCoreEvent('note:renamed', { from: ['Встреча.md'], to: ['Архив', 'Встреча 2026.md'] });

    await waitFor(() => expect(headerPath()).toBe('Архив/Встреча 2026'));
    expect(model.currentPath).toEqual(['Архив', 'Встреча 2026.md']);
    expect((await editorView()).state.doc.toString()).toBe('начало\nввод\n');
    await waitFor(() => expect(disk.texts.get('Архив/Встреча 2026.md')).toBe('начало\nввод\n'), { timeout: 3000 });
    expect(writes(calls).every(([, args]) => key((args as { request: { path: VaultPath } }).request.path) !== 'Встреча.md')).toBe(true);
    expect(disk.texts.has('Встреча.md')).toBe(false);
    void view;

    // История: «назад» — к «Плану», «вперёд» — к заметке под новым именем.
    model.back();
    await waitFor(() => expect(headerPath()).toBe('План'));
    model.forward();
    await waitFor(() => expect(headerPath()).toBe('Архив/Встреча 2026'));
  });

  it('событие пришло дважды (и ответом команды, и событием) — применяется один раз', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { model } = await openWindow(disk, ['Встреча.md']);

    disk.rename('Встреча.md', 'Итоги.md');
    await emitCoreEvent('note:renamed', { from: ['Встреча.md'], to: ['Итоги.md'] });
    await emitCoreEvent('note:renamed', { from: ['Встреча.md'], to: ['Итоги.md'] });

    await waitFor(() => expect(model.currentPath).toEqual(['Итоги.md']));
    expect(screen.queryByText(/удалена на диске/)).toBeNull();
  });
});

describe('С3.6: открытая заметка удалена снаружи', () => {
  it('закрывается с уведомлением; несохранённый ввод не пишется и файл не появляется снова', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { calls, model, view } = await openWindow(disk, ['Встреча.md']);
    type(view, 'ввод');

    disk.remove('Встреча.md');
    await emitCoreEvent('vault:changed', { rescan: false, changes: [{ kind: 'removed', path: ['Встреча.md'] }] });
    await emitCoreEvent('note:deleted', { path: ['Встреча.md'] });

    expect(await screen.findByText('Заметка «Встреча» удалена на диске.')).toBeTruthy();
    expect(model.currentPath).toBeNull();
    expect(screen.getByText('Нет открытой заметки')).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 1200));
    expect(writes(calls)).toEqual([]);
    expect(disk.texts.has('Встреча.md')).toBe(false);
  });

  it('решение 90: переименована снаружи в не-заметку (.md → .txt) — закрыта, уведомление с новым именем файла, ввод не пишется', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const { calls, model, view } = await openWindow(disk, ['Встреча.md']);
    type(view, 'ввод');

    disk.rename('Встреча.md', 'Встреча.txt');
    await emitCoreEvent('vault:changed', {
      rescan: false,
      changes: [{ kind: 'renamed', from: ['Встреча.md'], to: ['Встреча.txt'] }],
    });
    await emitCoreEvent('note:deleted', { path: ['Встреча.md'] });

    expect(await screen.findByText('Заметка «Встреча» больше не заметка: файл переименован в «Встреча.txt».')).toBeTruthy();
    expect(screen.queryByText(/удалена на диске/)).toBeNull();
    expect(model.currentPath).toBeNull();
    await new Promise((resolve) => setTimeout(resolve, 1200));
    expect(writes(calls)).toEqual([]);
  });

  it('удалена другая (не открытая) заметка — без уведомления, открытая не трогается', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n', 'План.md': 'план' });
    const { model } = await openWindow(disk, ['Встреча.md']);

    disk.remove('План.md');
    await emitCoreEvent('note:deleted', { path: ['План.md'] });
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(model.currentPath).toEqual(['Встреча.md']);
    expect(screen.queryByText(/удалена на диске/)).toBeNull();
  });

  it('своё удаление в дереве, событие ядра пришло раньше ответа команды — без «удалена на диске»', async () => {
    const disk = new Disk({ 'Встреча.md': 'начало\n' });
    const trash = disk.tree.mocks().entry_trash;
    const { model } = await openWindow(disk, ['Встреча.md'], {
      entry_trash: async (request) => {
        const result = trash ? await trash(request) : { outcome: 'trashed' as const };
        await emitCoreEvent('note:deleted', { path: request.path });
        return result;
      },
    });
    const row = await screen.findByRole('treeitem', { name: 'Встреча' });
    row.click();
    row.closest('[role="tree"]')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }));

    await waitFor(() => expect(model.currentPath).toBeNull());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByText(/удалена на диске/)).toBeNull();
  });
});
