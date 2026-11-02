// Правая боковая панель (0.2-2.1, С2-3.1, макет П-А): переключение видов
// значками, заголовок вида с числом, ширина перетаскиванием и с клавиатуры.
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { RIGHT_PANEL_WIDTH, type RightPanel as PanelState } from '../core/layout/layout.ts';
import { RightPanelModel } from './panel.svelte.ts';
import RightPanel from './RightPanel.svelte';
import { testPanelHost } from './testing.svelte.ts';

function model(initial: Partial<PanelState> = {}) {
  const host = testPanelHost({ open: true, ...initial });
  return {
    panel: new RightPanelModel(host),
    get persisted() {
      return host.persisted;
    },
  };
}

afterEach(() => cleanup());

describe('правая панель', () => {
  it('значки видов вверху, под ними название вида и число', () => {
    const { panel } = model();
    panel.setCount('backlinks', 4);
    render(RightPanel, { panel, shortcuts: { backlinks: 'Ctrl+Shift+B', tags: null } });

    const aside = screen.getByRole('complementary', { name: 'Правая панель' });
    const tabs = within(aside).getAllByRole('tab');
    expect(tabs.map((tab) => [tab.getAttribute('aria-label'), tab.getAttribute('aria-selected')])).toEqual([
      ['Обратные ссылки', 'true'],
      ['Теги', 'false'],
    ]);
    // Подсказка при наведении — название вида и сочетание, если назначено.
    expect(tabs.map((tab) => tab.getAttribute('title'))).toEqual(['Обратные ссылки (Ctrl+Shift+B)', 'Теги']);
    const heading = within(aside).getByRole('heading', { level: 2 });
    expect(heading.textContent?.replace(/\s+/g, ' ').trim()).toBe('Обратные ссылки 4');
    expect(within(aside).getByRole('tabpanel', { name: 'Обратные ссылки' })).toBeTruthy();
  });

  it('переключение видов значками и стрелками; число не показывается, пока вид его не дал', async () => {
    const m = model();
    const { panel } = m;
    render(RightPanel, { panel });
    const [backlinks, tags] = screen.getAllByRole('tab') as [HTMLElement, HTMLElement];

    await fireEvent.click(tags);
    expect(tags.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByRole('heading', { level: 2 }).textContent?.trim()).toBe('Теги');
    expect(screen.getByRole('tabpanel', { name: 'Теги' })).toBeTruthy();
    expect(m.persisted.at(-1)?.view).toBe('tags');

    // Одна остановка Tab: выбранный значок в порядке обхода, ← и → выбирают вид.
    expect([backlinks.tabIndex, tags.tabIndex]).toEqual([-1, 0]);
    await fireEvent.keyDown(tags, { key: 'ArrowLeft' });
    expect(panel.view).toBe('backlinks');
    expect(document.activeElement).toBe(backlinks);
  });

  it('место подключения видов: содержимое вида — снаружи', () => {
    const { panel } = model({ view: 'tags' });
    render(RightPanel, { panel });

    expect(screen.getByRole('tabpanel', { name: 'Теги' }).dataset.view).toBe('tags');
  });

  it('ширина перетаскиванием левой границы в пределах, запись по отпускании', async () => {
    const m = model();
    const { panel } = m;
    render(RightPanel, { panel });
    const aside = screen.getByRole('complementary', { name: 'Правая панель' });
    const handle = screen.getByRole('separator', { name: 'Ширина правой панели' });
    expect(aside.style.width).toBe('300px');
    expect(handle.getAttribute('aria-valuenow')).toBe('300');
    expect(handle.getAttribute('aria-valuemin')).toBe(String(RIGHT_PANEL_WIDTH.min));
    expect(handle.getAttribute('aria-valuemax')).toBe(String(RIGHT_PANEL_WIDTH.max));

    await fireEvent.pointerDown(handle, { clientX: 1000, button: 0, pointerId: 1 });
    await fireEvent.pointerMove(window, { clientX: 900, pointerId: 1 });
    expect(aside.style.width).toBe('400px');
    await fireEvent.pointerMove(window, { clientX: 0, pointerId: 1 });
    expect(aside.style.width).toBe(`${RIGHT_PANEL_WIDTH.max}px`);
    expect(m.persisted).toEqual([]);
    await fireEvent.pointerUp(window, { clientX: 0, pointerId: 1 });

    expect(m.persisted.at(-1)?.width).toBe(RIGHT_PANEL_WIDTH.max);
    // После отпускания движение мыши ширину не меняет.
    await fireEvent.pointerMove(window, { clientX: 900, pointerId: 1 });
    expect(aside.style.width).toBe(`${RIGHT_PANEL_WIDTH.max}px`);
  });

  it('ширина с клавиатуры: граница в порядке Tab, ← шире, → уже', async () => {
    const { panel } = model();
    render(RightPanel, { panel });
    const handle = screen.getByRole('separator', { name: 'Ширина правой панели' });
    expect(handle.tabIndex).toBe(0);
    expect(handle.getAttribute('aria-orientation')).toBe('vertical');

    await fireEvent.keyDown(handle, { key: 'ArrowLeft' });
    expect(panel.width).toBe(310);
    await fireEvent.keyDown(handle, { key: 'ArrowRight' });
    await fireEvent.keyDown(handle, { key: 'ArrowRight' });
    expect(panel.width).toBe(290);
  });

  it('ширина с клавиатуры: Home — самая узкая, End — самая широкая', async () => {
    const m = model();
    const { panel } = m;
    render(RightPanel, { panel });
    const handle = screen.getByRole('separator', { name: 'Ширина правой панели' });

    await fireEvent.keyDown(handle, { key: 'Home' });
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.min);
    await fireEvent.keyDown(handle, { key: 'End' });
    expect(panel.width).toBe(RIGHT_PANEL_WIDTH.max);
    expect(m.persisted.map((state) => state.width)).toEqual([RIGHT_PANEL_WIDTH.min, RIGHT_PANEL_WIDTH.max]);
  });

  it('действия заголовка — справа от названия вида, для текущего вида', async () => {
    const { panel } = model({ view: 'tags' });
    const headerActions = createRawSnippet((view: () => string) => ({
      render: () => `<button type="button">действие ${view()}</button>`,
    }));
    render(RightPanel, { panel, headerActions });

    const row = screen.getByRole('heading', { level: 2 }).parentElement as HTMLElement;
    expect(within(row).getByRole('button', { name: 'действие tags' })).toBeTruthy();
    // Название вида и число — без кнопок внутри заголовка.
    expect(screen.getByRole('heading', { level: 2 }).textContent?.trim()).toBe('Теги');
  });
});
