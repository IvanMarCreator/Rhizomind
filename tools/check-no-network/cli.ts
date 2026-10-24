// Проверка для ИБ: в интерфейсе нет сетевого кода (инвариант 1, ADR 0009).
// Запуск: node tools/check-no-network/cli.ts <папка или файл>...
// Проверяются исходники (src/) и собранный интерфейс (dist/).
// Код выхода 1 и список «файл:строка  что найдено», если есть находки.
import path from 'node:path';
import { scanPaths } from './scan.ts';

const targets = process.argv.slice(2);
if (targets.length === 0) {
  console.error('Укажите папки для проверки, например: node tools/check-no-network/cli.ts src dist');
  process.exit(2);
}

const base = process.cwd();
const found = await scanPaths(
  targets.map((t) => path.resolve(base, t)),
  base,
);

if (found.length > 0) {
  const label = { network: 'сеть', suspicious: 'подозрительно' } as const;
  console.error(
    'Найден сетевой или подозрительный код, запрещённый в Secure-редакции (инвариант 1 в CLAUDE.md):\n' +
      found.map((f) => `  ${f.file}:${f.line}  [${label[f.severity]}] ${f.what}`).join('\n') +
      '\nУберите эти места: в интерфейсе не должно быть сетевых возможностей.',
  );
  process.exit(1);
}
console.log(`check:network — сетевой код не найден (${targets.join(', ')})`);
