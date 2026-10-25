// Подмена ядра для тестов интерфейса (Vitest, компонентные тесты): без окна
// Tauri, на @tauri-apps/api/mocks. В приложение не импортируется.
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import type { CommandName, CommandRequest, CommandResponse, CoreError, CoreEvents, EventName } from './types.ts';

/** Подменный обработчик команды: получает запрос, возвращает ответ или бросает CoreError. */
export type CommandMock<K extends CommandName> = (
  request: CommandRequest<K>,
) => CommandResponse<K> | Promise<CommandResponse<K>>;

export type CoreMocks = { [K in CommandName]?: CommandMock<K> };

/** Вызов, дошедший до подменного ядра: имя команды и аргументы `invoke`. */
export type RecordedCall = [command: string, args: unknown];

const notImplemented: CoreError = { code: 'not_implemented', message_key: 'errors.not_implemented' };

/**
 * Подменяет ядро: команды из `mocks` отвечают подменой, остальные — ошибкой
 * `not_implemented`, как заглушки ядра. События подменяются тоже
 * (`emitCoreEvent`). Возвращает журнал вызовов. После теста — `clearMocks()`.
 */
export function mockCore(mocks: CoreMocks): RecordedCall[] {
  const calls: RecordedCall[] = [];
  mockIPC(
    (command, args) => {
      calls.push([command, args ?? {}]);
      const mock = (mocks as Record<string, ((request: unknown) => unknown) | undefined>)[command];
      if (!mock) throw notImplemented;
      const request = (args as { request?: unknown } | undefined)?.request ?? null;
      return mock(request);
    },
    { shouldMockEvents: true },
  );
  return calls;
}

/** Отправляет событие «от ядра» подписчикам интерфейса (после `mockCore`). */
export function emitCoreEvent<K extends EventName>(event: K, payload: CoreEvents[K]): Promise<void> {
  return emit(event, payload);
}
