// Выход из системы и выключение Windows (задача 0.11, инвариант 4): ввод
// записывается до того, как процесс завершится.
//
// Windows спрашивает каждое окно верхнего уровня `WM_QUERYENDSESSION`, потом
// шлёт `WM_ENDSESSION`; tao на `WM_ENDSESSION` сразу вызывает `exit(0)`.
// Тест повторяет это для окон процесса программы: помощник PowerShell (C# через
// Add-Type, компилируется заранее) по команде шлёт `WM_QUERYENDSESSION` всем
// окнам процесса, затем `WM_ENDSESSION(TRUE)` — как при выходе из системы.
//
// 1. Через WebDriver: ввод → сразу конец сеанса → файл содержит ввод.
//    «Медленный диск» (пауза 1 с перед каждой записью) делает проверку
//    надёжной: без задачи 0.11 процесс завершается раньше записи.
// 2. Без WebDriver (как tests/e2e/browser-exit.test.ts): процесс браузера
//    завершается без загрузки oneauth.dll (решение 64), временная папка
//    движка удалена. Дважды: `WM_ENDSESSION` окнам в порядке перечисления и
//    в обратном (ревью 0.11, п. 2) — порядок рассылки решает Windows; если
//    первым известили окно tao, браузер завершается в `RunEvent::Exit`.
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, afterEach, describe, expect, it } from 'vitest';
import { applicationPath, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';

const isWindows = process.platform === 'win32';

const NOTE = 'Заметка';
const TYPED = 'typed-right-before-logoff-0123456789';

/**
 * Программа ждёт записи до 10 с (решение 78), но при записи за ~1 с отвечает
 * раньше, чем Windows покажет экран со списком программ (~5 с).
 */
const QUERY_LIMIT_MS = 5_000;

function powershell(command: string): string {
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8' }).trim();
}

/**
 * Помощник: печатает `READY`, ждёт строку на входе и шлёт окнам процесса
 * `$App` конец сеанса (`-Reverse` — `WM_ENDSESSION` в обратном порядке окон).
 * Печатает `QUERY <мс> <ответы через запятую> WINDOWS <n>`.
 */
const SESSION_END = `
param([int]$App, [switch]$Reverse)
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class RhizomindSessionEnd {
  delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr lParam);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
  const uint WM_QUERYENDSESSION = 0x11;
  const uint WM_ENDSESSION = 0x16;
  public static string Run(uint app, bool reverse) {
    var windows = new List<IntPtr>();
    EnumProc collect = (h, l) => { uint pid; GetWindowThreadProcessId(h, out pid); if (pid == app) windows.Add(h); return true; };
    EnumWindows(collect, IntPtr.Zero);
    var logoff = new IntPtr(0x80000000L);
    var answers = new List<string>();
    var watch = Stopwatch.StartNew();
    foreach (var h in windows) {
      IntPtr r;
      var sent = SendMessageTimeoutW(h, WM_QUERYENDSESSION, IntPtr.Zero, logoff, 0, 10000, out r);
      answers.Add(sent == IntPtr.Zero ? "fail" : r.ToString());
    }
    var ms = watch.ElapsedMilliseconds;
    if (reverse) windows.Reverse();
    foreach (var h in windows) {
      IntPtr r;
      SendMessageTimeoutW(h, WM_ENDSESSION, new IntPtr(1), logoff, 0, 30000, out r);
    }
    return "QUERY " + ms + " " + string.Join(",", answers) + " WINDOWS " + windows.Count;
  }
}
'@
'READY'
[Console]::Out.Flush()
$null = [Console]::In.ReadLine()
[RhizomindSessionEnd]::Run([uint32]$App, [bool]$Reverse)
`;

interface Helper {
  /** Послать конец сеанса; вернуть строку `QUERY …`. */
  endSession: () => Promise<string>;
  child: ChildProcess;
}

/** Запускает помощника для процесса `pid` и ждёт, пока он скомпилируется. */
async function startHelper(dir: string, pid: number, reverse = false): Promise<Helper> {
  const script = path.join(dir, 'session-end.ps1');
  writeFileSync(script, SESSION_END);
  const args = ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, '-App', String(pid)];
  if (reverse) args.push('-Reverse');
  const child = spawn('powershell', args, { stdio: ['pipe', 'pipe', 'inherit'] });
  let output = '';
  child.stdout?.on('data', (chunk: Buffer) => (output += chunk.toString()));
  await waitFor('помощник готов', 60_000, () => (output.includes('READY') ? true : undefined));
  return {
    child,
    endSession: async () => {
      child.stdin?.write('go\n');
      return waitFor('помощник отправил конец сеанса', 60_000, () => /QUERY .*WINDOWS \d+/.exec(output)?.[0]);
    },
  };
}

async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`не дождались: ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

function isRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitForExit(pid: number, timeoutMs: number): Promise<void> {
  await waitFor(`выход процесса ${pid}`, timeoutMs, () => (isRunning(pid) ? undefined : true));
}

/** Разбор строки помощника: время ответа и ответы окон. */
function parseQuery(line: string): { ms: number; answers: string[]; windows: number } {
  const [, ms, answers, windows] = /QUERY (\d+) (\S*) WINDOWS (\d+)/.exec(line) ?? [];
  return { ms: Number(ms), answers: (answers ?? '').split(',').filter(Boolean), windows: Number(windows) };
}

/** Процесс тестовой сборки (его запускает драйвер движка, а не тест). */
function appPid(): number {
  const exe = applicationPath('webdriver');
  const out = powershell(
    `@(Get-Process -Name rhizomind -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq '${exe.replace(/'/g, "''")}' } | ForEach-Object { $_.Id }) -join ','`,
  );
  const pids = out === '' ? [] : out.split(',').map(Number);
  expect(pids, 'процесс программы один').toHaveLength(1);
  return pids[0] as number;
}

describe.skipIf(!isWindows)('выход из системы Windows: ввод не теряется (задача 0.11)', () => {
  let session: Session | undefined;
  let vault = '';
  let helper: Helper | undefined;
  const scratch = isWindows ? mkdtempSync(path.join(os.tmpdir(), 'rhizomind-session-end-')) : '';

  afterEach(async () => {
    helper?.child.kill();
    helper = undefined;
    await session?.stop().catch(() => undefined);
    session = undefined;
    if (vault) removeTestVault(vault);
    vault = '';
  });

  afterAll(() => {
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  it('ввод → сразу выход из системы → после выхода процесса файл содержит ввод', async () => {
    vault = createTestVault('Конец сеанса', { [`${NOTE}.md`]: 'начало\n' });
    session = await startApp({ vault, slowWriteMs: 1_000 });
    const pid = appPid();
    helper = await startHelper(scratch, pid);

    const { browser } = session;
    const row = browser.$(`[role="treeitem"][aria-label="${NOTE}"]`);
    await row.waitForExist({ timeout: 30_000 });
    await row.click();
    const text = browser.$('[role="textbox"][aria-label="Текст заметки"]');
    await text.waitForExist({ timeout: 10_000 });
    await text.click();
    await browser.keys(['Control', 'End']);
    await browser.keys(TYPED);

    const query = parseQuery(await helper.endSession());
    await waitForExit(pid, 30_000);

    expect(query.windows).toBeGreaterThan(0);
    expect(query.answers.every((answer) => answer === '1'), `ответы: ${query.answers.join(',')}`).toBe(true);
    expect(query.ms).toBeLessThan(QUERY_LIMIT_MS);
    expect(readFileSync(path.join(vault, `${NOTE}.md`), 'utf8')).toContain(TYPED);
  }, 180_000);

  it.each([
    { order: 'прямой', reverse: false },
    { order: 'обратный', reverse: true },
  ])('браузер завершается без oneauth.dll, временная папка движка удалена ($order порядок WM_ENDSESSION)', async ({
    reverse,
  }) => {
    const root = path.join(scratch, reverse ? 'direct-reverse' : 'direct');
    vault = createTestVault('Конец сеанса без драйвера', { [`${NOTE}.md`]: 'текст\n' });
    const profile = path.join(root, 'profile');
    const temp = path.join(root, 'temp');
    for (const dir of [profile, temp]) mkdirSync(dir, { recursive: true });
    const env: NodeJS.ProcessEnv = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith('WEBVIEW2_') && !key.startsWith('RHIZOMIND_E2E_')),
    );
    env.RHIZOMIND_E2E_PROFILE_DIR = profile;
    env.RHIZOMIND_E2E_VAULT = vault;
    env.TEMP = temp;
    env.TMP = temp;

    const app = spawn(applicationPath('webdriver'), [], { stdio: 'ignore', env });
    const pid = app.pid ?? 0;
    let watcher: ChildProcess | undefined;
    try {
      await waitFor('окно программы', 60_000, () => {
        const handle = powershell(`(Get-Process -Id ${pid}).MainWindowHandle`);
        return handle !== '' && handle !== '0' ? handle : undefined;
      });
      await new Promise((resolve) => setTimeout(resolve, 3000));
      helper = await startHelper(root, pid, reverse);

      // Наблюдатель модулей браузера — тот же, что в browser-exit.test.ts.
      const watcherScript = path.join(root, 'watch.ps1');
      writeFileSync(watcherScript, WATCHER);
      let output = '';
      watcher = spawn(
        'powershell',
        ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', watcherScript, '-App', String(pid)],
        { stdio: ['ignore', 'pipe', 'inherit'] },
      );
      watcher.stdout?.on('data', (chunk: Buffer) => (output += chunk.toString()));
      await waitFor('наблюдатель нашёл браузер', 30_000, () => (/BROWSER|NOBROWSER/.test(output) ? true : undefined));

      const query = parseQuery(await helper.endSession());
      await waitForExit(pid, 60_000);
      await waitFor('наблюдатель закончил', 30_000, () => (output.includes('DONE') ? true : undefined));

      expect(query.answers.every((answer) => answer === '1'), `ответы: ${query.answers.join(',')}`).toBe(true);
      expect(query.ms).toBeLessThan(QUERY_LIMIT_MS);
      expect(output).toContain('BROWSER');
      expect(output).not.toContain('LOADED');
      expect(readdirSync(temp).filter((entry) => entry.startsWith('rhizomind-webview-'))).toEqual([]);
    } finally {
      watcher?.kill();
      if (app.exitCode === null) app.kill();
    }
  }, 180_000);
});

/** Наблюдатель модулей процесса браузера (как в browser-exit.test.ts). */
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
