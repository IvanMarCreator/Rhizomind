/** Версия API ядра, которую мост выставляет интерфейсу как window.rhizomind. */
export const API_VERSION = 1;

/** API ядра v1. Пока только версия; методы появятся с задачей 0.4. */
export interface RhizomindApi {
  readonly apiVersion: typeof API_VERSION;
}
