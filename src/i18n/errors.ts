// Текст ошибки для пользователя — только из словаря. Сообщения исключений,
// `IpcFailure.detail` (текст Tauri может содержать параметры вызова, то есть
// имена заметок) и незнакомые ключи пользователю не показываются (обзор 0.4).
import { ApiVersionMismatch, CoreCallError } from '../api/index.ts';
import { hasMessage, t } from './index.ts';

export function userMessage(error: unknown): string {
  if (error instanceof ApiVersionMismatch) {
    return t('startup.apiMismatch.body', { expected: error.expected, actual: error.actual });
  }
  if (error instanceof CoreCallError) {
    const failure = error.error;
    if (failure.code !== 'ipc_failed') {
      const problemKey = failure.name_problem ? `names.${failure.name_problem}` : null;
      if (problemKey && hasMessage(problemKey)) return t(problemKey);
      if (hasMessage(failure.message_key)) return t(failure.message_key);
    }
  }
  return t('errors.ipc_failed');
}
