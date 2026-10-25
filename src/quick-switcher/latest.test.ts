import { describe, expect, it } from 'vitest';
import { LatestOnly } from './latest.ts';

interface Deferred {
  query: string;
  resolve: (value: string) => void;
  reject: (reason: unknown) => void;
}

/** Запросы, которые отвечают, когда тест скажет. */
function controlled() {
  const pending: Deferred[] = [];
  const run = (query: string) =>
    new Promise<string>((resolve, reject) => pending.push({ query, resolve, reject }));
  return { pending, run };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('только последний запрос', () => {
  it('первый запрос уходит сразу, без задержки', () => {
    const { pending, run } = controlled();
    const latest = new LatestOnly(run, () => {}, () => {});

    latest.request('в');

    expect(pending.map((p) => p.query)).toEqual(['в']);
  });

  it('пока запрос в работе, промежуточные не отправляются; после ответа уходит последний', async () => {
    const { pending, run } = controlled();
    const results: string[] = [];
    const latest = new LatestOnly(run, (_, value) => results.push(value), () => {});

    latest.request('в');
    latest.request('вс');
    latest.request('вст');
    latest.request('встр');
    expect(pending).toHaveLength(1);

    pending[0]?.resolve('ответ в');
    await flush();
    // Устаревший ответ не показывается: запрос уже другой.
    expect(results).toEqual([]);
    expect(pending.map((p) => p.query)).toEqual(['в', 'встр']);

    pending[1]?.resolve('ответ встр');
    await flush();
    expect(results).toEqual(['ответ встр']);
  });

  it('ошибка устаревшего запроса не показывается, последнего — показывается', async () => {
    const { pending, run } = controlled();
    const errors: unknown[] = [];
    const latest = new LatestOnly(run, () => {}, (_, error) => errors.push(error));

    latest.request('а');
    latest.request('б');
    pending[0]?.reject('сбой а');
    await flush();
    expect(errors).toEqual([]);

    pending[1]?.reject('сбой б');
    await flush();
    expect(errors).toEqual(['сбой б']);
  });

  it('после остановки ответы не доставляются и новые запросы не уходят', async () => {
    const { pending, run } = controlled();
    const results: string[] = [];
    const latest = new LatestOnly(run, (_, value) => results.push(value), () => {});

    latest.request('а');
    latest.request('б');
    latest.dispose();
    pending[0]?.resolve('ответ а');
    await flush();
    latest.request('в');

    expect(results).toEqual([]);
    expect(pending).toHaveLength(1);
  });

  it('ответ доставляется вместе со своим запросом', async () => {
    const { pending, run } = controlled();
    const seen: [string, string][] = [];
    const latest = new LatestOnly(run, (query, value) => seen.push([query, value]), () => {});

    latest.request('итоги');
    pending[0]?.resolve('ответ');
    await flush();

    expect(seen).toEqual([['итоги', 'ответ']]);
  });
});
