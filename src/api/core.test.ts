import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CoreCallError, callCore } from './core.ts';

describe('callCore', () => {
  beforeEach(() => {
    // Подмена ядра из @tauri-apps/api/mocks работает через window.
    vi.stubGlobal('window', globalThis);
  });

  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  it('передаёт имя команды и запрос ядру и возвращает его ответ', async () => {
    const calls: [string, unknown][] = [];
    mockIPC((command, args) => {
      calls.push([command, args]);
      return { outcome: 'focused' };
    });

    await expect(callCore('vault_open', { id: 'a' })).resolves.toEqual({ outcome: 'focused' });
    expect(calls).toEqual([['vault_open', { request: { id: 'a' } }]]);
  });

  it('отказ моста Tauri (не ошибка ядра) — CoreCallError с кодом ipc_failed', async () => {
    mockIPC(() => {
      throw new Error('vault_list not allowed');
    });

    const failure = await callCore('vault_list').catch((e: unknown) => e);

    expect(failure).toBeInstanceOf(CoreCallError);
    expect((failure as CoreCallError).error).toEqual({
      code: 'ipc_failed',
      message_key: 'errors.ipc_failed',
      detail: 'vault_list not allowed',
    });
  });
});
