// Тест следов и флагов на ВЫПУСКНОЙ сборке (Windows; план 0.1, задача 0.3,
// ADR 0007). Без WebDriver: msedgedriver подменил бы папку данных.
//
// Снимки %LOCALAPPDATA%, %APPDATA%, %TEMP% до запуска, во время работы и после
// выхода. Во время работы вне хранилища появляется только временная папка
// WebView2 `rhizomind-webview-<pid>-<случайное>` (vaults.json — с задачи 1.3,
// пока его нет); после выхода её нет. Заодно: остатки прошлых запусков
// убираются, переменные WEBVIEW2_* не доходят до движка, процесс браузера
// запущен с защищёнными флагами.
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { applicationPath } from './driver.ts';

const isWindows = process.platform === 'win32';
const env = (name: string): string => {
  const value = process.env[name];
  if (!value) throw new Error(`нет переменной ${name}`);
  return value;
};

// --- снимки файловой системы ---------------------------------------------

type Snapshot = Map<string, string>;

function snapshot(roots: string[]): Snapshot {
  const result: Snapshot = new Map();
  const walk = (dir: string) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return; // нет прав — пропускаем (системные папки)
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) {
        result.set(full, 'link');
      } else if (entry.isDirectory()) {
        result.set(full + path.sep, 'dir');
        walk(full);
      } else {
        try {
          const st = lstatSync(full);
          result.set(full, `${st.size}:${st.mtimeMs}`);
        } catch {
          // файл исчез во время обхода
        }
      }
    }
  };
  for (const root of new Set(roots)) walk(root);
  return result;
}

/** Новые и изменённые пути относительно `before` (удалённые — отдельно). */
function diff(before: Snapshot, after: Snapshot): { changed: string[]; removed: string[] } {
  const changed = [...after].filter(([p, v]) => before.get(p) !== v && !(v === 'dir' && before.has(p))).map(([p]) => p);
  const removed = [...before.keys()].filter((p) => !after.has(p));
  return { changed, removed };
}

// Пишут сами инструменты теста, а не приложение: PowerShell (список
// процессов, закрытие окна) ведёт кэш модулей и профиль запуска
// (%LOCALAPPDATA%\Microsoft\Windows\PowerShell, %LOCALAPPDATA%\Microsoft\PowerShell).
const TEST_HARNESS_PATHS = [
  path.join('Microsoft', 'Windows', 'PowerShell') + path.sep,
  path.join('AppData', 'Local', 'Microsoft', 'PowerShell') + path.sep,
];

// Точные пути, которые меняет PowerShell без приложения (например, список
// переходов PowerShell в Recent\CustomDestinations): калибровка до запуска.
const calibratedNoise = new Set<string>();

const isHarnessNoise = (p: string) => calibratedNoise.has(p) || TEST_HARNESS_PATHS.some((h) => p.includes(h));

/** Калибровка: что меняют вызовы PowerShell самого теста, пока приложение не запущено. */
function calibrateHarnessNoise(roots: string[]): string[] {
  const start = snapshot(roots);
  processes();
  execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', `(Get-Process -Id ${process.pid}) | Out-Null`]);
  const { changed } = diff(start, snapshot(roots));
  for (const p of changed) calibratedNoise.add(p);
  return changed;
}

// --- процессы --------------------------------------------------------------

interface ProcessInfo {
  ProcessId: number;
  ParentProcessId: number;
  Name: string;
  CommandLine: string | null;
}

function processes(): ProcessInfo[] {
  const out = execFileSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress',
    ],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
  );
  return JSON.parse(out) as ProcessInfo[];
}

function descendants(root: number, all: ProcessInfo[]): ProcessInfo[] {
  const tree: ProcessInfo[] = [];
  const queue = [root];
  while (queue.length > 0) {
    const parent = queue.shift();
    for (const p of all) {
      if (p.ParentProcessId === parent && !tree.includes(p)) {
        tree.push(p);
        queue.push(p.ProcessId);
      }
    }
  }
  return tree;
}

function portIsOpen(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = connect(port, '127.0.0.1');
    socket.once('connect', () => {
      socket.end();
      resolve(true);
    });
    socket.once('error', () => {
      socket.destroy();
      resolve(false);
    });
  });
}

async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`не дождались: ${what}`);
    await new Promise((r) => setTimeout(r, 500));
  }
}

// --- тест --------------------------------------------------------------------

describe.skipIf(!isWindows)('следы и флаги выпускной сборки (Windows)', () => {
  // TEMP на машинах CI задан коротким именем 8.3 (RUNNER~1): снимки ведутся по
  // полным путям, а командная строка движка — в том виде, что видит приложение.
  const temp = isWindows ? env('TEMP') : '';
  const real = (p: string) => realpathSync.native(p);
  const roots = isWindows ? [env('LOCALAPPDATA'), env('APPDATA'), temp].map(real) : [];
  // Известный локальный след (docs/SECURITY.md, «Известные ограничения»):
  // при закрытии окна появляется новый файл диспетчера учётных данных Windows
  // прямо в %LOCALAPPDATA%\Microsoft\Credentials\. Создатель не установлен
  // (движок WebView2 или код ядра) — расследование в бэклоге (docs/reviews/
  // 2026-10-25-task-0.3.md). Разрешены только НОВЫЕ файлы ровно в этой папке;
  // изменение существующих и любой другой след по-прежнему валят тест.
  const credentialsDir = isWindows ? path.join(real(env('LOCALAPPDATA')), 'Microsoft', 'Credentials') : '';
  const isKnownTrace = (p: string) => isWindows && path.dirname(p) === credentialsDir && !before.has(p);
  const unexpected = (p: string) => !isHarnessNoise(p) && !isKnownTrace(p);
  // Остаток «аварийного» запуска: процесс 4000000000 не существует.
  const leftover = path.join(temp, 'rhizomind-webview-4000000000-0123456789abcdef');
  const evilUserData = path.join(temp, `rhizomind-e2e-evil-udf-${Date.now()}`);
  const DEBUG_PORT = 9333;

  let app: ChildProcess;
  let exitCode: Promise<number | null>;
  let before: Snapshot;
  let during: Snapshot;
  let dataDir = '';
  let browser: ProcessInfo | undefined;
  let engine: ProcessInfo[] = [];

  beforeAll(async () => {
    const exe = applicationPath('release');
    expect(existsSync(exe), `${exe}: нужна выпускная сборка (npm run build)`).toBe(true);
    expect(readFileSync(exe).includes(Buffer.from('ТЕСТОВАЯ СБОРКА (webdriver)', 'utf8'))).toBe(false);

    mkdirSync(path.join(leftover, 'EBWebView'), { recursive: true });
    writeFileSync(path.join(leftover, 'EBWebView', 'Local State'), '{}');
    processes(); // прогрев PowerShell до первого снимка
    const harness = calibrateHarnessNoise(roots);
    console.log(`калибровка: вызовы PowerShell теста без приложения меняют ${harness.length} путей: ${harness.join(', ')}`);
    before = snapshot(roots);

    app = spawn(exe, [], {
      stdio: 'ignore',
      env: {
        ...process.env,
        // Любая из них, дойди она до WebView2, открыла бы порт отладки,
        // перенесла бы папку данных или подменила бы движок.
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${DEBUG_PORT}`,
        WEBVIEW2_USER_DATA_FOLDER: evilUserData,
        WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: 'C:\\rhizomind-no-such-webview2',
      },
    });
    exitCode = new Promise((resolve) => app.once('exit', (code) => resolve(code)));
    const pid = app.pid ?? 0;

    dataDir = await waitFor('временная папка движка', 30_000, () =>
      readdirSync(temp)
        .filter((n) => n.startsWith(`rhizomind-webview-${pid}-`))
        .map((n) => path.join(temp, n))[0],
    );
    browser = await waitFor('процесс браузера WebView2', 30_000, () =>
      processes().find((p) => p.ParentProcessId === pid && p.Name.toLowerCase() === 'msedgewebview2.exe'),
    );
    // Движок поднимает процессы не сразу: даём ему поработать.
    await new Promise((r) => setTimeout(r, 5000));
    engine = descendants(pid, processes());
    during = snapshot(roots);
  }, 120_000);

  afterAll(() => {
    if (app && app.exitCode === null) app.kill();
    rmSync(leftover, { recursive: true, force: true });
    rmSync(evilUserData, { recursive: true, force: true });
  });

  it('при запуске убирает остатки прошлых запусков', () => {
    expect(existsSync(leftover)).toBe(false);
  });

  it('во время работы вне хранилища пишет только временную папку WebView2', () => {
    const { changed } = diff(before, during);
    const own = real(dataDir) + path.sep;
    const outside = changed.filter((p) => !p.startsWith(own));
    const noise = outside.filter(isHarnessNoise);
    console.log(`во время работы: ${changed.length} новых/изменённых путей, вне временной папки: ${outside.length} (из них инструменты теста: ${noise.length})`);
    const known = outside.filter(isKnownTrace);
    if (known.length > 0) console.log(`известный след (SECURITY.md): ${known.join(', ')}`);
    expect(outside.filter(unexpected)).toEqual([]);
    expect(path.dirname(dataDir)).toBe(path.resolve(temp));
  });

  it('переменные WEBVIEW2_* не доходят до движка', async () => {
    expect(await portIsOpen(DEBUG_PORT)).toBe(false);
    expect(existsSync(evilUserData)).toBe(false);
    for (const p of engine.filter((e) => e.Name.toLowerCase() === 'msedgewebview2.exe')) {
      expect(p.CommandLine ?? '').not.toContain('--remote-debugging-port');
      expect(p.CommandLine ?? '').not.toContain(evilUserData);
    }
  });

  it('процесс браузера: временная папка, защищённые флаги, без отправки дампов', () => {
    const cmd = browser?.CommandLine ?? '';
    console.log(`процесс браузера: ${cmd}`);
    expect(cmd).toContain(`--user-data-dir="${dataDir}\\EBWebView"`);
    for (const flag of [
      '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection',
      '--no-proxy-server',
      '--disable-background-networking',
      '--disable-component-update',
      '--disable-domain-reliability',
      '--no-pings',
      '--host-resolver-rules="MAP * ~NOTFOUND"',
      // IsCustomCrashReportingEnabled = true (правка wry, vendor/wry)
      '--edge-webview-disable-crash-reporting=1',
    ]) {
      expect(cmd).toContain(flag);
    }
    expect(cmd).not.toContain('--autoplay-policy');
  });

  it('после выхода временной папки нет и следов вне хранилища не осталось', async () => {
    execFileSync('powershell', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `(Get-Process -Id ${app.pid}).CloseMainWindow() | Out-Null`,
    ]);
    const code = await Promise.race([exitCode, new Promise<'timeout'>((r) => setTimeout(() => r('timeout'), 40_000))]);
    expect(code).toBe(0);

    expect(existsSync(dataDir), `осталась ${dataDir}`).toBe(false);
    const alive = processes().filter((p) => engine.some((e) => e.ProcessId === p.ProcessId && e.Name === p.Name));
    expect(alive.map((p) => `${p.ProcessId} ${p.Name}`)).toEqual([]);

    const after = snapshot(roots);
    const { changed, removed } = diff(before, after);
    const known = changed.filter(isKnownTrace);
    if (known.length > 0) console.log(`известный след после выхода (SECURITY.md): ${known.join(', ')}`);
    expect(changed.filter(unexpected)).toEqual([]);
    const leftoverReal = path.join(real(temp), path.basename(leftover));
    expect(removed.filter((p) => !p.startsWith(leftoverReal))).toEqual([]);
  }, 60_000);
});
