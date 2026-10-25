import { describe, expect, it } from 'vitest';
import { hasMessage, t } from './index.ts';

describe('функция перевода', () => {
  it('возвращает текст по ключу', () => {
    expect(t('shell.empty.title')).toBe('Нет открытой заметки');
  });

  it('подставляет параметры', () => {
    expect(t('vaults.switcher.current', { name: 'Рабочая база' })).toBe('Хранилище «Рабочая база»');
  });

  it('параметр без значения остаётся видимым, а не превращается в undefined', () => {
    expect(t('vaults.switcher.current')).toBe('Хранилище «{name}»');
  });

  it('проверяет ключи, пришедшие от ядра строкой', () => {
    expect(hasMessage('errors.not_found')).toBe(true);
    expect(hasMessage('errors.нет_такого')).toBe(false);
    expect(hasMessage('shell')).toBe(false);
  });
});
