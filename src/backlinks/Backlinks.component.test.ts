// Вид «Обратные ссылки» (0.2-2.2, С2-3.2, макет П-А и П-3) на подмене индекса
// (src/api/testing.ts, `indexMocks`): заголовок с числом, группы по заметкам,
// строка с выделенной ссылкой — только текст, «Нет обратных ссылок»,
// «Индекс строится… N %», перезапрос по `index:changed`.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { BacklinkLine, BacklinkNote, Backlinks as BacklinksData, IndexStatus, VaultPath } from '../api/index.ts';
import { emitCoreEvent, indexMocks, mockCore, type IndexFake, type RecordedCall } from '../api/testing.ts';
import { RightPanelModel } from '../sidebar-right/panel.svelte.ts';
import { testPanelHost } from '../sidebar-right/testing.svelte.ts';
import Backlinks from './Backlinks.svelte';
import type { LinkPosition } from './view.svelte.ts';

const TARGET: VaultPath = ['Проекты', 'Альфа', 'Встреча по запуску.md'];
const LINK = '[[Встреча по запуску]]';

/** Строка обратной ссылки: положение ссылки в `snippet` считается по тексту. */
function line(n: number, snippet: string, link = LINK, column?: number): BacklinkLine {
  const from = snippet.indexOf(link);
  return { line: n, column_utf16: column ?? from, snippet, from_utf16: from, to_utf16: from + link.length };
}

function note(from: VaultPath, links: BacklinkLine[], total = links.length): BacklinkNote {
  return { from, name: (from.at(-1) ?? '').replace(/\.md$/, ''), links_total: total, links };
}

const INBOX_MEETING = note(['Входящие', 'Встреча.md'], [line(3, `Перенесли на после ${LINK} — сначала нужны сроки.`)]);
const ALPHA_MEETING = note(['Проекты', 'Альфа', 'встреча.md'], [line(7, `…итоги и решения — в ${LINK}, там же список.`)]);
const PLAN = note(
  ['План работ.md'],
  [
    line(2, `Срок пилота согласовали на ${LINK}.`),
    line(5, `<b>жирный</b> <img src=x onerror=alert(1)> ${LINK}`),
  ],
);

const DATA: BacklinksData = { notes_total: 3, truncated: false, notes: [INBOX_MEETING, ALPHA_MEETING, PLAN] };

function setup(fake: IndexFake = { backlinks: { [TARGET.join('/')]: DATA } }, path: VaultPath | null = TARGET) {
  const calls = mockCore(indexMocks(fake));
  const panel = new RightPanelModel(testPanelHost({ open: true }));
  const opened: { path: VaultPath; at: LinkPosition | null }[] = [];
  const result = render(Backlinks, { path, panel, onOpen: (p: VaultPath, at: LinkPosition | null) => opened.push({ path: p, at }) });
  return { calls, panel, opened, ...result };
}

const listCalls = (calls: RecordedCall[]) => calls.filter(([command]) => command === 'backlinks_list');
const groups = () => screen.getAllByRole('listitem').filter((item) => item.classList.contains('group'));

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('вид «Обратные ссылки»', () => {
  it('заголовок с числом заметок: вид ставит число в заголовок панели', async () => {
    const { panel, calls } = setup();
    await waitFor(() => expect(panel.counts.backlinks).toBe(3));
    expect(listCalls(calls).map(([, args]) => args)).toEqual([{ request: { path: TARGET } }]);
  });

  it('группы по заметкам в порядке ядра, папка серым при повторе имени', async () => {
    setup();
    await waitFor(() => expect(groups()).toHaveLength(3));
    const heads = groups().map((group) => {
      const name = within(group).getByRole('button', { name: /^Открыть/ });
      return [name.querySelector('.name')?.textContent, name.querySelector('.folder')?.textContent ?? null];
    });
    expect(heads).toEqual([
      ['Встреча', 'Входящие'],
      // Имя повторяется без учёта регистра — у обеих видна папка.
      ['встреча', 'Проекты / Альфа'],
      ['План работ', null],
    ]);
    // Справа — число ссылок в заметке.
    const [, , plan] = groups() as [HTMLElement, HTMLElement, HTMLElement];
    expect(plan.querySelector('.links-total')?.textContent).toBe('2');
  });

  it('строка с выделенной ссылкой — текст, не HTML', async () => {
    const { container } = setup();
    await waitFor(() => expect(groups()).toHaveLength(3));
    const lines = [...container.querySelectorAll('button.line')];
    expect(lines.map((el) => el.querySelector('mark')?.textContent)).toEqual([LINK, LINK, LINK, LINK]);
    expect(lines[0]?.textContent).toBe(`Перенесли на после ${LINK} — сначала нужны сроки.`);

    const html = lines[3] as HTMLElement;
    expect(html.textContent).toBe(`<b>жирный</b> <img src=x onerror=alert(1)> ${LINK}`);
    expect(html.querySelector('b, img')).toBeNull();
    expect(container.querySelector('img, b')).toBeNull();
  });

  it('Ctrl+щелчок — так же: строка ведёт к ссылке, имя — к заметке сверху', async () => {
    const { container, opened } = setup();
    await waitFor(() => expect(groups()).toHaveLength(3));
    const lines = [...container.querySelectorAll('button.line')] as HTMLElement[];

    await fireEvent.click(lines[2] as HTMLElement);
    await fireEvent.click(lines[1] as HTMLElement, { ctrlKey: true });
    await fireEvent.click(screen.getByRole('button', { name: 'Открыть «План работ»' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Открыть «План работ»' }), { ctrlKey: true });

    expect(opened).toEqual([
      { path: PLAN.from, at: { line: 2, column: PLAN.links[0]?.column_utf16 } },
      { path: ALPHA_MEETING.from, at: { line: 7, column: ALPHA_MEETING.links[0]?.column_utf16 } },
      { path: PLAN.from, at: null },
      { path: PLAN.from, at: null },
    ]);
  });

  it('группу можно свернуть шевроном', async () => {
    const { container } = setup();
    await waitFor(() => expect(groups()).toHaveLength(3));
    const toggle = screen.getAllByRole('button', { name: 'Свернуть' })[0] as HTMLElement;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');

    await fireEvent.click(toggle);

    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.getAttribute('aria-label')).toBe('Развернуть');
    expect(container.querySelectorAll('button.line')).toHaveLength(3);
  });

  it('Нет обратных ссылок: у заметки без ссылок и без открытой заметки, число 0', async () => {
    const { panel } = setup({ backlinks: {} });
    expect(await screen.findByText('Нет обратных ссылок')).toBeTruthy();
    expect(panel.counts.backlinks).toBe(0);
    cleanup();
    clearMocks();

    const none = setup(undefined, null);
    expect(await screen.findByText('Нет обратных ссылок')).toBeTruthy();
    expect(none.panel.counts.backlinks).toBe(0);
    expect(listCalls(none.calls)).toEqual([]);
  });

  it('Индекс строится… 37 %: без числа в заголовке, список появляется сам по index:status ready', async () => {
    const status: IndexStatus = { state: 'building', done: 37, total: 100 };
    const fake: IndexFake = { status, backlinks: { [TARGET.join('/')]: DATA } };
    const { panel } = setup(fake);

    expect(await screen.findByText('Индекс строится… 37 %')).toBeTruthy();
    const bar = screen.getByRole('progressbar', { name: 'Индекс строится…' });
    expect(bar.getAttribute('aria-valuenow')).toBe('37');
    expect(panel.counts.backlinks).toBeNull();

    // Ход сборки — событием index:status.
    await emitCoreEvent('index:status', { state: 'building', done: 61, total: 100 });
    expect(await screen.findByText('Индекс строится… 61 %')).toBeTruthy();

    fake.status = { state: 'ready', done: 100, total: 100 };
    Object.assign(status, fake.status);
    await emitCoreEvent('index:status', { state: 'ready', done: 100, total: 100 });
    await waitFor(() => expect(groups()).toHaveLength(3));
    expect(panel.counts.backlinks).toBe(3);
    expect(screen.queryByText(/Индекс строится/)).toBeNull();
  });

  it('Индекс строится без известного объёма — без процентов', async () => {
    setup({ status: { state: 'building', done: 0, total: 0 } });
    expect(await screen.findByText('Индекс строится…')).toBeTruthy();
  });

  it('перезапрос по index:changed', async () => {
    const backlinks: Record<string, BacklinksData> = {};
    const { calls, panel } = setup({ backlinks });
    expect(await screen.findByText('Нет обратных ссылок')).toBeTruthy();

    backlinks[TARGET.join('/')] = { notes_total: 1, truncated: false, notes: [PLAN] };
    await emitCoreEvent('index:changed', {});

    await waitFor(() => expect(groups()).toHaveLength(1));
    expect(panel.counts.backlinks).toBe(1);
    expect(listCalls(calls)).toHaveLength(2);
  });

  it('показано не всё (truncated) — строка об этом внизу, в заголовке — всего заметок', async () => {
    const { panel } = setup({ backlinks: { [TARGET.join('/')]: { notes_total: 1200, truncated: true, notes: [PLAN] } } });
    expect(await screen.findByText('Показаны не все обратные ссылки')).toBeTruthy();
    expect(panel.counts.backlinks).toBe(1200);
  });

  it('смена заметки — запрос по новому пути; старый ответ, пришедший позже, не показывается', async () => {
    let release: (() => void) | undefined;
    const other: VaultPath = ['Другая.md'];
    const calls = mockCore({
      ...indexMocks(),
      backlinks_list: ({ path }) =>
        path.join('/') === TARGET.join('/')
          ? new Promise((resolve) => {
              release = () => resolve({ status: 'ready', ...DATA });
            })
          : { status: 'ready', notes_total: 1, truncated: false, notes: [PLAN] },
    });
    const panel = new RightPanelModel(testPanelHost({ open: true }));
    const view = render(Backlinks, { path: TARGET, panel, onOpen: () => undefined });
    await waitFor(() => expect(release).toBeDefined());

    await view.rerender({ path: other });
    await waitFor(() => expect(groups()).toHaveLength(1));
    release?.();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(groups()).toHaveLength(1);
    expect(panel.counts.backlinks).toBe(1);
    expect(listCalls(calls)).toHaveLength(2);
  });

  it('ошибка ядра — сообщение в виде', async () => {
    mockCore({
      backlinks_list: () => {
        throw { code: 'io_failed', message_key: 'errors.io_failed' };
      },
    });
    const panel = new RightPanelModel(testPanelHost({ open: true }));
    render(Backlinks, { path: TARGET, panel, onOpen: () => undefined });
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(panel.counts.backlinks).toBeNull();
  });
});
