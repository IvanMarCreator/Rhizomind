// Проверка лицензий npm: все production-зависимости интерфейса совместимы с GPL-3.0.
// Источник — package-lock.json. Запуск: node tools/check-licenses/cli.ts
// Код выхода 1, если есть нарушения.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { findLicenseProblems, productionPackages } from './licenses.ts';

const lockPath = path.join(import.meta.dirname, '..', '..', 'package-lock.json');
const lock: unknown = JSON.parse(await readFile(lockPath, 'utf8'));
const problems = findLicenseProblems(lock);

if (problems.length > 0) {
  console.error(
    'Лицензии production-зависимостей несовместимы с GPL-3.0 или не указаны:\n' +
      problems.map((p) => `  ${p.name}@${p.version}  ${p.license}`).join('\n') +
      '\nЗамените зависимость или согласуйте исключение (docs/dependencies.md).',
  );
  process.exit(1);
}

const packages = productionPackages(lock);
console.log(`check:licenses — ${packages.length} production-зависимостей npm, все лицензии совместимы с GPL-3.0:`);
for (const [key, info] of packages) {
  console.log(`  ${key.replace(/^.*node_modules\//, '')}@${info.version ?? '?'}  ${String(info.license)}`);
}
