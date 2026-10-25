import { describe, expect, it } from 'vitest';
import { ApiVersionMismatch, CoreCallError } from '../api/index.ts';
import { userMessage } from './errors.ts';

describe('текст ошибки для пользователя', () => {
  it('ошибка ядра — по message_key из словаря', () => {
    const error = new CoreCallError('note_read', { code: 'not_found', message_key: 'errors.not_found' });

    expect(userMessage(error)).toBe('Файл или папка не найдены.');
  });

  it('недопустимое имя — по причине names.<problem>', () => {
    const error = new CoreCallError('entry_rename', {
      code: 'invalid_name',
      message_key: 'errors.invalid_name',
      name_problem: 'trailing_dot_or_space',
    });

    expect(userMessage(error)).toBe('Имя не может заканчиваться точкой или пробелом.');
  });

  it('отказ моста Tauri: текст detail пользователю не показывается', () => {
    const error = new CoreCallError('note_read', {
      code: 'ipc_failed',
      message_key: 'errors.ipc_failed',
      detail: 'invalid args `request` for command `note_read`: Секретная/Заметка.md',
    });

    const text = userMessage(error);

    expect(text).toBe('Внутренняя ошибка приложения. Перезапустите окно.');
    expect(text).not.toContain('Секретная');
  });

  it('незнакомый ключ от ядра — общий текст, а не сам ключ', () => {
    const error = new CoreCallError('note_read', {
      code: 'io_failed',
      message_key: 'errors.новый_код',
    });

    expect(userMessage(error)).toBe('Внутренняя ошибка приложения. Перезапустите окно.');
  });

  it('несовпадение версии API — понятное сообщение с номерами версий', () => {
    expect(userMessage(new ApiVersionMismatch(1, 2))).toBe(
      'Интерфейс ожидает API ядра версии 1, а ядро сообщает версию 2. Установите Rhizomind заново из одного выпуска.',
    );
  });

  it('любая другая ошибка — общий текст без её сообщения', () => {
    expect(userMessage(new Error('stack: C:\\Users\\Иван\\Заметки'))).toBe(
      'Внутренняя ошибка приложения. Перезапустите окно.',
    );
    expect(userMessage('строка')).toBe('Внутренняя ошибка приложения. Перезапустите окно.');
  });
});
