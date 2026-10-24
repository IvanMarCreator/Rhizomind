// SBOM (перечень зависимостей) для выпуска: ядро — cargo-cyclonedx 0.5,
// интерфейс — npm sbom. Результат — в release/.
// npm sbom берётся полным (с инструментами сборки): с `--omit dev` npm 10
// теряет svelte (пакет одновременно peer-зависимость dev-пакетов), а перечень
// без пропусков ИБ важнее краткости. Что попадает в сборку — docs/dependencies.md.
// Нужен установленный cargo-cyclonedx: cargo install --locked cargo-cyclonedx@0.5.9
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..', '..');
const outDir = path.join(root, 'release');
mkdirSync(outDir, { recursive: true });

const npmSbom = execFileSync(
  process.platform === 'win32' ? 'npm.cmd' : 'npm',
  ['sbom', '--sbom-format', 'cyclonedx', '--sbom-type', 'application'],
  { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, shell: process.platform === 'win32' },
);
writeFileSync(path.join(outDir, 'sbom-npm.cdx.json'), npmSbom);

const manifest = path.join(root, 'src-tauri', 'Cargo.toml');
execFileSync('cargo', ['cyclonedx', '--manifest-path', manifest, '--format', 'json', '--override-filename', 'sbom-rust.cdx'], {
  cwd: root,
  stdio: 'inherit',
});
const generated = path.join(root, 'src-tauri', 'sbom-rust.cdx.json');
copyFileSync(generated, path.join(outDir, 'sbom-rust.cdx.json'));
rmSync(generated);

console.log('sbom — release/sbom-npm.cdx.json, release/sbom-rust.cdx.json');
