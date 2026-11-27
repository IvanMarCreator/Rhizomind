import { clearMocks } from '@tauri-apps/api/mocks';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import type { IndexReply, SearchResults } from '../api/index.ts';
import { emitCoreEvent, mockCore } from '../api/testing.ts';
import SearchDialog from './SearchDialog.svelte';

afterEach(clearMocks);

it('оставляет свежую выдачу и открывает строку с её столбцом', async () => {
  let old!: (reply: IndexReply<SearchResults>) => void;
  const search = vi.fn(({ text }: { text: string }): Promise<IndexReply<SearchResults>> => {
    if (text === 'старый') return new Promise((resolve) => { old = resolve; });
    return Promise.resolve({ status: 'ready', truncated: false, items: [
      { path: ['Работа', 'План.md'], location: { line: 4, column_utf16: 6, snippet: 'После проверки' } },
    ] });
  });
  mockCore({ vault_search: search });
  const onOpen = vi.fn();
  render(SearchDialog, { current: null, onOpen, onClose: vi.fn() });
  const field = screen.getByRole('searchbox');
  for (const input of [field, screen.getByLabelText('Папка'), screen.getByLabelText('Тег')]) {
    expect(input.getAttribute('spellcheck')).toBe('false');
    expect(input.getAttribute('autocomplete')).toBe('off');
  }
  await fireEvent.input(field, { target: { value: 'старый' } });
  await waitFor(() => expect(old).toBeTypeOf('function'));
  await fireEvent.input(field, { target: { value: 'проверка' } });
  await screen.findByText(/После проверки/u);
  old({ status: 'ready', truncated: false, items: [] });
  await fireEvent.keyDown(field, { key: 'Enter' });
  expect(onOpen).toHaveBeenCalledWith(['Работа', 'План.md'], { line: 4, column: 6 });
});

it('объединяет фильтры с упоминаниями и показывает ход индекса', async () => {
  const search = vi.fn(() => ({ status: 'index_building' as const, done: 3, total: 4 }));
  mockCore({ vault_search: search });
  render(SearchDialog, { current: ['План.md'], onOpen: vi.fn(), onClose: vi.fn() });
  await fireEvent.input(screen.getByLabelText('Папка'), { target: { value: 'Работа/Квартал' } });
  await fireEvent.input(screen.getByLabelText('Тег'), { target: { value: '#Проект' } });
  await fireEvent.click(screen.getByRole('checkbox', { name: /Упоминания без ссылки/u }));
  await waitFor(() => expect(search).toHaveBeenLastCalledWith({
    text: '', folder: ['Работа', 'Квартал'], tag: 'Проект', mentions_of: ['План.md'],
  }));
  expect((await screen.findByRole('status')).textContent).toMatch(/75\s*%/);
});

it('обновляет открытую выдачу после правки обычного текста', async () => {
  let snippet = 'План на вчера';
  mockCore({ vault_search: () => ({ status: 'ready', truncated: false, items: [
    { path: ['День.md'], location: { line: 1, column_utf16: 0, snippet } },
  ] }) });
  render(SearchDialog, { current: null, onOpen: vi.fn(), onClose: vi.fn() });
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'план' } });
  await screen.findByText(/План на вчера/u);
  snippet = 'План на сегодня';
  await emitCoreEvent('search:changed', {});
  await screen.findByText(/План на сегодня/u);
});
