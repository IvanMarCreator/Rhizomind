// Подписка на события ядра API v1 (С5.5). Окну выданы только права
// core:event:allow-listen и allow-unlisten: интерфейс слушает, но не отправляет.
// Слушаем события своего окна, а не всех окон: ядро отправляет события только
// окну своего хранилища (emit_to, задача 1.7).
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { CoreEvents, EventName } from './types.ts';

/** Отписка от события. */
export type Unsubscribe = () => void;

/** Подписывается на событие ядра для этого окна; обработчик получает данные события. */
export async function onCoreEvent<K extends EventName>(
  event: K,
  handler: (payload: CoreEvents[K]) => void,
): Promise<Unsubscribe> {
  return getCurrentWebviewWindow().listen<CoreEvents[K]>(event, (message) => handler(message.payload));
}
