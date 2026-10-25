// Доступ интерфейса к ядру: команды, события, проверка версии и типы API v1.
// Подмена ядра для тестов — отдельно, в ./testing.ts.
export { core } from './commands.ts';
export { CoreCallError, type IpcFailure } from './core.ts';
export { onCoreEvent, type Unsubscribe } from './events.ts';
export { ApiVersionMismatch, checkApiVersion } from './version.ts';
export type * from './types.ts';
export { API_VERSION } from './types.ts';
