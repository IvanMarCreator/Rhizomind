import { afterEach, describe, expect, it, vi } from 'vitest';
import { blockExternalDrops } from './externalDrop.ts';

/** Событие перетаскивания (в jsdom нет DragEvent и DataTransfer). */
function drag(type: string, target: EventTarget): Event & { dataTransfer: { dropEffect: string } } {
  const event = Object.assign(new Event(type, { bubbles: true, cancelable: true }), {
    dataTransfer: { dropEffect: 'copy' },
  });
  target.dispatchEvent(event);
  return event;
}

let stop: (() => void) | null = null;
afterEach(() => {
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
});
