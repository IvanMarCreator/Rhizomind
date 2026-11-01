// SBOM (перечень зависимостей) и список лицензий ядра для выпуска — без сети.
// Результат — в release/:
//   sbom-rust.cdx.json  — ядро, CycloneDX 1.5 (cargo-cyclonedx 0.5) для цели выпуска;
//   sbom-npm.cdx.json   — интерфейс, CycloneDX (npm sbom);
//   licenses-rust.json  — лицензии крейтов ядра по цели выпуска (cargo deny list).
//
// Цель Rust задаётся явно (по умолчанию x86_64-pc-windows-msvc — выпуск 0.1 для
// Windows; другая — `npm run sbom -- --target <цель>`): без неё cargo-cyclonedx
// берёт цель машины сборки, и на Linux в перечень попали бы WebKitGTK и libsoup
// вместо WebView2 (аудит 3.3, docs/security/audit-0.1.md).
//
// Без сети: cargo запускается с CARGO_NET_OFFLINE=true (cargo deny — ещё и с
// --offline), npm — с --offline; всё берётся из Cargo.lock, package-lock.json и
// уже скачанных пакетов. Если чего-то нет локально, команда падает, а не скачивает.
//
// npm sbom берётся полным (с инструментами сборки): с `--omit dev` npm 10
// теряет svelte (пакет одновременно peer-зависимость dev-пакетов), а перечень
// без пропусков ИБ важнее краткости. Что попадает в сборку — docs/dependencies.md.
// Нужны cargo-cyclonedx 0.5.9 и cargo-deny 0.20.2 (cargo install --locked …).
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const DEFAULT_TARGET = 'x86_64-pc-windows-msvc';

function targetFromArgs(args: readonly string[]): string {
  const index = args.indexOf('--target');
  if (index === -1) return DEFAULT_TARGET;
  const target = args[index + 1];
  if (target === undefined || !/^[a-z0-9_]+(-[a-z0-9_.]+){2,3}$/.test(target)) {
    throw new Error(`sbom: после --target нужна цель Rust, например ${DEFAULT_TARGET}`);
  }
  return target;
}

const target = targetFromArgs(process.argv.slice(2));
const root = path.join(import.meta.dirname, '..', '..');
const outDir = path.join(root, 'release');
mkdirSync(outDir, { recursive: true });
const offline = { ...process.env, CARGO_NET_OFFLINE: 'true' };
const manifest = path.join(root, 'src-tauri', 'Cargo.toml');

const npmSbom = execFileSync(
  process.platform === 'win32' ? 'npm.cmd' : 'npm',
  ['sbom', '--offline', '--sbom-format', 'cyclonedx', '--sbom-type', 'application'],
  { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, shell: process.platform === 'win32' },
);
writeFileSync(path.join(outDir, 'sbom-npm.cdx.json'), npmSbom);

execFileSync(
  'cargo',
  [
    'cyclonedx',
    '--manifest-path',
    manifest,
    '--format',
    'json',
    '--spec-version',
    '1.5',
    '--target',
    target,
    '--override-filename',
    'sbom-rust.cdx',
  ],
  { cwd: root, stdio: 'inherit', env: offline },
);
const generated = path.join(root, 'src-tauri', 'sbom-rust.cdx.json');
copyFileSync(generated, path.join(outDir, 'sbom-rust.cdx.json'));
rmSync(generated);
// cargo-cyclonedx пишет SBOM для каждого члена рабочего пространства. Генератор
// тестового хранилища (tools/gen-vault) в программу не входит — его SBOM не нужен.
rmSync(path.join(root, 'tools', 'gen-vault', 'sbom-rust.cdx.json'), { force: true });

// Список лицензий для ИБ: cargo deny уже закреплён в CI и проверяет те же
// лицензии (src-tauri/deny.toml), отдельный cargo-about не нужен.
const licenses = execFileSync(
  'cargo',
  ['deny', '--manifest-path', manifest, '--offline', '--target', target, 'list', '--format', 'json', '--layout', 'license'],
  { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: offline },
);
writeFileSync(path.join(outDir, 'licenses-rust.json'), licenses);

console.log(
  `sbom (${target}) — release/sbom-rust.cdx.json, release/sbom-npm.cdx.json, release/licenses-rust.json`,
);
