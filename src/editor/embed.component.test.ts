import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { previewInto } from './embeds/view.ts';
import { WindowModel } from '../shell/window.svelte.ts';
import { FakeVault } from '../tree/fakeVault.ts';

const picture = { kind: 'image', name: 'Схема.png', width: 1, height: 1, png: 'aW1hZ2U=' } as const;
function start(text = 'Введение\n![[Схема.png]]\n', preview: NonNullable<CoreMocks['embed_read']> = () => picture) {
  const files = new FakeVault([{ kind: 'note', path: ['План.md'] }, { kind: 'attachment', path: ['Схема.png'] }]);
  const calls = mockCore({
    ...files.mocks(),
    vault_current: () => ({ api_version: 1, vault: { id: 'v', name: 'База' } }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    note_read: () => ({ text, version: { hash: 'x', size: 32, modified_ms: 0 }, read_only: null }),
    embed_read: preview,
  });
  const model = new WindowModel();
  render(Shell, { model });
  return { model, calls };
}
afterEach(() => { cleanup(); clearMocks(); });

it('embed показывает изображение в живом предпросмотре заметки', async () => {
  const { model } = start();
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(['План.md']);
  const image = await screen.findByRole('img', { name: 'Схема.png' });
  expect(image.getAttribute('src')).toBe('data:image/png;base64,' + picture.png);
});

it('embed открывает то же изображение из дерева, Esc возвращает фокус в дерево', async () => {
  start();
  const row = await screen.findByText('Схема');
  await fireEvent.click(row);
  const dialog = await screen.findByRole('dialog', { name: 'Схема.png' });
  expect(dialog.querySelector('img')?.getAttribute('src')).toBe('data:image/png;base64,' + picture.png);
  await fireEvent.keyDown(dialog, { key: 'Escape' });
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  expect(document.activeElement).toBe(screen.getByRole('tree'));
});

it('embed оставляет циклы компактными и блоки кода буквальными, отказ не создаёт заметку', async () => {
  const { model,calls } = start('Введение\n![[План]]\n![[Нет]]\n```\n![[Код]]\n```', ({source}) =>
    source.kind === 'target' && source.target === 'План'
      ? { kind:'note',name:'План.md',path:['План.md'],text:'# Заголовок\n**Жирный** ![[План]] ![[Б]]' }
      : { kind:'unavailable',reason:'missing' });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(['План.md']);
  expect((await screen.findByText('Жирный')).classList.contains('cm-rz-strong')).toBe(true);
  const fragment = document.querySelector<HTMLElement>('.rz-embed-note');
  if (!fragment) throw new Error('Нет текста вложенной заметки');
  expect(within(fragment).getByRole('button',{name:'План'})).toBeTruthy();
  expect(within(fragment).getByRole('button',{name:'Б'})).toBeTruthy();
  expect(await screen.findByText('Нет — Файл не найден')).toBeTruthy();
  await fireEvent.click(screen.getByText('Нет — Файл не найден'));
  expect(calls.some(([command])=>command==='note_create')).toBe(false);
  const reads = calls.filter(([command])=>command==='embed_read');
  expect(reads).toHaveLength(2);
  expect(screen.getByText('![[Код]]')).toBeTruthy();
});

it('embed ограничивает суммарные пиксели видимых растров, открывает переполнение отдельно и освобождает бюджет', async () => {
  mockCore({embed_read: () => ({...picture,width:1024,height:1024})});
  const freed = document.createElement('span');
  const nodes = [...Array.from({length:8}, () => document.createElement('span')), freed];
  const opened: string[] = [];
  const source = {kind:'path',path:['Схема.png']} as const;
  const cleanups = nodes.map(node => previewInto(node,{...source,path:[...source.path]},() => opened.push('open'),() => {}));
  try {
    await waitFor(() => expect(nodes[8]?.querySelector('button')).toBeTruthy());
    expect(nodes.filter(node => node.querySelector('img'))).toHaveLength(8);
    nodes[8]?.querySelector('button')?.click();
    expect(opened).toEqual(['open']);
    cleanups[0]?.(); cleanups[8]?.();

    cleanups[8] = previewInto(freed,{kind:'path',path:['Схема.png']},()=>{},()=>{});
    await waitFor(() => expect(freed.querySelector('img')).toBeTruthy());
  } finally { for(const stop of cleanups) stop(); }
});
