// Подмена ядра для тестов интерфейса (Vitest, компонентные тесты): без окна
// Tauri, на @tauri-apps/api/mocks. В приложение не импортируется.
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { CommandName, CommandRequest, CommandResponse, CoreError, CoreEvents, EventName } from './types.ts';

/** Подменный обработчик команды: получает запрос, возвращает ответ или бросает CoreError. */
export type CommandMock<K extends CommandName> = (
  request: CommandRequest<K>,
) => CommandResponse<K> | Promise<CommandResponse<K>>;

export type CoreMocks = { [K in CommandName]?: CommandMock<K> };

/** Вызов, дошедший до подменного ядра: имя команды и аргументы `invoke`. */
export type RecordedCall = [command: string, args: unknown];

const notImplemented: CoreError = { code: 'not_implemented', message_key: 'errors.not_implemented' };

type Invoke = (command: string, args?: unknown, options?: unknown) => Promise<unknown>;

declare global {
  interface Window {
    /** Мост Tauri; в тестах его ставит @tauri-apps/api/mocks. */
    __TAURI_INTERNALS__: { invoke: Invoke };
  }
}

/**
 * Подменяет ядро: команды из `mocks` отвечают подменой, остальные — ошибкой
 * `not_implemented`, как заглушки ядра. Окно — `main`; события подменяются
 * тоже (`emitCoreEvent`). Возвращает журнал всех вызовов моста, включая
 * подписку на события. После теста — `clearMocks()`.
 */
export function mockCore(mocks: CoreMocks): RecordedCall[] {
  const calls: RecordedCall[] = [];
  mockWindows('main');
  mockIPC(
    (command, args) => {
      const mock = (mocks as Record<string, ((request: unknown) => unknown) | undefined>)[command];
      if (!mock) throw notImplemented;
      const request = (args as { request?: unknown } | undefined)?.request ?? null;
      return mock(request);
    },
    { shouldMockEvents: true },
  );
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  internals.invoke = (command, args, options) => {
    calls.push([command, args ?? {}]);
    return invoke(command, args, options);
  };
  return calls;
}

/** Отправляет событие «от ядра» подписчикам интерфейса (после `mockCore`). */
export function emitCoreEvent<K extends EventName>(event: K, payload: CoreEvents[K]): Promise<void> {
  return emit(event, payload);
}
