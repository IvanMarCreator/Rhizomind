// Выход из программы без OneAuth (решение владельца 64, задача 0.7): при
// закрытии последнего окна ядро записывает ввод, завершает процесс браузера
// WebView2 (`TerminateProcess` по `BrowserProcessId`) и только потом
// закрывает окно. При штатном закрытии движок загружает свой модуль OneAuth
// (`oneauth.dll`), и Windows обращается к службе входа Microsoft
// (docs/research/2026-10-26-traces-investigation.md, «Дополнение 0.6б»).
//
// Облегчённая проверка для основной CI (полная — задание `oneauth-kill-first`
// в tools/traces-investigation/): наблюдатель PowerShell опрашивает модули
// процесса браузера, пока программа не завершится. Контрольный прогон с
// RHIZOMIND_E2E_KEEP_BROWSER (штатное закрытие, только тестовая сборка)
// доказывает, что наблюдатель загрузку OneAuth видит. Ещё проверяется, что
// дампов падения нет и временная папка движка удалена.
//
// Без WebDriver: программа запускается напрямую, как ярлыком (msedgedriver
// подменяет папку данных и сам управляет процессом браузера).
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { applicationPath } from './driver.ts';

const isWindows = process.platform === 'win32';

function powershell(command: string): string {
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8' }).trim();
}

async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`не дождались: ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 300));
  }
}

function exitOf(child: ChildProcess, timeoutMs: number): Promise<number | null | 'timeout'> {
  return new Promise((resolve) => {
    if (child.exitCode !== null) {
      resolve(child.exitCode);
      return;
    }
    const timer = setTimeout(() => resolve('timeout'), timeoutMs);
    child.once('exit', (code) => {
      clearTimeout(timer);
      resolve(code);
    });
  });
}

/**
 * Наблюдатель: находит процесс браузера WebView2 (дочерний `msedgewebview2`
 * программы) и опрашивает его модули, пока программа работает. Печатает
 * `BROWSER <pid>`, `LOADED`, если увидел `oneauth.dll`, и `DONE` в конце.
 */
const WATCHER = `
param([int]$App)
$browser = $null
for ($i = 0; $i -lt 200 -and -not $browser; $i++) {
  $browser = Get-CimInstance Win32_Process -Filter "ParentProcessId=$App AND Name='msedgewebview2.exe'" | Select-Object -First 1
  if (-not $browser) { Start-Sleep -Milliseconds 100 }
}
if (-not $browser) { 'NOBROWSER'; exit }
"BROWSER $($browser.ProcessId)"
[Console]::Out.Flush()
$loaded = $false
while (Get-Process -Id $App -ErrorAction SilentlyContinue) {
  $p = Get-Process -Id $browser.ProcessId -ErrorAction SilentlyContinue
  if (-not $p) { break }
  try {
    if (-not $loaded -and ($p.Modules | Where-Object { $_.ModuleName -ieq 'oneauth.dll' })) {
      $loaded = $true
      'LOADED'
      [Console]::Out.Flush()
    }
  } catch {}
  Start-Sleep -Milliseconds 20
}
'DONE'
`;

/** Файлы отчётов об ошибках Windows (WER, локальные дампы) — для сравнения. */
function crashReports(): string[] {
  const local = process.env.LOCALAPPDATA ?? '';
  const dirs = [
    path.join(local, 'CrashDumps'),
    path.join(local, 'Microsoft', 'Windows', 'WER', 'ReportQueue'),
    path.join(local, 'Microsoft', 'Windows', 'WER', 'ReportArchive'),
  ];
  return dirs.flatMap((dir) => (existsSync(dir) ? readdirSync(dir).map((name) => path.join(dir, name)) : []));
}

interface Run {
  output: string;
  exit: number | null | 'timeout';
  newReports: string[];
  tempLeft: string[];
}

const scratch = isWindows ? mkdtempSync(path.join(os.tmpdir(), 'rhizomind-browser-exit-')) : '';

async function runOnce(name: string, keepBrowser: boolean): Promise<Run> {
  const root = path.join(scratch, name);
  const vault = path.join(root, 'Хранилище');
  const profile = path.join(root, 'profile');
  const temp = path.join(root, 'temp');
  for (const dir of [vault, profile, temp]) mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(vault, 'Заметка.md'), 'текст\n');
  const watcherScript = path.join(root, 'watch.ps1');
  writeFileSync(watcherScript, WATCHER);

  const env: NodeJS.ProcessEnv = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith('WEBVIEW2_') && !key.startsWith('RHIZOMIND_E2E_')),
  );
  env.RHIZOMIND_E2E_PROFILE_DIR = profile;
  env.RHIZOMIND_E2E_VAULT = vault;
  env.TEMP = temp;
  env.TMP = temp;
  if (keepBrowser) env.RHIZOMIND_E2E_KEEP_BROWSER = '1';

  const reportsBefore = new Set(crashReports());
  const app = spawn(applicationPath('webdriver'), [], { stdio: 'ignore', env });
  const pid = app.pid ?? 0;
  try {
    await waitFor('окно программы', 60_000, () => {
      const handle = powershell(`(Get-Process -Id ${pid}).MainWindowHandle`);
      return handle !== '' && handle !== '0' ? handle : undefined;
    });
    // Окно показано, хранилище открыто: дать движку дорисовать страницу.
    await new Promise((resolve) => setTimeout(resolve, 3000));

    let output = '';
    const watcher = spawn(
      'powershell',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', watcherScript, '-App', String(pid)],
      { stdio: ['ignore', 'pipe', 'inherit'] },
    );
    watcher.stdout?.on('data', (chunk: Buffer) => (output += chunk.toString()));
    await waitFor('наблюдатель нашёл браузер', 30_000, () => (/BROWSER|NOBROWSER/.test(output) ? true : undefined));

    expect(powershell(`(Get-Process -Id ${pid}).CloseMainWindow()`)).toBe('True');
    const exit = await exitOf(app, 60_000);
    await exitOf(watcher, 30_000);
    return {
      output,
      exit,
      newReports: crashReports().filter((file) => !reportsBefore.has(file) && /msedgewebview2|rhizomind/i.test(file)),
      tempLeft: readdirSync(temp).filter((entry) => entry.startsWith('rhizomind-webview-')),
    };
  } finally {
    if (app.exitCode === null) app.kill();
  }
}

describe.skipIf(!isWindows)('выход из программы: процесс браузера завершается до закрытия окна (решение 64)', () => {
  afterAll(() => {
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  it('контроль: при штатном закрытии наблюдатель видит загрузку oneauth.dll', async () => {
    const run = await runOnce('control', true);
    expect(run.output).toContain('BROWSER');
    expect(run.output).toContain('LOADED');
  }, 180_000);

  it('oneauth.dll не загружается, дампов падения нет, временная папка движка удалена', async () => {
    const run = await runOnce('terminated', false);
    expect(run.output).toContain('BROWSER');
    expect(run.output).not.toContain('LOADED');
    expect(run.exit).not.toBe('timeout');
    expect(run.newReports).toEqual([]);
    expect(run.tempLeft).toEqual([]);
  }, 180_000);
});
