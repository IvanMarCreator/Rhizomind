import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, expect, it, vi } from 'vitest';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import AttachmentDialog from './AttachmentDialog.svelte';
import { captureAttachment } from './attachment.ts';

afterEach(() => { cleanup(); clearMocks(); });

function show(extra: CoreMocks = {}) {
  const anchor = { note: ['План.md'], insert: vi.fn(() => true), pasteText: vi.fn(), release: vi.fn() };
  const notify = vi.fn();
  const onClose = vi.fn();
  const calls = mockCore({ tree_list: () => ({ entries: [{ kind: 'attachment', path: ['Папка', 'Фото#1.png'] }] }),
    attachment_insert: () => ({ outcome: 'created', path: ['Копия.png'], target: 'Копия.png' }), ...extra });
  render(AttachmentDialog, { props: { anchor, notify, onClose } });
  return { anchor, calls, onClose, notify };
}

it('attachment — внешняя строка и отмена не вызывают запись или команду проверки пути', async () => {
  const { calls, onClose } = show();
  const filter = screen.getByLabelText('Файл в хранилище');
  await fireEvent.input(filter, { target: { value: 'C:/другое/план.png' } });
  expect(screen.getByText(/сначала скопируйте файл/i)).toBeTruthy();
  await fireEvent.keyDown(filter, { key: 'Escape' });
  expect(onClose).toHaveBeenCalledOnce();
  expect(calls.map(([command]) => command)).toEqual(['tree_list']);
});

it('attachment — непредставимое имя требует правки, при отказе диска ссылка не вставляется', async () => {
  const { calls, anchor } = show({ attachment_insert: () => { throw { code: 'io_failed', message_key: 'errors.io_failed' }; } });
  await fireEvent.click(await screen.findByRole('button', { name: 'Папка/Фото#1.png' }));
  const save = screen.getByRole('button', { name: 'Вставить вложение' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  await fireEvent.input(screen.getByLabelText('Имя копии'), { target: { value: 'Фото 1.png' } });
  await fireEvent.click(save);
  await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy());
  expect(calls.at(-1)).toEqual(['attachment_insert', { request: { note: ['План.md'], source: { kind: 'file', path: ['Папка', 'Фото#1.png'] }, name: 'Фото 1.png' } }]);
  expect(anchor.insert).not.toHaveBeenCalled();
});

it('attachment — файл из буфера с непредставимым именем можно переименовать до копирования', async () => {
  const { calls, anchor, onClose } = show({ attachment_insert: (request) => request.source.kind === 'clipboard'
    ? { outcome: 'choose_name', path: ['Папка', 'Фото#1.png'] }
    : { outcome: 'created', path: ['Фото 1.png'], target: 'Фото 1.png' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Вставить из буфера обмена' }));
  const input = await screen.findByLabelText('Имя копии');
  expect((input as HTMLInputElement).value).toBe('Фото#1.png');
  expect(anchor.insert).not.toHaveBeenCalled();
  await fireEvent.input(input, { target: { value: 'Фото 1.png' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Вставить вложение' }));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(calls.at(-1)?.[1]).toEqual({ request: { note: ['План.md'], source: { kind: 'file', path: ['Папка', 'Фото#1.png'] }, name: 'Фото 1.png' } });
  expect(anchor.insert).toHaveBeenCalledWith('![[Фото 1.png]]');
});

it('attachment — следит за выделением исходной заметки при правках и прекращает вставку после закрытия', () => {
  const host = document.body.appendChild(document.createElement('div'));
  const view = new EditorView({ parent: host, state: EditorState.create({ doc: 'abc', selection: { anchor: 1 } }) });
  try {
    const origin = captureAttachment(view, ['План.md']);
    if (!origin) throw new Error('нет исходного редактора');
    view.dispatch({ changes: { from: 0, insert: 'X' }, selection: { anchor: 0 } });
    expect(origin.insert('![[Фото.png]]')).toBe(true);
    expect(view.state.doc.toString()).toBe('Xa![[Фото.png]]bc');
    host.remove();
    expect(origin.insert('ошибочная ссылка')).toBe(false);
    origin.release();
  } finally { view.destroy(); host.remove(); }
});
