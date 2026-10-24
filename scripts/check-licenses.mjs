// Проверка лицензий: все production-зависимости (то, что попадает в сборку)
// имеют лицензию, совместимую с GPL-3.0. Источник — package-lock.json.
// Запуск: node scripts/check-licenses.mjs. Код выхода 1, если есть нарушения.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const ALLOWED = new Set([
  'MIT',
  'ISC',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'Apache-2.0',
  '0BSD',
  'MPL-2.0',
  'Unlicense',
  'CC0-1.0',
  'BlueOak-1.0.0',
  'Python-2.0',
]);
const ALLOWED_PREFIXES = ['GPL-3.0', 'LGPL-'];

function isAllowedId(id) {
  return ALLOWED.has(id) || ALLOWED_PREFIXES.some((prefix) => id.startsWith(prefix));
}

/** SPDX-выражение: OR — достаточно одной разрешённой, AND — нужны все. */
export function isAllowedLicense(expression) {
  if (typeof expression !== 'string' || expression.trim() === '') return false;
  const plain = expression.replace(/[()]/g, ' ').trim();
  return plain
    .split(/\s+OR\s+/)
    .some((alternative) => alternative.split(/\s+AND\s+/).every((id) => isAllowedId(id.trim())));
}

/** Пакеты из package-lock.json, которые ставятся без devDependencies. */
function productionPackages(lock) {
  return Object.entries(lock.packages ?? {}).filter(
    ([key, info]) => key !== '' && !info.dev && !info.devOptional && !info.link,
  );
}

/** Нарушения в package-lock.json: [{ name, version, license }]. */
export function findLicenseProblems(lock) {
  const problems = [];
  for (const [key, info] of productionPackages(lock)) {
    if (isAllowedLicense(info.license)) continue;
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);
    problems.push({ name, version: info.version, license: info.license ?? '(нет)' });
  }
  return problems;
}

async function main() {
  const lockPath = path.join(import.meta.dirname, '..', 'package-lock.json');
  const lock = JSON.parse(await readFile(lockPath, 'utf8'));
  const problems = findLicenseProblems(lock);
  if (problems.length > 0) {
    console.error(
      'Лицензии production-зависимостей несовместимы с GPL-3.0 или не указаны:\n' +
        problems.map((p) => `  ${p.name}@${p.version}  ${p.license}`).join('\n') +
        '\nЗамените зависимость или согласуйте исключение (docs/dependencies.md).',
    );
    process.exit(1);
  }
  const count = productionPackages(lock).length;
  console.log(`check:licenses — ${count} production-зависимостей, все лицензии совместимы с GPL-3.0`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) await main();
