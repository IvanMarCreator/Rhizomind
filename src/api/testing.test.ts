// Подмена команд 0.2 отвечает так же, как ядро (контракт — contracts/v1/).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMocks } from '@tauri-apps/api/mocks';
import { core } from './commands.ts';
import { TEST_RELINK_TOKEN, indexMocks, mockCore, relinkMocks } from './testing.ts';

// Подмена ядра из @tauri-apps/api/mocks работает через window.
beforeEach(() => {
  vi.stubGlobal('window', globalThis);
});

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});

describe('подмена индекса', () => {
  it('готовый индекс отдаёт данные, неизвестное — пустым списком', async () => {
    mockCore(
      indexMocks({
        backlinks: { 'Проекты/Встреча.md': { notes_total: 1, notes: [] } },
        tags: [{ tag: 'Проект', key: 'проект', count: 1 }],
        tagNotes: { проект: [['Встреча.md']] },
      }),
    );
    expect(await core.backlinksList({ path: ['Проекты', 'Встреча.md'] })).toEqual({
      status: 'ready',
      notes_total: 1,
      notes: [],
    });
    expect(await core.backlinksList({ path: ['Нет.md'] })).toEqual({ status: 'ready', notes_total: 0, notes: [] });
    expect(await core.tagsList()).toEqual({ status: 'ready', tags: [{ tag: 'Проект', key: 'проект', count: 1 }] });
    expect(await core.tagNotes({ key: 'проект' })).toEqual({ status: 'ready', notes_total: 1, notes: [['Встреча.md']] });
    expect(await core.tagNotes({ key: 'нет' })).toEqual({ status: 'ready', notes_total: 0, notes: [] });
  });

  it('пока индекс строится — index_building с ходом сборки', async () => {
    mockCore(indexMocks({ status: { state: 'building', done: 37, total: 100 } }));
    expect(await core.indexStatus()).toEqual({ state: 'building', done: 37, total: 100 });
    for (const reply of [await core.tagsList(), await core.backlinksList({ path: ['a.md'] })]) {
      expect(reply).toEqual({ status: 'index_building', done: 37, total: 100 });
    }
  });
});

describe('подмена обновления ссылок', () => {
  it('предпросмотр по умолчанию — переписывать нечего; ответы окна записаны', async () => {
    const calls = mockCore(relinkMocks());
    expect(await core.relinkPreview({ op: 'rename', path: ['a.md'], new_name: 'b.md' })).toEqual({
      status: 'ready',
      notes: 0,
      links: 0,
      token: TEST_RELINK_TOKEN,
    });
    await core.linksFlushDone({ token: TEST_RELINK_TOKEN, saved: true });
    await core.relinkAnswer({ token: TEST_RELINK_TOKEN, apply: false });
    expect(calls.map(([command]) => command)).toEqual(['relink_preview', 'links_flush_done', 'relink_answer']);
  });
});
