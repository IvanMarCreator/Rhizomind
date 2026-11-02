// Состояние правой боковой панели (0.2-2.1, С2-3.1). Файл компонентный: класс
// на рунах Svelte собирается только в проекте components.
import { describe, expect, it } from 'vitest';
import { RIGHT_PANEL_WIDTH, defaultRightPanel, type RightPanel } from '../core/layout/layout.ts';
import { RightPanelModel } from './panel.svelte.ts';
import { testPanelHost } from './testing.svelte.ts';

function host(initial: RightPanel = defaultRightPanel()) {
  return testPanelHost(initial);
}

describe('состояние правой панели', () => {
  it('кнопка открывает и закрывает панель; каждое изменение записывается', () => {
    const h = host();
    const panel = new RightPanelModel(h);

    panel.toggle();
    expect(panel.open).toBe(true);
    panel.toggle();
    expect(panel.open).toBe(false);
    expect(h.writes.map((w) => [w.next.open, w.persist])).toEqual([
      [true, true],
      [false, true],
    ]);
  });

  it('команда вида открывает панель на этом виде; повтор при открытом виде закрывает', () => {
    const h = host();
    const panel = new RightPanelModel(h);

    panel.showView('tags');
    expect([panel.open, panel.view]).toEqual([true, 'tags']);
    panel.showView('backlinks');
    expect([panel.open, panel.view]).toEqual([true, 'backlinks']);
    panel.showView('backlinks');
    expect([panel.open, panel.view]).toEqual([false, 'backlinks']);
  });

  it('выбор вида значком не закрывает панель', () => {
    const panel = new RightPanelModel(host({ ...defaultRightPanel(), open: true }));

    panel.selectView('tags');
    panel.selectView('tags');

    expect([panel.open, panel.view]).toEqual([true, 'tags']);
  });

  it('ширина перетаскиванием в пределах: левая граница влево — шире; запись — по отпускании', () => {
    const h = host({ ...defaultRightPanel(), open: true });
    const panel = new RightPanelModel(h);

    panel.beginResize(1000);
    panel.resizeTo(950);
    expect(panel.width).toBe(350);
    panel.resizeTo(-5000);
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.max);
    panel.resizeTo(5000);
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.min);
    expect(h.writes.every((w) => !w.persist)).toBe(true);
    panel.resizeTo(980);
    panel.endResize();

    expect(panel.width).toBe(320);
    expect(h.writes.at(-1)).toEqual({ next: { ...h.state, width: 320 }, persist: true });
  });

  it('ширина с клавиатуры — шагом, в пределах, с записью', () => {
    const h = host({ ...defaultRightPanel(), open: true, width: RIGHT_PANEL_WIDTH.max - 5 });
    const panel = new RightPanelModel(h);

    panel.resizeBy(20);
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.max);
    panel.resizeBy(-20);
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.max - 20);
    expect(h.writes.every((w) => w.persist)).toBe(true);
  });

  it('порядок тегов запоминается; число в заголовке вида ставит сам вид', () => {
    const h = host();
    const panel = new RightPanelModel(h);

    panel.setTagOrder('count');
    panel.setCount('backlinks', 4);

    expect(h.state.tagOrder).toBe('count');
    expect(panel.counts).toEqual({ backlinks: 4, tags: null });
  });
});
