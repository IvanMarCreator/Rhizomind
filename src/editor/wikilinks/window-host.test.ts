// Ждущий переход по ссылке (решение 86) у WindowWikiHost: срабатывает только
// при открытии той заметки, куда вела ссылка, и сбрасывается любым другим
// открытием — не срабатывает позже при случайном открытии той заметки.
import { describe, expect, it } from 'vitest';
import type { VaultPath } from '../../api/index.ts';
import type { WikiEvent } from './host.ts';
import type { FollowDeps } from './open.ts';
import { WindowWikiHost } from './window-host.ts';

const HOME: VaultPath = ['Дневник.md'];
const MEETING: VaultPath = ['Альфа', 'Встреча.md'];
const OTHER: VaultPath = ['Другая.md'];

function setup() {
  let current: VaultPath | null = HOME;
  const opened: VaultPath[] = [];
  const deps: FollowDeps = {
    resolve: async (_from, target) => (target === 'Встреча' ? MEETING : null),
    treeList: async () => [],
    folderCreate: async (dir, name) => [...dir, name],
    noteCreate: async (dir, name) => [...dir, name],
  };
  const host = new WindowWikiHost(
    { from: () => current, openNote: (path) => opened.push(path), notify: () => undefined },
    deps,
  );
  /** Окно показало заметку `path`: редактор подписывается на события. */
  const show = async (path: VaultPath) => {
    current = path;
    host.shown(path);
    const events: WikiEvent[] = [];
    const stop = host.subscribe((event) => events.push(event));
    await Promise.resolve();
    await Promise.resolve();
    stop();
    return events;
  };
  return { host, opened, show };
}

describe('ждущий переход по ссылке', () => {
  it('приходит, когда открылась заметка из ссылки', async () => {
    const { host, opened, show } = setup();
    await host.follow('Встреча#Итоги');
    expect(opened).toEqual([MEETING]);
    expect(await show(MEETING)).toEqual([{ kind: 'arrive', heading: 'Итоги' }]);
  });

  it('другое открытие до завершения перехода (Ctrl+O) сбрасывает его', async () => {
    const { host, show } = setup();
    await host.follow('Встреча#Итоги');
    expect(await show(OTHER)).toEqual([]);
    // Позже ту заметку открыли просто так — перехода нет.
    expect(await show(MEETING)).toEqual([]);
  });

  it('открытие другой заметки без подписки редактора (ошибка чтения) тоже сбрасывает', async () => {
    const { host, show } = setup();
    await host.follow('Встреча#Итоги');
    host.shown(OTHER);
    expect(await show(MEETING)).toEqual([]);
  });
});
