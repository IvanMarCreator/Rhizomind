import { describe, expect, it } from 'vitest';
import { highlight, highlightItem, type Segment } from './highlight.ts';

/** Совпавшие куски в квадратных скобках: «[Встр]еча». */
function show(segments: Segment[]): string {
  return segments.map((s) => (s.match ? `[${s.text}]` : s.text)).join('');
}

describe('подсветка совпадений', () => {
  it('пустой запрос — без подсветки', () => {
    expect(show(highlight('Встреча', ''))).toBe('Встреча');
    expect(show(highlight('Встреча', '   '))).toBe('Встреча');
  });

  it('подстрока без учёта регистра', () => {
    expect(show(highlight('Встреча по запуску', 'встр'))).toBe('[Встр]еча по запуску');
    expect(show(highlight('План работ', 'РАБОТ'))).toBe('План [работ]');
  });

  it('нечёткое совпадение — буквы по порядку', () => {
    expect(show(highlight('Встреча', 'втч'))).toBe('[В]с[т]ре[ч]а');
  });

  it('несколько слов подсвечиваются каждое', () => {
    expect(show(highlight('Встреча по запуску', 'зап встр'))).toBe('[Встр]еча по [зап]уску');
  });

  it('слово без совпадения ничего не подсвечивает', () => {
    expect(show(highlight('Итоги', 'встр'))).toBe('Итоги');
  });

  it('символы вне BMP не разрываются', () => {
    expect(show(highlight('😀 План', 'план'))).toBe('😀 [План]');
  });

  it('элемент: слово ищется сначала в имени, потом в папках', () => {
    const item = { name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] };
    const result = highlightItem(item, 'альфа встр');
    expect(show(result.name)).toBe('[Встр]еча');
    expect(show(result.folders)).toBe('Проекты / [Альфа]');
  });

  it('элемент: слово с косой чертой делится на части', () => {
    const item = { name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] };
    const result = highlightItem(item, 'альфа/встр');
    expect(show(result.name)).toBe('[Встр]еча');
    expect(show(result.folders)).toBe('Проекты / [Альфа]');
  });

  it('элемент в корне — папок нет', () => {
    expect(highlightItem({ name: 'Итоги', path: ['Итоги.md'] }, 'ит').folders).toEqual([]);
  });
});
