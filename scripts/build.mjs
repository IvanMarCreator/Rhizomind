// Сборка приложения в dist/: ядро (main, ESM), мост (preload, CommonJS —
// требование песочницы Electron) через esbuild и интерфейс через Vite.
import { build as esbuild } from 'esbuild';
import { rm } from 'node:fs/promises';
import path from 'node:path';
import { build as vite } from 'vite';

const ROOT = path.resolve(import.meta.dirname, '..');
const DIST = path.join(ROOT, 'dist');

await rm(DIST, { recursive: true, force: true });

const common = {
  bundle: true,
  platform: 'node',
  target: 'node24',
  external: ['electron'],
  sourcemap: false,
  logLevel: 'info',
};

await esbuild({
  ...common,
  entryPoints: [path.join(ROOT, 'src/main/main.ts')],
  outfile: path.join(DIST, 'main/main.js'),
  format: 'esm',
});

await esbuild({
  ...common,
  entryPoints: [path.join(ROOT, 'src/preload/preload.ts')],
  outfile: path.join(DIST, 'preload/preload.cjs'),
  format: 'cjs',
});

await vite({ root: path.join(ROOT, 'src/renderer'), configFile: path.join(ROOT, 'vite.config.ts') });
