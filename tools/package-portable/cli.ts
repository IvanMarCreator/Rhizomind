// Портативная сборка для Windows: zip с rhizomind.exe и текстом лицензии.
// Запускается после `tauri build --no-bundle` (npm run package:portable).
// Архив собирает tar.exe из Windows (bsdtar умеет zip), без сторонних пакетов.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

if (process.platform !== 'win32') {
  console.error('package:portable собирается только на Windows (rhizomind.exe).');
  process.exit(2);
}

const root = path.join(import.meta.dirname, '..', '..');
const { version } = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')) as { version: string };
const exe = path.join(root, 'src-tauri', 'target', 'release', 'rhizomind.exe');
if (!existsSync(exe)) {
  console.error(`Нет ${exe}: сначала tauri build --no-bundle`);
  process.exit(1);
}

const arch = process.arch === 'arm64' ? 'arm64' : 'x64';
const outDir = path.join(root, 'release');
const zip = path.join(outDir, `rhizomind-${version}-windows-${arch}-portable.zip`);
mkdirSync(outDir, { recursive: true });
rmSync(zip, { force: true });

const staging = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-portable-'));
try {
  copyFileSync(exe, path.join(staging, 'rhizomind.exe'));
  copyFileSync(path.join(root, 'LICENSE'), path.join(staging, 'LICENSE.txt'));
  execFileSync('tar.exe', ['-a', '-c', '-f', zip, '-C', staging, 'rhizomind.exe', 'LICENSE.txt'], { stdio: 'inherit' });
} finally {
  rmSync(staging, { recursive: true, force: true });
}
console.log(`package:portable — ${path.relative(root, zip)}`);
