import { describe, expect, it } from 'vitest';
import { LIMITS } from '../../api/limits.ts';
import {
  LAYOUT_VERSION,
  LayoutTooLarge,
  activePane,
  defaultLayout,
  openNote,
  parseLayout,
  serializeLayout,
  type Layout,
} from './layout.ts';

describe('модель раскладки v1', () => {
  it('по умолчанию — одна пустая панель', () => {
    const layout = defaultLayout();

    expect(layout.version).toBe(1);
    expect(layout.root).toEqual({ type: 'pane', id: 'pane-1', tabs: [], active: -1 });
    expect(activePane(layout).tabs).toEqual([]);
  });

  it('открытие заметки ставит её вкладкой активной панели; в 0.1 вкладка одна', () => {
    let layout = openNote(defaultLayout(), ['Проекты', 'Встреча.md']);
    layout = openNote(layout, ['План.md']);

    expect(activePane(layout)).toEqual({
      type: 'pane',
      id: 'pane-1',
      tabs: [{ type: 'note', path: ['План.md'] }],
      active: 0,
    });
  });

  it('открытие не меняет исходную раскладку', () => {
    const before = defaultLayout();
    openNote(before, ['А.md']);

    expect(before.root).toEqual({ type: 'pane', id: 'pane-1', tabs: [], active: -1 });
  });

  it('сохраняется в JSON с версией формата и читается обратно', () => {
    const layout = openNote(defaultLayout(), ['Проекты', 'Встреча.md']);

    const saved = serializeLayout(layout);

    expect(saved).toEqual({
      format: 'rhizomind-layout',
      version: LAYOUT_VERSION,
      root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path: ['Проекты', 'Встреча.md'] }], active: 0 },
      active_pane: 'pane-1',
    });
    const reread = parseLayout(JSON.parse(JSON.stringify(saved)));
    expect(reread).toEqual({ layout, recovered: false });
  });

  it('читает дерево разбиений: активная — панель из active_pane', () => {
    const saved = {
      format: 'rhizomind-layout',
      version: 1,
      root: {
        type: 'split',
        direction: 'row',
        sizes: [0.5, 0.5],
        children: [
          { type: 'pane', id: 'pane-1', tabs: [], active: -1 },
          { type: 'pane', id: 'pane-2', tabs: [{ type: 'note', path: ['Б.md'] }], active: 0 },
        ],
      },
      active_pane: 'pane-2',
    };

    const { layout, recovered } = parseLayout(saved);

    expect(recovered).toBe(false);
    expect(activePane(layout).id).toBe('pane-2');
  });

  it('пустая раскладка (новое хранилище) — по умолчанию, без пометки о сбое', () => {
    expect(parseLayout(null)).toEqual({ layout: defaultLayout(), recovered: false });
  });

  it.each([
    ['не объект', 'строка'],
    ['чужой формат', { format: 'dockview', version: 1 }],
    ['будущая версия', { format: 'rhizomind-layout', version: 2, root: {}, active_pane: 'pane-1' }],
    ['без корня', { format: 'rhizomind-layout', version: 1, active_pane: 'pane-1' }],
    [
      'путь не списком',
      {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'p', tabs: [{ type: 'note', path: 'А.md' }], active: 0 },
        active_pane: 'p',
      },
    ],
    [
      'компонент пути «..»',
      {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'p', tabs: [{ type: 'note', path: ['..', 'А.md'] }], active: 0 },
        active_pane: 'p',
      },
    ],
    [
      'активная вкладка за пределами',
      {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'p', tabs: [], active: 3 },
        active_pane: 'p',
      },
    ],
    [
      'нет активной панели',
      {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'p', tabs: [], active: -1 },
        active_pane: 'другая',
      },
    ],
    [
      'размеры не совпадают с числом частей',
      {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'split', direction: 'row', sizes: [1], children: [] },
        active_pane: 'p',
      },
    ],
  ])('повреждённая раскладка (%s) заменяется раскладкой по умолчанию', (_case, saved) => {
    expect(parseLayout(saved)).toEqual({ layout: defaultLayout(), recovered: true });
  });

  it('слишком глубокое дерево разбиений не читается', () => {
    let root: unknown = { type: 'pane', id: 'p', tabs: [], active: -1 };
    for (let i = 0; i < 100; i++) root = { type: 'split', direction: 'row', sizes: [1], children: [root] };

    expect(parseLayout({ format: 'rhizomind-layout', version: 1, root, active_pane: 'p' }).recovered).toBe(true);
  });

  it('раскладка больше предела ядра (1 МБ) не отправляется', () => {
    const name = 'Ж'.repeat(200);
    const tabs = Array.from({ length: 3000 }, (_, i) => ({ type: 'note' as const, path: [name, `${i}.md`] }));
    const layout: Layout = {
      version: 1,
      root: { type: 'pane', id: 'pane-1', tabs, active: 0 },
      activePane: 'pane-1',
    };

    expect(LIMITS.max_workspace_bytes).toBe(1024 * 1024);
    expect(() => serializeLayout(layout)).toThrow(LayoutTooLarge);
  });
});
