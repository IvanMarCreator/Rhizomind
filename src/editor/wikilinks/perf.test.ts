// Скорость автодополнения вики-ссылок на большой заметке (задача 2.6а,
// перепроверка QA): ввод и движение курсора после `[[` проверяют, не в коде
// ли ссылка, — по пошаговому разбору блоков (поле блоков подключено в обоих
// режимах), а не разбором оград от начала текста на каждое нажатие. Цель —
// ~5 мс; предел втрое больше, с запасом на CI. Прежнее значение в режиме
// исходного текста — ~220 мс на 9 МБ. Схема замера — как в
// src/editor/live-preview/perf.test.ts.
import { EditorSelection, type EditorState } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { MAX_BLOCK_TEXT } from '../live-preview/markdown.ts';
import { activeCompletion } from './autocomplete.ts';

const LIMIT_MS = 15;
const MB = 1024 * 1024;

/** Плотный Markdown с блоком кода на каждые ~150 символов. */
function dense(size: number): string {
  const parts: string[] = [];
  let length = 0;
  for (let i = 0; length < size; i++) {
    const chunk = `## Раздел ${i}\n\nТекст с **жирным**, \`код\` и [[Заметка ${i}]].\n\n\`\`\`js\nconst x = ${i};\n\`\`\`\n\n`;
    parts.push(chunk);
    length += chunk.length;
  }
  return parts.join('');
}

/** Лучший из нескольких замеров, мс; первые два прогона — разогрев. */
function best(fn: () => void, runs = 7): number {
  fn();
  fn();
  let time = Infinity;
  for (let k = 0; k < runs; k++) {
    const start = performance.now();
    fn();
    time = Math.min(time, performance.now() - start);
  }
  return time;
}

describe.each(['source', 'live_preview'] as const)('автодополнение в заметке ~9 МБ с блоками кода (%s)', (mode) => {
  const text = `${dense(9 * MB)}\nСм. [[Пла`;
  const end = text.length;
  const state: EditorState = createEditorState(text, { mode }).update({ selection: EditorSelection.cursor(end) }).state;

  it('список открыт (иначе замеры ничего не проверяют)', () => {
    expect(text.length).toBeLessThan(MAX_BLOCK_TEXT);
    expect(activeCompletion(state)?.query).toBe('Пла');
  });

  it(`ввод после [[ в конце — меньше ${LIMIT_MS} мс`, { timeout: 60_000 }, () => {
    const time = best(() => {
      const next = state.update({ changes: { from: end, insert: 'н' }, selection: { anchor: end + 1 } }).state;
      expect(activeCompletion(next)?.query).toBe('План');
    });
    expect(time).toBeLessThan(LIMIT_MS);
  });

  it(`движение курсора внутри [[ — меньше ${LIMIT_MS} мс`, { timeout: 60_000 }, () => {
    const time = best(() => {
      const next = state.update({ selection: { anchor: end - 1 } }).state;
      expect(activeCompletion(next)?.query).toBe('Пл');
    });
    expect(time).toBeLessThan(LIMIT_MS);
  });
});
