import { describe, expect, it } from 'vitest';
import { parseLine } from '../live-preview/markdown.ts';

describe('embed в синтаксисе живого предпросмотра', () => {
  it('выделяет только полностью закрытые встраивания вне кода', () => {
    for (const value of ['`![[Код]]`','\\![[Буквально]]','![[незакрыто','![alt](https://example.invalid/a.png)']) {
      expect(parseLine(value,null).pieces.some(p => p.kind === 'embed')).toBe(false);
    }
    const text = 'Материалы ![[/Проекты/Фото.JPG]] и ![[А|видимое имя]]';
    expect(parseLine(text,null).pieces.filter(p => p.kind === 'embed').map(p => [text.slice(p.from,p.to),p.target])).toEqual([
      ['![[/Проекты/Фото.JPG]]','/Проекты/Фото.JPG'], ['![[А|видимое имя]]','А|видимое имя'],
    ]);
  });
});
