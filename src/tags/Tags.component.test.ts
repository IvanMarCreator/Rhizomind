// Вид «Теги» в правой панели (0.2-2.3, С2-3.3 «Список тегов», макет П-2):
// все теги хранилища с числом заметок, порядок «по имени / по числу»,
// раскрытие тега со списком заметок, перезапрос по `index:changed`,
// открытие с раскрытым тегом по вызову из редактора.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { TagSummary, VaultPath } from '../api/index.ts';
import { emitCoreEvent, indexMocks, mockCore, type CoreMocks, type IndexFake } from '../api/testing.ts';
import type { RightPanel as PanelState } from '../core/layout/layout.ts';
import { RightPanelModel } from '../sidebar-right/panel.svelte.ts';
import { testPanelHost } from '../sidebar-right/testing.svelte.ts';
import { TagsModel } from './model.svelte.ts';
import Tags from './Tags.svelte';

const tag = (name: string, count: number, key = name.toLowerCase()): TagSummary => ({ tag: name, key, count });

/** Подмена индекса, которая читает `fake` при каждом вызове: тест меняет данные и состояние на ходу. */
function liveIndex(fake: IndexFake): CoreMocks {
  const now = () => indexMocks(fake) as Required<Pick<CoreMocks, 'tags_list' | 'tag_notes'>>;
  return {
    tags_list: (request) => now().tags_list(request),
    tag_notes: (request) => now().tag_notes(request),
  };
}

function setup(fake: IndexFake, initial: Partial<PanelState> = {}, current: VaultPath | null = null) {
  // Подмена индекса живая: тест меняет `fake` и шлёт `index:changed`.
  const calls = mockCore(liveIndex(fake));
  const host = testPanelHost({ open: true, view: 'tags', ...initial });
  const panel = new RightPanelModel(host);
  const model = new TagsModel(panel);
  const opened: VaultPath[] = [];
  const view = render(Tags, { model, current, onOpen: (path: VaultPath) => opened.push(path) });
  return { calls, host, panel, model, opened, view };
}

/** Имена тегов в списке по порядку — как видит пользователь, с «#». */
function shownTags(): string[] {
  return [...document.querySelectorAll('.tag-row .name')].map((name) => name.textContent ?? '');
}

function tagButton(name: string): HTMLElement {
  return screen.getByRole('button', { name: new RegExp(`^#${name},`) });
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('вид «Теги»', () => {
  it('порядок Intl.Collator ru numeric', async () => {
    setup({
      tags: [
        tag('задача10', 1),
        tag('Яблоко', 2, 'яблоко'),
        tag('задача2', 1),
        tag('ИБ', 4, 'иб'),
        tag('2026-11-01', 1),
        tag('встреча', 11),
        tag('работа/отчёт', 3),
        tag('alpha', 5),
      ],
    });
    await screen.findByRole('button', { name: /^#встреча,/ });

    // Как дерево файлов: цифры по значению, кириллица перед латиницей (правило `ru`).
    expect(shownTags()).toEqual([
      '2026-11-01',
      'встреча',
      'задача2',
      'задача10',
      'ИБ',
      'работа/отчёт',
      'Яблоко',
      'alpha',
    ]);
    // Строка: «#» отдельно (бледнее), написание из ядра (Р18), число заметок.
    const row = tagButton('ИБ');
    expect(row.querySelector('.hash')?.textContent).toBe('#');
    expect(row.querySelector('.count')?.textContent).toBe('4');
  });

  it('число тегов — в заголовке вида (setCount)', async () => {
    const { panel } = setup({ tags: [tag('a', 1), tag('b', 2), tag('c', 3)] });
    await waitFor(() => expect(panel.counts.tags).toBe(3));
  });

  it('переключатель по имени / по числу, запоминается', async () => {
    const { host, panel } = setup({ tags: [tag('бета', 2), tag('альфа', 2), tag('гамма', 9), tag('дельта', 1)] });
    await screen.findByRole('button', { name: /^#альфа,/ });
    const byName = screen.getByRole('button', { name: 'По имени' });
    const byCount = screen.getByRole('button', { name: 'По числу заметок' });
    expect(byName.getAttribute('aria-pressed')).toBe('true');
    expect(byCount.getAttribute('aria-pressed')).toBe('false');

    await fireEvent.click(byCount);
    // По числу — больше сверху; при равном числе — по имени.
    expect(shownTags()).toEqual(['гамма', 'альфа', 'бета', 'дельта']);
    expect(byCount.getAttribute('aria-pressed')).toBe('true');
    // Запоминается вместе с панелью — через её состояние, с записью раскладки.
    expect(panel.tagOrder).toBe('count');
    expect(host.persisted.at(-1)?.tagOrder).toBe('count');

    await fireEvent.click(byName);
    expect(shownTags()).toEqual(['альфа', 'бета', 'гамма', 'дельта']);
  });

  it('порядок из сохранённого состояния панели', async () => {
    setup({ tags: [tag('а', 1), tag('б', 5)] }, { tagOrder: 'count' });
    await screen.findByRole('button', { name: /^#б,/ });
    expect(shownTags()).toEqual(['б', 'а']);
  });

  it('щелчок раскрывает заметки тега', async () => {
    const { calls } = setup({
      tags: [tag('пилот', 6), tag('идея', 1)],
      tagNotes: {
        пилот: [
          ['Проекты', 'Альфа', 'Встреча.md'],
          ['Начало.md'],
          ['Входящие', 'Встреча.md'],
          ['Регламенты', 'Регламент доступа.md'],
          ['Проекты', 'Альфа', 'Встреча по запуску.md'],
          ['Проекты', 'План работ.md'],
        ],
      },
    });
    const button = await screen.findByRole('button', { name: /^#пилот,/ });
    expect(button.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const list = await screen.findByRole('list', { name: '#пилот' });
    const rows = within(list).getAllByRole('button');
    // По имени (естественный порядок), при повторе имени — папка серым.
    expect(rows.map((row) => row.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'Встреча Входящие',
      'Встреча Проекты / Альфа',
      'Встреча по запуску',
      'Начало',
      'План работ',
      'Регламент доступа',
    ]);
    expect(rows[0]?.querySelector('.folder')?.textContent).toBe('Входящие');
    expect(rows[2]?.querySelector('.folder')).toBeNull();
    expect(calls.filter(([command]) => command === 'tag_notes')).toEqual([['tag_notes', { request: { key: 'пилот' } }]]);

    // Повторный щелчок сворачивает.
    await fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('list', { name: '#пилот' })).toBeNull();
  });

  it('раскрыть → свернуть → раскрыть: запоздавший ответ первого раскрытия не затирает свежий', async () => {
    const pending: ((notes: VaultPath[]) => void)[] = [];
    mockCore({
      tags_list: () => ({ status: 'ready', tags: [tag('пилот', 1)] }),
      tag_notes: () =>
        new Promise((resolve) =>
          pending.push((notes) => resolve({ status: 'ready', notes_total: notes.length, notes })),
        ),
    });
    const model = new TagsModel(new RightPanelModel(testPanelHost({ open: true, view: 'tags' })));
    render(Tags, { model, current: null, onOpen: () => {} });
    const button = await screen.findByRole('button', { name: /^#пилот,/ });

    await fireEvent.click(button);
    await fireEvent.click(button);
    await fireEvent.click(button);
    await waitFor(() => expect(pending).toHaveLength(2));
    pending[1]?.([['Новая.md']]);
    await screen.findByRole('button', { name: /Новая/ });
    pending[0]?.([['Старая.md']]);
    await new Promise((resolve) => setTimeout(resolve, 20));

    const list = screen.getByRole('list', { name: '#пилот' });
    expect(within(list).getAllByRole('button').map((row) => row.textContent?.trim())).toEqual(['Новая']);
  });

  it('щелчок по заметке открывает её; открытая заметка выделена', async () => {
    const current = ['Проекты', 'План работ.md'];
    const { opened } = setup(
      { tags: [tag('пилот', 2)], tagNotes: { пилот: [['Начало.md'], ['Проекты', 'План работ.md']] } },
      {},
      current,
    );
    await fireEvent.click(await screen.findByRole('button', { name: /^#пилот,/ }));
    const list = await screen.findByRole('list', { name: '#пилот' });
    const [start, plan] = within(list).getAllByRole('button') as [HTMLElement, HTMLElement];
    expect(plan.getAttribute('aria-current')).toBe('page');
    expect(start.getAttribute('aria-current')).toBeNull();

    await fireEvent.click(start);
    expect(opened).toEqual([['Начало.md']]);
  });

  it('тег без заметок исчезает по index:changed', async () => {
    const fake: IndexFake = {
      tags: [tag('черновик', 1), tag('проект', 2)],
      tagNotes: { черновик: [['Набросок.md']], проект: [['А.md'], ['Б.md']] },
    };
    const { calls, panel } = setup(fake);
    await fireEvent.click(await screen.findByRole('button', { name: /^#черновик,/ }));
    await fireEvent.click(screen.getByRole('button', { name: /^#проект,/ }));
    await screen.findByRole('list', { name: '#проект' });

    // Из «Набросок» убрали тег: ядро больше не отдаёт его в списке.
    fake.tags = [tag('проект', 3)];
    fake.tagNotes = { проект: [['А.md'], ['Б.md'], ['Набросок.md']] };
    await emitCoreEvent('index:changed', {});

    await waitFor(() => expect(screen.queryByRole('button', { name: /^#черновик,/ })).toBeNull());
    expect(panel.counts.tags).toBe(1);
    // Раскрытый тег перезапрошен — заметок стало три.
    const list = await screen.findByRole('list', { name: '#проект' });
    await waitFor(() => expect(within(list).getAllByRole('button')).toHaveLength(3));
    expect(tagButton('проект').querySelector('.count')?.textContent).toBe('3');
    // Заметки исчезнувшего тега не запрашиваются.
    const notesCalls = calls.filter(([command]) => command === 'tag_notes').map(([, args]) => args);
    expect(notesCalls.filter((args) => JSON.stringify(args).includes('черновик'))).toHaveLength(1);
  });

  it('пустое хранилище: «Тегов нет»', async () => {
    const { panel } = setup({ tags: [] });
    expect(await screen.findByText('Тегов нет')).toBeTruthy();
    expect(panel.counts.tags).toBe(0);
  });

  it('индекс строится: «Индекс строится… 37 %», список появляется по index:status ready', async () => {
    const fake: IndexFake = { status: { state: 'building', done: 37, total: 100 }, tags: [tag('идея', 1)] };
    const { panel } = setup(fake);
    expect(await screen.findByText('Индекс строится… 37 %')).toBeTruthy();
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('37');
    expect(panel.counts.tags).toBeNull();

    await emitCoreEvent('index:status', { state: 'building', done: 50, total: 100 });
    expect(await screen.findByText('Индекс строится… 50 %')).toBeTruthy();

    fake.status = { state: 'ready', done: 100, total: 100 };
    await emitCoreEvent('index:status', fake.status);
    expect(await screen.findByRole('button', { name: /^#идея,/ })).toBeTruthy();
    expect(panel.counts.tags).toBe(1);
  });

  it('индекс строится, всего ещё неизвестно: «Индекс строится…» без процентов', async () => {
    setup({ status: { state: 'building', done: 0, total: 0 } });
    expect(await screen.findByText('Индекс строится…')).toBeTruthy();
    expect(screen.getByRole('progressbar').hasAttribute('aria-valuenow')).toBe(false);
  });

  it('открыть с раскрытым тегом (вызов из редактора)', async () => {
    const fake: IndexFake = {
      tags: [tag('Проект', 2, 'проект'), tag('идея', 1)],
      tagNotes: { проект: [['А.md'], ['Б.md']] },
    };
    // Панель закрыта; вид — «Обратные ссылки».
    mockCore(liveIndex(fake));
    const host = testPanelHost({ open: false, view: 'backlinks' });
    const panel = new RightPanelModel(host);
    const model = new TagsModel(panel);

    // Редактор (0.2-3.1) передаёт тег так, как он написан в тексте.
    model.reveal('ПРОЕКТ');
    expect(panel.open).toBe(true);
    expect(panel.view).toBe('tags');
    expect(host.persisted.at(-1)).toMatchObject({ open: true, view: 'tags' });

    // Вид появляется в панели позже (Shell рисует его по состоянию панели).
    render(Tags, { model, current: null, onOpen: () => {} });
    const button = await screen.findByRole('button', { name: /^#Проект,/ });
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const list = await screen.findByRole('list', { name: '#Проект' });
    await waitFor(() => expect(within(list).getAllByRole('button')).toHaveLength(2));

    // Повторный вызов не сворачивает тег.
    model.reveal('проект');
    expect(model.isExpanded('проект')).toBe(true);
  });
});
