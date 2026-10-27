// Запрет ESLint на окно через `.defaultView` и `.win` в коде интерфейса (ревью 2.4, В1).
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ESLint } from 'eslint';
import { beforeAll, describe, expect, it } from 'vitest';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const eslint = new ESLint({ cwd: root });

async function restricted(code: string, file: string): Promise<number> {
  const [result] = await eslint.lintText(code, { filePath: path.join(root, file) });
  return (result?.messages ?? []).filter((m) => m.ruleId === 'no-restricted-syntax').length;
}

// Первый вызов ESLint загружает настройки и парсеры (секунды; под нагрузкой
// параллельных тестов — больше 5 с, тест «мигал»). Прогрев — отдельно, со своим
// пределом; сами проверки после него быстрые.
beforeAll(async () => {
  await restricted('export {};\n', 'src/x.ts');
  await restricted('export {};\n', 'src-isolation/x.js');
  await restricted('export {};\n', 'tools/x.ts');
}, 60_000);

describe('ESLint: окно через .defaultView и .win', { timeout: 15_000 }, () => {
  it.each([
    ['export const w = document.body.ownerDocument.defaultView;\n', 'src/x.ts'],
    ['export const f = (v: { win: Window }) => v.win;\n', 'src/editor/x.ts'],
    ["export const f = (v: Record<string, unknown>) => v['win'];\n", 'src/x.ts'],
    ["export const f = (d: Document) => d['defaultView'];\n", 'src/x.ts'],
    ['const w = document.defaultView;\n', 'src-isolation/x.js'],
  ])('запрещено: %s в %s', async (code, file) => {
    expect(await restricted(code, file)).toBe(1);
  });

  it('другие свойства и инструменты в tools/ — не задеты', async () => {
    expect(await restricted('export const h = window.innerHeight;\n', 'src/x.ts')).toBe(0);
    expect(await restricted('export const f = (v: { win: number }) => v.win;\n', 'tools/x.ts')).toBe(0);
  });
});
