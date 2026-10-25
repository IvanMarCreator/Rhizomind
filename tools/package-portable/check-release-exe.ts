// Проверка перед упаковкой установщиков (package:msi, package:nsis): tauri
// запускает её после сборки и до упаковки (build.beforeBundleCommand в
// src-tauri/tauri.conf.json). Тестовая сборка (функция webdriver, порт отладки)
// к пользователю попасть не должна — тот же признак, что у package:portable.
// Код выхода 1 — упаковка прерывается.
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { isWebdriverBuild } from './build-kind.ts';

const root = path.join(import.meta.dirname, '..', '..');
const targetDir = process.env.CARGO_TARGET_DIR
  ? path.resolve(root, process.env.CARGO_TARGET_DIR)
  : path.join(root, 'src-tauri', 'target');
const exeName = process.platform === 'win32' ? 'rhizomind.exe' : 'rhizomind';
// Сборка с --target кладёт файл в <папка>/<цель>/release.
const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
const candidates = [
  path.join(targetDir, 'release', exeName),
  ...(triple ? [path.join(targetDir, triple, 'release', exeName)] : []),
].filter((p) => existsSync(p));

if (candidates.length === 0) {
  console.error(`check-release-exe: не найден собранный ${exeName} в ${targetDir}`);
  process.exit(1);
}
for (const exe of candidates) {
  if (isWebdriverBuild(readFileSync(exe))) {
    console.error(`check-release-exe: ${exe} — тестовая сборка (функция webdriver, порт отладки): упаковывать нельзя.`);
    process.exit(1);
  }
}
console.log(`check-release-exe: выпускная сборка (${candidates.map((p) => path.relative(root, p)).join(', ')})`);
