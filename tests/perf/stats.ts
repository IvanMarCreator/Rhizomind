// Сводка замеров производительности (задача 3.2, С9.5).
//
// Замер критерия — несколько повторов (запуск программы, открытие быстрого
// перехода с вводом запроса, серия нажатий в редакторе). Повтор сводится
// одним числом: худшим значением (критерий «не дольше») или 95-м процентилем
// (сотни нажатий: «без заметных задержек»). Вывод — по медиане повторов.
//
// Почему медиана, а не одна попытка: раннеры GitHub — общие машины, и
// отдельная попытка там бывает в разы медленнее обычной (задача 3.2а,
// docs/reviews/2026-10-29-task-3.2.md: генератор на Windows — медиана 5,7 с,
// разово 58 с, повтор сразу после — 6 с). Медиана трёх-пяти повторов снимает
// один-два таких выброса, а систематическое превышение (большинство повторов
// за пределом) по-прежнему валит шаг. Худший результат — в отчёте рядом.

/** Замер одного критерия: значения в мс по повторам. */
export interface Measured {
  id: string;
  title: string;
  /** Предел в мс; `null` — справочный замер без вывода. */
  limitMs: number | null;
  /** Как свести повтор к одному числу. */
  repeatStat: 'max' | 'p95';
  repeats: number[][];
}

export interface Summary {
  id: string;
  title: string;
  limitMs: number | null;
  repeatStat: 'max' | 'p95';
  perRepeat: number[];
  median: number;
  worst: number;
  samples: number;
  /** `null` — справочный замер. */
  pass: boolean | null;
}

function sorted(values: readonly number[]): number[] {
  if (values.length === 0) throw new Error('нет значений');
  return [...values].sort((a, b) => a - b);
}

/** Элемент массива по индексу; выход за границы — ошибка замера, а не undefined. */
export function nth(values: readonly number[], index: number): number {
  const value = values[index];
  if (value === undefined) throw new Error(`нет значения ${index} из ${values.length}`);
  return value;
}

export function median(values: readonly number[]): number {
  const s = sorted(values);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? nth(s, mid) : (nth(s, mid - 1) + nth(s, mid)) / 2;
}

/** Процентиль по ближайшему рангу: значение, не меньше которого `p` % набора. */
export function percentile(values: readonly number[], p: number): number {
  const s = sorted(values);
  return nth(s, Math.max(0, Math.ceil((p / 100) * s.length) - 1));
}

export function summarize(measured: Measured): Summary {
  const perRepeat = measured.repeats.map((repeat, i) => {
    if (repeat.length === 0) throw new Error(`${measured.title}: повтор ${i + 1} без значений`);
    return measured.repeatStat === 'max' ? Math.max(...repeat) : percentile(repeat, 95);
  });
  const all = measured.repeats.flat();
  const mid = median(perRepeat);
  return {
    id: measured.id,
    title: measured.title,
    limitMs: measured.limitMs,
    repeatStat: measured.repeatStat,
    perRepeat,
    median: mid,
    worst: Math.max(...all),
    samples: all.length,
    pass: measured.limitMs === null ? null : mid <= measured.limitMs,
  };
}

/** Миллисекунды для таблицы: до десятых, без «.0». */
export function ms(value: number): string {
  return String(Math.round(value * 10) / 10);
}

export function renderTable(summaries: readonly Summary[]): string {
  const rows = summaries.map((s) => {
    const verdict = s.pass === null ? 'справочно' : s.pass ? 'укладывается' : '**НЕ укладывается**';
    const limit = s.limitMs === null ? '—' : `≤ ${ms(s.limitMs)} мс`;
    return `| ${s.title} | ${limit} | ${ms(s.median)} мс | ${ms(s.worst)} мс | ${s.perRepeat.map(ms).join(' / ')} | ${verdict} |`;
  });
  return ['| Критерий | Предел | Медиана | Худший | Повторы | Вывод |', '|---|---|---|---|---|---|', ...rows].join('\n');
}
