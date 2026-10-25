// Проверка версии API ядра при запуске интерфейса (С5.5).
import { core } from './commands.ts';
import { API_VERSION, type VaultCurrentResponse } from './types.ts';

/** Ядро говорит на другой версии API, чем этот интерфейс. */
export class ApiVersionMismatch extends Error {
  readonly expected: number;
  readonly actual: number;

  constructor(expected: number, actual: number) {
    super(`API ядра версии ${actual}, интерфейс ожидает ${expected}`);
    this.name = 'ApiVersionMismatch';
    this.expected = expected;
    this.actual = actual;
  }
}

/**
 * Первый вызов интерфейса при запуске: `vault_current` и сверка
 * `api_version`. Возвращает ответ (хранилище окна), чтобы не вызывать
 * команду дважды; другая версия — `ApiVersionMismatch`.
 */
export async function checkApiVersion(): Promise<VaultCurrentResponse> {
  const current = await core.vaultCurrent();
  if (current.api_version !== API_VERSION) {
    throw new ApiVersionMismatch(API_VERSION, current.api_version);
  }
  return current;
}
