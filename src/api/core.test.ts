import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { callCore } from './core.ts';

describe('callCore', () => {
  beforeEach(() => {
    // Подмена ядра из @tauri-apps/api/mocks работает через window.
    vi.stubGlobal('window', globalThis);
  });

  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  it('передаёт имя команды и параметры ядру и возвращает его ответ', async () => {
    const calls: [string, unknown][] = [];
    mockIPC((command, args) => {
      calls.push([command, args]);
      return { ok: true };
    });

    await expect(callCore<{ ok: boolean }>('vault_current', { id: 'a' })).resolves.toEqual({ ok: true });
    expect(calls).toEqual([['vault_current', { id: 'a' }]]);
  });

  it('отказ ядра приходит в интерфейс как отклонённое обещание', async () => {
    mockIPC(() => {
      throw new Error('нет такой команды');
    });

    await expect(callCore('unknown')).rejects.toThrow('нет такой команды');
  });
});
