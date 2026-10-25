import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { onCoreEvent } from './events.ts';
import { emitCoreEvent, mockCore } from './testing.ts';
import type { NoteRenamedPayload } from './types.ts';

describe('события ядра', () => {
  beforeEach(() => {
    vi.stubGlobal('window', globalThis);
  });

  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  it('подписчик получает данные события, а после отписки — нет', async () => {
    mockCore({});
    const received: NoteRenamedPayload[] = [];

    const unlisten = await onCoreEvent('note:renamed', (payload) => received.push(payload));
    await emitCoreEvent('note:renamed', { from: ['a.md'], to: ['b', 'a.md'] });
    unlisten();
    await emitCoreEvent('note:renamed', { from: ['b', 'a.md'], to: ['a.md'] });

    expect(received).toEqual([{ from: ['a.md'], to: ['b', 'a.md'] }]);
  });

  it('подписка — на события своего окна, а не на все окна', async () => {
    const calls = mockCore({});

    await onCoreEvent('vault:changed', () => {});

    const listen = calls.find(([command]) => command === 'plugin:event|listen');
    expect(listen?.[1]).toMatchObject({
      event: 'vault:changed',
      target: { kind: 'WebviewWindow', label: 'main' },
    });
  });

  it('подписка на одно событие не получает другие', async () => {
    mockCore({});
    const received: unknown[] = [];

    await onCoreEvent('note:deleted', (payload) => received.push(payload));
    await emitCoreEvent('vault:changed', { rescan: true, changes: [] });

    expect(received).toEqual([]);
  });
});
