// Подписка на события ядра API v1 (С5.5). Окну выданы только права
// core:event:allow-listen и allow-unlisten: интерфейс слушает, но не отправляет.
import { listen } from '@tauri-apps/api/event';
import type { CoreEvents, EventName } from './types.ts';

/** Отписка от события. */
export type Unsubscribe = () => void;

/** Подписывается на событие ядра; обработчик получает данные события. */
export async function onCoreEvent<K extends EventName>(
  event: K,
  handler: (payload: CoreEvents[K]) => void,
): Promise<Unsubscribe> {
  return listen<CoreEvents[K]>(event, (message) => handler(message.payload));
}
