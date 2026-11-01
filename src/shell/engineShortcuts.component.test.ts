import { afterEach, describe, expect, it } from 'vitest';
import { blockEngineContextMenu, blockEngineZoom } from './engineShortcuts.ts';

describe('масштаб Ctrl+колесо (решение 94)', () => {
  let stop: (() => void) | undefined;
  afterEach(() => stop?.());

  function wheel(init: WheelEventInit): WheelEvent {
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -100, ...init });
    document.body.dispatchEvent(event);
    return event;
  }

  it('Ctrl+колесо гасится, обычная прокрутка — нет', () => {
    stop = blockEngineZoom(document);
    expect(wheel({ ctrlKey: true }).defaultPrevented).toBe(true);
    expect(wheel({ metaKey: true }).defaultPrevented).toBe(true);
    expect(wheel({}).defaultPrevented).toBe(false);
    expect(wheel({ shiftKey: true }).defaultPrevented).toBe(false);
  });
});

describe('меню движка (решение 97)', () => {
  let stop: (() => void) | undefined;
  afterEach(() => stop?.());

  it('правый щелчок где угодно не открывает меню движка; свои обработчики его получают', () => {
    stop = blockEngineContextMenu(document);
    const button = document.createElement('button');
    document.body.appendChild(button);
    let seen = false;
    button.addEventListener('contextmenu', () => (seen = true));

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    button.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(seen).toBe(true);
    button.remove();
  });
});
