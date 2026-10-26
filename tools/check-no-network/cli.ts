// Проверка для ИБ: в интерфейсе нет сетевого кода (инвариант 1, ADR 0009).
// Запуск: node tools/check-no-network/cli.ts <папка или файл>... [--maps <сборка>=<копия с картами>]...
//   [--allowlist <файл>]
// Проверяются исходники (src/, src-isolation/) и собранный интерфейс (dist/).
// Находки в чужом коде из node_modules проходят только по разобранному
// исключению из allowlist.json (решение владельца 52, см. allowlist.ts).
// Код выхода 1 и список «файл:строка:позиция  что найдено», если есть ошибки.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkNetwork, parseAllowlist, type MapPair } from './allowlist.ts';

const base = process.cwd();
const targets: string[] = [];
const maps: MapPair[] = [];
let allowlistFile = path.join(path.dirname(fileURLToPath(import.meta.url)), 'allowlist.json');

const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const arg = args[i] as string;
  if (arg === '--maps') {
    const [dist, copy] = (args[++i] ?? '').split('=');
    if (!dist || !copy) {
      console.error('--maps <сборка>=<копия сборки с картами>, например --maps dist=.network-check/dist');
      process.exit(2);
    }
    maps.push({ dist: path.resolve(base, dist), maps: path.resolve(base, copy) });
  } else if (arg === '--allowlist') {
    allowlistFile = path.resolve(base, args[++i] ?? '');
  } else {
    targets.push(arg);
  }
}
if (targets.length === 0) {
  console.error('Укажите папки для проверки, например: node tools/check-no-network/cli.ts src dist --maps dist=.network-check/dist');
  process.exit(2);
}

const allowlist = parseAllowlist(JSON.parse(await readFile(allowlistFile, 'utf8')));
const { problems, allowed } = await checkNetwork({
  targets: targets.map((t) => path.resolve(base, t)),
  base,
  maps,
  allowlist,
});

if (problems.length > 0) {
  console.error(
    'Проверка сети не пройдена (инвариант 1 в CLAUDE.md, решение владельца 52):\n' +
      problems.map((p) => `  ${p}`).join('\n') +
      '\nСвой код — уберите эти места. Чужой код — разберите находку и внесите исключение в ' +
      path.relative(base, allowlistFile).split(path.sep).join('/') +
      ' (копия — docs/SECURITY.md).',
  );
  process.exit(1);
}
const exceptions = allowed.length > 0 ? `; по разобранным исключениям пропущено находок: ${allowed.length}` : '';
console.log(`check:network — сетевой код не найден (${targets.join(', ')})${exceptions}`);
