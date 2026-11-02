// Поиск заголовка для перехода `[[Заметка#Заголовок]]` (С3.4).
import { Text } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { findHeading, positionAt } from './reveal.ts';

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

describe('positionAt — место ссылки из обратных ссылок (0.2-2.2)', () => {
  const text = doc('первая\nвторая 😀 [[Ссылка]]\nтретья');

  it('строка с 1 и столбец в единицах UTF-16', () => {
    const at = positionAt(text, { line: 2, column: 10 });
    expect(text.sliceString(at, at + 2)).toBe('[[');
  });

  it('за пределами — к ближайшему: строка и столбец ограничиваются', () => {
    expect(positionAt(text, { line: 1, column: 100 })).toBe(text.line(1).to);
    expect(positionAt(text, { line: 99, column: 0 })).toBe(text.line(3).from);
    expect(positionAt(text, { line: 0, column: -5 })).toBe(0);
  });
});
