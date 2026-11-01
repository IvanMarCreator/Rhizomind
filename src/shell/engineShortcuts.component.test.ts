import { afterEach, describe, expect, it } from 'vitest';
import { blockEngineZoom } from './engineShortcuts.ts';

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
