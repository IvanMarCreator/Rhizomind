import { afterEach, describe, expect, it, vi } from 'vitest';
import { blockExternalDrops } from './externalDrop.ts';

/** Событие перетаскивания (в jsdom нет DragEvent и DataTransfer). */
function drag(
  type: string,
  target: EventTarget,
  types: string[] = ['text/plain'],
): Event & { dataTransfer: { dropEffect: string; types: string[] } } {
  const event = Object.assign(new Event(type, { bubbles: true, cancelable: true }), {
    dataTransfer: { dropEffect: 'copy', types },
  });
  target.dispatchEvent(event);
  return event;
}

let stop: (() => void) | null = null;
afterEach(() => {
  vi.useRealTimers();
  stop?.();
  stop = null;
  document.body.replaceChildren();
});

describe('броски извне в окно (исправление КТ1)', () => {
  it('перетаскивание извне (без dragstart на странице): над окном «запрещено», бросок гасится до обработчиков страницы', () => {
    stop = blockExternalDrops(document);
    const zone = document.createElement('div');
    document.body.append(zone);
    const pageDrop = vi.fn();
    zone.addEventListener('drop', pageDrop);

    const over = drag('dragover', zone);
    expect(over.defaultPrevented).toBe(true);
    expect(over.dataTransfer.dropEffect).toBe('none');
    const drop = drag('drop', zone);
    expect(drop.defaultPrevented).toBe(true);
    expect(pageDrop).not.toHaveBeenCalled();
  });

  it('перетаскивание внутри страницы проходит к её обработчикам', () => {
    stop = blockExternalDrops(document);
    const source = document.createElement('div');
    const zone = document.createElement('div');
    document.body.append(source, zone);
    const pageDrop = vi.fn();
    zone.addEventListener('drop', pageDrop);

    drag('dragstart', source);
    const over = drag('dragover', zone);
    expect(over.defaultPrevented).toBe(false);
    drag('drop', zone);
    expect(pageDrop).toHaveBeenCalledTimes(1);
    drag('dragend', source);

    // После конца своего перетаскивания следующее извне снова гасится.
    expect(drag('drop', zone).defaultPrevented).toBe(true);
    expect(pageDrop).toHaveBeenCalledTimes(1);
  });

  // Ревью: источник убран из DOM до dragend (CodeMirror удаляет перенесённый
  // диапазон, дерево перерисовывается) — dragend до документа не доходит, и
  // флаг «своё перетаскивание» не должен застревать.
  it('источник удалён до dragend: следующий бросок извне всё равно гасится', () => {
    vi.useFakeTimers();
    stop = blockExternalDrops(document);
    const source = document.createElement('div');
    const zone = document.createElement('div');
    document.body.append(source, zone);
    const pageDrop = vi.fn();
    zone.addEventListener('drop', pageDrop);

    drag('dragstart', source);
    source.remove();
    drag('drop', zone);
    expect(pageDrop).toHaveBeenCalledTimes(1);
    drag('dragend', source); // уходит в никуда: источник вне документа
    vi.runAllTimers();

    expect(drag('dragover', zone).defaultPrevented).toBe(true);
    expect(drag('drop', zone).defaultPrevented).toBe(true);
    expect(pageDrop).toHaveBeenCalledTimes(1);
  });

  it('источник удалён, броска не было: движение мыши снимает флаг своего перетаскивания', () => {
    stop = blockExternalDrops(document);
    const source = document.createElement('div');
    const zone = document.createElement('div');
    document.body.append(source, zone);
    drag('dragstart', source);
    source.remove();
    zone.dispatchEvent(new Event('mousemove', { bubbles: true }));
    expect(drag('drop', zone).defaultPrevented).toBe(true);
  });

  it('файлы ОС гасятся всегда, даже во время своего перетаскивания', () => {
    stop = blockExternalDrops(document);
    const source = document.createElement('div');
    const zone = document.createElement('div');
    document.body.append(source, zone);
    const pageDrop = vi.fn();
    zone.addEventListener('drop', pageDrop);
    drag('dragstart', source);
    expect(drag('dragover', zone, ['Files']).defaultPrevented).toBe(true);
    expect(drag('drop', zone, ['Files']).defaultPrevented).toBe(true);
    expect(pageDrop).not.toHaveBeenCalled();
  });
});
