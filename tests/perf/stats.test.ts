// Сводка замеров производительности (задача 3.2): статистики и таблица.
import { describe, expect, it } from 'vitest';
import { median, percentile, renderTable, summarize, type Measured } from './stats.ts';

describe('статистики', () => {
  it('медиана: нечётное и чётное число значений, порядок не важен', () => {
    expect(median([5, 1, 3])).toBe(3);
    expect(median([4, 1, 3, 2])).toBe(2.5);
  });

  it('процентиль — по ближайшему рангу: 95-й из 20 значений — 19-е по возрастанию', () => {
    const values = Array.from({ length: 20 }, (_, i) => 20 - i);
    expect(percentile(values, 95)).toBe(19);
    expect(percentile(values, 100)).toBe(20);
    expect(percentile([7], 95)).toBe(7);
  });

  it('пустой набор — ошибка, а не NaN в отчёте', () => {
    expect(() => median([])).toThrow();
    expect(() => percentile([], 95)).toThrow();
  });
});

const base: Omit<Measured, 'repeats'> = { id: 'x', title: 'Замер', limitMs: 100, repeatStat: 'max' };

describe('сводка критерия', () => {
  it('повтор сводится своей статистикой (худший), вывод — по медиане повторов', () => {
    const s = summarize({ ...base, repeats: [[10, 90], [20, 30], [500, 40]] });
    expect(s.perRepeat).toEqual([90, 30, 500]);
    expect(s.median).toBe(90);
    expect(s.worst).toBe(500);
    expect(s.pass).toBe(true);
  });

  it('разовый выброс в одном повторе из трёх не валит, систематическое превышение — валит', () => {
    expect(summarize({ ...base, repeats: [[50], [60], [900]] }).pass).toBe(true);
    expect(summarize({ ...base, repeats: [[150], [60], [140]] }).pass).toBe(false);
  });

  it('95-й процентиль внутри повтора', () => {
    const repeat = Array.from({ length: 20 }, (_, i) => i + 1);
    expect(summarize({ ...base, repeatStat: 'p95', repeats: [repeat] }).perRepeat).toEqual([19]);
  });

  it('справочный замер без предела: вывода нет', () => {
    const s = summarize({ ...base, limitMs: null, repeats: [[1]] });
    expect(s.pass).toBeNull();
  });

  it('повтор без значений — ошибка замера', () => {
    expect(() => summarize({ ...base, repeats: [[1], []] })).toThrow(/повтор 2/);
  });
});

describe('таблица', () => {
  it('строка критерия: предел, медиана, худший, значения повторов и вывод', () => {
    const table = renderTable([
      summarize({ ...base, title: 'Быстрый переход', repeats: [[40], [55], [41]] }),
      summarize({ ...base, title: 'Дерево', limitMs: 3000, repeats: [[3500], [3400], [100]] }),
      summarize({ ...base, title: 'Открытие', limitMs: null, repeats: [[12.34]] }),
    ]);
    const lines = table.split('\n');
    expect(lines[0]).toBe('| Критерий | Предел | Медиана | Худший | Повторы | Вывод |');
    expect(lines[2]).toBe('| Быстрый переход | ≤ 100 мс | 41 мс | 55 мс | 40 / 55 / 41 | укладывается |');
    expect(lines[3]).toBe('| Дерево | ≤ 3000 мс | 3400 мс | 3500 мс | 3500 / 3400 / 100 | **НЕ укладывается** |');
    expect(lines[4]).toBe('| Открытие | — | 12.3 мс | 12.3 мс | 12.3 | справочно |');
  });
});
