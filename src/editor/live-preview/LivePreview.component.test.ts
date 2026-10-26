// Живой предпросмотр и режим исходного текста в окне (С3.3, решение 72) на
// подмене ядра: переключатель «книга / карандаш» и Ctrl+E, запоминание режима
// в настройках хранилища, чекбоксы и внешние ссылки.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { NoteReadResponse, Settings } from '../../api/index.ts';
import { mockCore, type CoreMocks, type RecordedCall } from '../../api/testing.ts';
import Shell from '../../shell/Shell.svelte';
import { WindowModel } from '../../shell/window.svelte.ts';

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
};

const NOTE = '# Заголовок\n\nтекст **жирный**\n\n- [ ] задача\n- [x] сделано\n\n[сайт](https://example.org)\n';
const VERSION = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };

interface Setup {
  settings?: Settings;
  readOnly?: boolean;
  extra?: CoreMocks;
}

async function openNote({ settings = SETTINGS, readOnly = false, extra = {} }: Setup = {}) {
  const disk = { text: NOTE, settings: { ...settings } };
  const calls = mockCore({
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...disk.settings } }),
    settings_write: ({ settings: next }) => {
      disk.settings = next;
      return null;
    },
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [] }),
    note_read: (): NoteReadResponse => ({
      text: disk.text,
      version: VERSION,
      read_only: readOnly ? { reason: 'invalid_utf8' } : null,
    }),
    note_write: ({ text }) => {
      disk.text = text;
      return { outcome: 'saved', version: VERSION };
    },
    open_external: () => ({ outcome: 'cancelled' }),
    ...extra,
  });
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(['Заметка.md']);
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return { calls, model, view, disk };
}

const called = (calls: RecordedCall[], command: string) => calls.filter(([name]) => name === command);

function pressCtrlE() {
  fireEvent.keyDown(window, { key: 'e', code: 'KeyE', ctrlKey: true });
}

/** Курсор в конец текста: строки с элементами — без курсора. */
function cursorToEnd(view: EditorView) {
  view.dispatch({ selection: { anchor: view.state.doc.length } });
}

afterEach(() => {
  cleanup();
  clearMocks();
  delete document.documentElement.dataset.theme;
});

describe('режим редактора', () => {
  it('заметка открывается в режиме из настроек хранилища: живой предпросмотр оформляет текст', async () => {
    const { view } = await openNote();
    expect(view.contentDOM.querySelector('.cm-rz-heading')).not.toBeNull();
    expect(screen.getByRole('radio', { name: /Живой предпросмотр/ }).getAttribute('aria-checked')).toBe('true');
  });

  it('режим исходного текста из настроек: разметка видна как есть', async () => {
    const { view } = await openNote({ settings: { ...SETTINGS, editor_mode: 'source' } });
    expect(view.contentDOM.querySelector('.cm-rz-heading')).toBeNull();
    expect(screen.getByRole('radio', { name: /Исходный текст/ }).getAttribute('aria-checked')).toBe('true');
  });

  it('кнопка «карандаш»: оформление снято, текст тот же байт в байт, выбор записан в настройки', async () => {
    const { view, calls, disk } = await openNote();
    cursorToEnd(view);
    const before = view.state.doc.toString();

    await fireEvent.click(screen.getByRole('radio', { name: /Исходный текст/ }));

    expect(view.contentDOM.querySelector('.cm-rz-heading')).toBeNull();
    expect(view.contentDOM.textContent).toContain('# Заголовок');
    expect(view.state.doc.toString()).toBe(before);
    await waitFor(() => expect(disk.settings.editor_mode).toBe('source'));
    expect(called(calls, 'note_write')).toEqual([]);
  });

  it('Ctrl+E переключает режим туда и обратно; текст не меняется', async () => {
    const { view, disk, calls } = await openNote();
    const before = view.state.doc.toString();

    pressCtrlE();
    await waitFor(() => expect(disk.settings.editor_mode).toBe('source'));
    expect(view.contentDOM.querySelector('.cm-rz-heading')).toBeNull();

    pressCtrlE();
    await waitFor(() => expect(disk.settings.editor_mode).toBe('live_preview'));
    expect(view.contentDOM.querySelector('.cm-rz-heading')).not.toBeNull();
    expect(view.state.doc.toString()).toBe(before);
    expect(called(calls, 'note_write')).toEqual([]);
  });

  it('запись режима не затирает другие настройки, изменённые после запуска окна', async () => {
    const { disk } = await openNote();
    disk.settings = { ...disk.settings, theme: 'dark', confirm_delete: false };

    pressCtrlE();

    await waitFor(() => expect(disk.settings).toEqual({ ...SETTINGS, theme: 'dark', confirm_delete: false, editor_mode: 'source' }));
  });

  it('переключатель — одна остановка Tab; ← и → выбирают режим', async () => {
    const { disk } = await openNote();
    const book = screen.getByRole('radio', { name: /Живой предпросмотр/ });
    const pencil = screen.getByRole('radio', { name: /Исходный текст/ });
    expect(book.tabIndex).toBe(0);
    expect(pencil.tabIndex).toBe(-1);

    book.focus();
    await fireEvent.keyDown(book, { key: 'ArrowRight' });
    await waitFor(() => expect(disk.settings.editor_mode).toBe('source'));
    expect(document.activeElement).toBe(pencil);
    expect(pencil.tabIndex).toBe(0);

    await fireEvent.keyDown(pencil, { key: 'ArrowLeft' });
    await waitFor(() => expect(disk.settings.editor_mode).toBe('live_preview'));
    expect(document.activeElement).toBe(book);
  });

  it('только чтение: режим переключается, правка по-прежнему запрещена', async () => {
    const { view } = await openNote({ readOnly: true });
    pressCtrlE();
    await waitFor(() => expect(view.contentDOM.querySelector('.cm-rz-heading')).toBeNull());
    expect(view.state.readOnly).toBe(true);
    pressCtrlE();
    await waitFor(() => expect(view.contentDOM.querySelector('.cm-rz-heading')).not.toBeNull());
    expect(view.state.readOnly).toBe(true);
    expect(screen.getByRole('radio', { name: /Исходный текст/ }).getAttribute('aria-label')).toBe('Исходный текст — только просмотр');
  });
});

describe('чекбоксы', () => {
  it('щелчок по чекбоксу меняет только `[ ]` на `[x]` и обратно', async () => {
    const { view, disk } = await openNote();
    cursorToEnd(view);

    const boxes = view.contentDOM.querySelectorAll('.cm-rz-checkbox');
    expect(boxes).toHaveLength(2);
    await fireEvent.mouseDown(boxes[0] as Element, { button: 0 });
    expect(view.state.doc.toString()).toBe(NOTE.replace('- [ ] задача', '- [x] задача'));

    await fireEvent.mouseDown(view.contentDOM.querySelectorAll('.cm-rz-checkbox')[1] as Element, { button: 0 });
    expect(view.state.doc.toString()).toBe(NOTE.replace('- [ ] задача', '- [x] задача').replace('- [x] сделано', '- [ ] сделано'));

    window.dispatchEvent(new Event('pagehide'));
    await waitFor(() => expect(disk.text).toBe(view.state.doc.toString()));
  });

  it('только чтение: щелчок по чекбоксу ничего не меняет', async () => {
    const { view, calls } = await openNote({ readOnly: true });
    cursorToEnd(view);
    await fireEvent.mouseDown(view.contentDOM.querySelector('.cm-rz-checkbox') as Element, { button: 0 });
    expect(view.state.doc.toString()).toBe(NOTE);
    expect(called(calls, 'note_write')).toEqual([]);
  });
});

describe('внешние ссылки', () => {
  it('щелчок по ссылке — команда ядра open_external (подтверждение в ядре), без перехода в окне', async () => {
    const { view, calls } = await openNote();
    cursorToEnd(view);
    const link = view.contentDOM.querySelector('[data-rz-url]');
    expect(link?.tagName).not.toBe('A');

    await fireEvent.mouseDown(link as Element, { button: 0 });

    await waitFor(() => expect(called(calls, 'open_external')).toEqual([['open_external', { request: { url: 'https://example.org' } }]]));
  });

  it('на строке с курсором обычный щелчок не открывает ссылку, Ctrl+щелчок — открывает', async () => {
    const { view, calls } = await openNote();
    view.dispatch({ selection: { anchor: view.state.doc.length - 2 } });
    const link = view.contentDOM.querySelector('[data-rz-url]') as Element;

    await fireEvent.mouseDown(link, { button: 0 });
    expect(called(calls, 'open_external')).toEqual([]);

    await fireEvent.mouseDown(link, { button: 0, ctrlKey: true });
    await waitFor(() => expect(called(calls, 'open_external')).toHaveLength(1));
  });

  it('адрес не http, https или mailto — щелчок не уходит в ядро', async () => {
    const { view, calls } = await openNote();
    view.dispatch({ changes: { from: 0, insert: '[файл](file:///C:/x)\n' } });
    cursorToEnd(view);
    await fireEvent.mouseDown(view.contentDOM.querySelector('[data-rz-url^="file"]') as Element, { button: 0 });
    expect(called(calls, 'open_external')).toEqual([]);
  });
});
