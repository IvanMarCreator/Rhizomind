// Проверка для ИБ: `cargo tree -e normal --target <цель>` по каждой десктопной
// цели не содержит сетевых крейтов. Ожидаемый результат — пусто для всех целей.
// Запуск: node tools/check-cargo-tree/cli.ts. Код выхода 1, если что-то найдено.
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { DESKTOP_TARGETS, cargoTreeArgs, findNetworkCrates } from './tree.ts';

const manifest = path.join(import.meta.dirname, '..', '..', 'src-tauri', 'Cargo.toml');
let failed = false;

for (const target of DESKTOP_TARGETS) {
  const output = execFileSync('cargo', cargoTreeArgs(target, manifest), {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  const crateCount = new Set(output.split('\n').filter(Boolean).map((l) => l.split('|')[0]?.replace(' (*)', ''))).size;
  const hits = findNetworkCrates(output);
  if (hits.length === 0) {
    console.log(`check:cargo-tree — ${target}: ${crateCount} крейтов, сетевых нет (пусто)`);
  } else {
    failed = true;
    console.error(`check:cargo-tree — ${target}: найдены сетевые крейты:\n${hits.map((h) => `  ${h}`).join('\n')}`);
  }
}

if (failed) {
  console.error('В ядре не должно быть сетевых библиотек (инвариант 1, ADR 0009).');
  process.exit(1);
}
