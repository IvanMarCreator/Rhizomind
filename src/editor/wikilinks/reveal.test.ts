// Поиск заголовка для перехода `[[Заметка#Заголовок]]` (С3.4).
import { Text } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { findHeading } from './reveal.ts';

const doc = (text: string) => Text.of(text.split('\n'));
const lineOf = (text: string, heading: string) => {
  const at = findHeading(doc(text), heading);
  return at === null ? null : doc(text).lineAt(at).number;
};

describe('findHeading', () => {
  it('заголовок любого уровня, без учёта регистра и закрывающих решёток', () => {
    const text = '# Встреча\n\n## Итоги ##\n\n### итоги дня';
    expect(lineOf(text, 'Итоги')).toBe(3);
    expect(lineOf(text, 'ИТОГИ ДНЯ')).toBe(5);
    expect(lineOf(text, 'встреча')).toBe(1);
  });

  it('первый из одинаковых', () => {
    expect(lineOf('## А\n## А', 'А')).toBe(1);
  });

  it('нет такого, `#тег`, пустой заголовок — null', () => {
    expect(lineOf('#Итоги\nИтоги', 'Итоги')).toBeNull();
    expect(lineOf('# А', '')).toBeNull();
  });

  it('в блоке кода и блоке свойств — не заголовок', () => {
    expect(lineOf('---\ntitle: x\n# Итоги\n---\n```\n# Итоги\n```\n## Итоги', 'Итоги')).toBe(8);
  });

  it('Unicode: составная и готовая «й» совпадают', () => {
    expect(lineOf('## Мой план', 'Мой план')).toBe(1);
  });
});
