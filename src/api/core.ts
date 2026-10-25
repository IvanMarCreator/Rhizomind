// Единственная точка обращения интерфейса к ядру (спецификация 0.1, С5.4).
// Остальной интерфейс импортирует только модуль src/api/, а не @tauri-apps/api,
// чтобы в тестах ядро подменялось в одном месте (src/api/testing.ts).
import { invoke } from '@tauri-apps/api/core';
import type { CommandName, CommandRequest, CommandResponse, CoreError } from './types.ts';

/**
 * Отказ не от ядра, а от моста Tauri: команда не разрешена окну, параметры не
 * разобрались (лишнее поле, неверный тип). Для интерфейса это ошибка программы.
 */
export interface IpcFailure {
  code: 'ipc_failed';
  message_key: 'errors.ipc_failed';
  /** Текст Tauri — только для журнала, не для показа. */
  detail: string;
}

/** Отклонённый вызов команды ядра. */
export class CoreCallError extends Error {
  readonly command: string;
  readonly error: CoreError | IpcFailure;

  constructor(command: string, error: CoreError | IpcFailure) {
    super(`${command}: ${error.code}`);
    this.name = 'CoreCallError';
    this.command = command;
    this.error = error;
  }
}

function isCoreError(value: unknown): value is CoreError {
  if (typeof value !== 'object' || value === null) return false;
  const { code, message_key } = value as Record<string, unknown>;
  return typeof code === 'string' && typeof message_key === 'string';
}

function toFailure(reason: unknown): CoreError | IpcFailure {
  if (isCoreError(reason)) return reason;
  const detail = reason instanceof Error ? reason.message : String(reason);
  return { code: 'ipc_failed', message_key: 'errors.ipc_failed', detail };
}

/** Аргументы обёртки: без параметров — ничего, иначе — объект запроса. */
export type CallArgs<K extends CommandName> = CommandRequest<K> extends null ? [] : [request: CommandRequest<K>];

/**
 * Вызов команды ядра API v1. Параметры уходят ядру объектом `{ request }`
 * (Rust: один параметр `request` со строгой схемой). Отказ — `CoreCallError`.
 */
export async function callCore<K extends CommandName>(
  command: K,
  ...args: CallArgs<K>
): Promise<CommandResponse<K>> {
  const [request] = args;
  try {
    return await invoke<CommandResponse<K>>(command, request === undefined ? {} : { request });
  } catch (reason) {
    throw new CoreCallError(command, toFailure(reason));
  }
}
