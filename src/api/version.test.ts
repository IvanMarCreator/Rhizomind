import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mockCore } from './testing.ts';
import { ApiVersionMismatch, checkApiVersion } from './version.ts';

describe('проверка версии API при запуске', () => {
  beforeEach(() => {
    vi.stubGlobal('window', globalThis);
  });

  afterEach(() => {
    clearMocks();
    vi.unstubAllGlobals();
  });

  it('версия 1 принимается, ответ vault_current возвращается', async () => {
    mockCore({ vault_current: () => ({ api_version: 1, vault: null }) });

    await expect(checkApiVersion()).resolves.toEqual({ api_version: 1, vault: null });
  });

  it('другая версия ядра — ApiVersionMismatch с обеими версиями', async () => {
    mockCore({ vault_current: () => ({ api_version: 2, vault: null }) });

    const failure = await checkApiVersion().catch((e: unknown) => e);

    expect(failure).toBeInstanceOf(ApiVersionMismatch);
    expect(failure).toMatchObject({ expected: 1, actual: 2 });
  });
});
