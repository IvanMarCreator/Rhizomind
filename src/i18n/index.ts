// Тексты интерфейса: функция перевода над словарём `ru` (в 0.1 язык один).
import { ru } from './ru.ts';

export type MessageKey = keyof typeof ru;
export type MessageParams = Record<string, string | number>;

const messages: Readonly<Record<string, string>> = ru;

/** Ключ из словаря? Для ключей, пришедших строкой (`message_key` от ядра). */
export function hasMessage(key: string): key is MessageKey {
  return Object.hasOwn(messages, key);
}

/** Текст по ключу; `{имя}` заменяется параметром, без параметра остаётся как есть. */
export function t(key: MessageKey, params: MessageParams = {}): string {
  return messages[key]!.replace(/\{(\w+)\}/g, (whole, name: string) =>
    Object.hasOwn(params, name) ? String(params[name]) : whole,
  );
}
