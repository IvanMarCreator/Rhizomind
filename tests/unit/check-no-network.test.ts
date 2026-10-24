import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { scanDirectory } from '../../scripts/check-no-network.mjs';

const run = promisify(execFile);
const SCRIPT = path.resolve(import.meta.dirname, '../../scripts/check-no-network.mjs');

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(path.join(tmpdir(), 'rhizomind-net-'));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

async function put(rel: string, text: string): Promise<void> {
  const file = path.join(dir, rel);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, text);
}

describe('scanDirectory', () => {
  it('ничего не находит в коде без сети', async () => {
    await put('main/main.ts', "import { app } from 'electron';\nimport path from 'node:path';\nconst network = 1;\n");
    await put('renderer/App.svelte', '<h1>Rhizomind</h1>\n');

    expect(await scanDirectory(dir)).toEqual([]);
  });

  it.each([
    ["import http from 'node:http';", 'http'],
    ["import https from 'https';", 'https'],
    ["import { createServer } from 'net';", 'net'],
    ["const net = require('node:net');", 'net'],
    ["import { app, net } from 'electron';", 'electron.net'],
    ["const r = electron.net.request('x');", 'electron.net'],
    ["await fetch('https://example.com');", 'fetch('],
    ['const x = new XMLHttpRequest();', 'XMLHttpRequest'],
    ["const s = new WebSocket('wss://x');", 'WebSocket'],
    ["const e = new EventSource('/x');", 'EventSource'],
    ["navigator.sendBeacon('/x', data);", 'navigator.sendBeacon'],
    ["import { autoUpdater } from 'electron';", 'autoUpdater'],
    ['crashReporter.start({ uploadToServer: false });', 'crashReporter'],
  ])('находит «%s» как %s', async (code, kind) => {
    await put('main/bad.ts', `// начало\n${code}\n`);

    const found = await scanDirectory(dir);

    expect(found).toContainEqual({ file: path.join('main', 'bad.ts'), line: 2, kind });
  });

  it('проверяет .svelte, .html и вложенные папки', async () => {
    await put('renderer/deep/Widget.svelte', '<script>\n  fetch("/x");\n</script>\n');
    await put('renderer/index.html', '<script>new WebSocket("ws://x")</script>\n');

    const found = await scanDirectory(dir);

    expect(found).toEqual(
      expect.arrayContaining([
        { file: path.join('renderer', 'deep', 'Widget.svelte'), line: 2, kind: 'fetch(' },
        { file: path.join('renderer', 'index.html'), line: 1, kind: 'WebSocket' },
      ]),
    );
  });
});

describe('командная строка', () => {
  it('завершается с кодом 0, если сети нет', async () => {
    await put('main/main.ts', "import { app } from 'electron';\n");

    const { stdout } = await run(process.execPath, [SCRIPT, dir]);

    expect(stdout).toContain('сетевой код не найден');
  });

  it('падает с понятным сообщением, если сеть найдена', async () => {
    await put('main/main.ts', "import { net } from 'electron';\n");

    const failure = await run(process.execPath, [SCRIPT, dir]).then(
      () => null,
      (error: { code: number; stderr: string }) => error,
    );

    expect(failure?.code).toBe(1);
    expect(failure?.stderr).toContain('Найден сетевой код');
    expect(failure?.stderr).toContain(`${path.join('main', 'main.ts')}:1`);
    expect(failure?.stderr).toContain('electron.net');
  });
});
