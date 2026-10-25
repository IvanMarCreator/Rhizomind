// Проверка для ИБ: `cargo tree -e normal,no-proc-macro --target <цель>` по каждой десктопной
// цели не содержит сетевых крейтов, и набор крейтов совпадает со списком в
// crates.json (новый крейт — только после просмотра).
// Запуск: node tools/check-cargo-tree/cli.ts [--update]. Код выхода 1 при находке
// или расхождении. `--update` записывает текущий набор в crates.json.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { DESKTOP_TARGETS, cargoTreeArgs, compareWithBaseline, crateNames, deniedCrates, findNetworkCrates } from './tree.ts';

const root = path.join(import.meta.dirname, '..', '..');
const manifest = path.join(root, 'src-tauri', 'Cargo.toml');
const baselinePath = path.join(import.meta.dirname, 'crates.json');
const update = process.argv.includes('--update');
const denied = deniedCrates(readFileSync(path.join(root, 'src-tauri', 'deny.toml'), 'utf8'));
const baseline = JSON.parse(readFileSync(baselinePath, 'utf8')) as Record<string, string[]>;
const next: Record<string, string[]> = {};
let failed = false;

for (const target of DESKTOP_TARGETS) {
  const output = execFileSync('cargo', cargoTreeArgs(target, manifest), {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  const names = crateNames(output);
  next[target] = names;
  const hits = findNetworkCrates(output, denied);
  if (hits.length === 0) {
    console.log(`check:cargo-tree — ${target}: ${names.length} крейтов, сетевых нет (пусто)`);
  } else {
    failed = true;
    console.error(`check:cargo-tree — ${target}: найдены сетевые крейты:\n${hits.map((h) => `  ${h}`).join('\n')}`);
  }
  const { added, removed } = compareWithBaseline(names, baseline[target] ?? []);
  if (!update && (added.length > 0 || removed.length > 0)) {
    failed = true;
    console.error(
      `check:cargo-tree — ${target}: набор крейтов изменился` +
        (added.length ? `\n  новые: ${added.join(', ')}` : '') +
        (removed.length ? `\n  пропали: ${removed.join(', ')}` : ''),
    );
  }
}

if (update) {
  writeFileSync(baselinePath, JSON.stringify(next, null, 2) + '\n');
  console.log(`check:cargo-tree — записан ${path.relative(root, baselinePath)}`);
} else if (failed) {
  console.error(
    'В ядре не должно быть сетевых библиотек (инвариант 1, ADR 0009). Новый крейт: проверить лицензию и ' +
      'сетевой код (docs/dependencies.md), затем node tools/check-cargo-tree/cli.ts --update.',
  );
  process.exit(1);
}
