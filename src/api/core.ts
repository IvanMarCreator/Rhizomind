// Единственная точка обращения интерфейса к ядру (спецификация 0.1, С5.4).
// Остальной интерфейс импортирует только этот модуль, а не @tauri-apps/api,
// чтобы в тестах ядро подменялось в одном месте (@tauri-apps/api/mocks).
// Типизированные команды и события API ядра v1 добавляет задача 0.4.
import { invoke } from '@tauri-apps/api/core';

/** Параметры команды ядра: объект с именованными полями. */
export type CoreArgs = Record<string, unknown>;

/** Вызов команды ядра. Отказ ядра — отклонённое обещание. */
export function callCore<T>(command: string, args?: CoreArgs): Promise<T> {
  return invoke<T>(command, args);
}
