import { describe, expect, it } from 'vitest';
import { expectedTagKeys, releaseFixture } from './vault.ts';

const manifest = {
  format_version: 2,
  notes: 9_999,
  tags: [{ key: 'проект' }],
  hub: { path: 'Узел.md', backlinks: Array.from({ length: 100 }, (_, i) => ({ source: `Ссылка ${i}.md` })) },
  folder_1000: { path: 'Большой архив', notes: 1_000, incoming_links: 5_000 },
};

describe('нагрузка С2-8.3', () => {
  it('учитывает дополнительные теги 5000-строчной заметки и не удваивает общие', () => {
    expect(expectedTagKeys(['архив', 'проект'], '# Длинная заметка\nТеги: #Заметки #Проект и `#код`.'))
      .toEqual(['архив', 'заметки', 'проект']);
  });
  it('сверяет размер хранилища, папку и различные исходные заметки узла', () => {
    const fixture = releaseFixture(manifest, 10_000);
    expect(fixture.hubSources.size).toBe(100);
    expect(fixture.folder).toBe('Большой архив');
  });

  it('отклоняет неполную нагрузку или устаревший формат генератора', () => {
    expect(() => releaseFixture(manifest, 50_000)).toThrow();
    expect(() => releaseFixture({ ...manifest, format_version: 1 }, 10_000)).toThrow();
    expect(() => releaseFixture({ ...manifest, folder_1000: { ...manifest.folder_1000, incoming_links: 99 } }, 10_000)).toThrow();
    expect(() => releaseFixture({ ...manifest, hub: { ...manifest.hub, backlinks: Array(100).fill({ source: 'А.md' }) } }, 10_000)).toThrow();
  });
});
