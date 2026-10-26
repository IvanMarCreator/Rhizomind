// Тест следов и флагов на ВЫПУСКНОЙ сборке (Windows; план 0.1, задача 0.3,
// ADR 0007). Без WebDriver: msedgedriver подменил бы папку данных.
//
// Сценарий (задача 0.6б): приложение открывает тестовое хранилище из
// vaults.json, в заметку вводится текст, окно закрывается. Снимки
// %LOCALAPPDATA%, %APPDATA%, %TEMP% до запуска, во время работы и после
// выхода; экспорт реестра HKCU до запуска и после выхода (tools/traces/registry.ts).
// Вне хранилища меняются только временная папка WebView2
// `rhizomind-webview-<pid>-<случайное>` (после выхода её нет), vaults.json и
// учтённые записи Windows. Случайные имена хранилища, заметки и введённого
// текста не должны появиться в реестре нигде. Заодно: остатки прошлых запусков
// убираются, переменные WEBVIEW2_* не доходят до движка, процесс браузера
// запущен с защищёнными флагами.
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { checkRegistryChanges, diffRegistry, findMarkers, parseRegExport, summarize, type RegExport } from '../../tools/traces/registry.ts';
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
  // Журнал загрузки .NET для powershell.exe (UI Automation при вводе текста).
  path.join('Microsoft', 'CLR_v4.0', 'UsageLogs') + path.sep,
  // База кэша WinINet (ESE) — её меняют фоновые задачи Windows и без
  // приложения (пауза без приложения, задача 0.6); Rhizomind и WebView2 WinINet
  // не используют.
  path.join('Microsoft', 'Windows', 'WebCache') + path.sep,
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

// --- реестр HKCU ------------------------------------------------------------

// Файлы экспорта — в test-results/ рабочей копии, не в проверяемых папках профиля.
const REG_DIR = path.join(import.meta.dirname, '..', '..', 'test-results', 'traces');

function registrySnapshot(label: string): RegExport {
  mkdirSync(REG_DIR, { recursive: true });
  const file = path.join(REG_DIR, `hkcu-${label}.reg`);
  execFileSync('reg', ['export', 'HKCU', file, '/y'], { stdio: 'ignore' });
  return parseRegExport(readFileSync(file).toString('utf16le'));
}

// --- ввод в окно приложения -------------------------------------------------

// Выпускная сборка без WebDriver: заметку открываем быстрым переходом (Ctrl+O),
// фокус в редактор ставим через UI Automation (у элемента CodeMirror класс
// cm-content), текст вводим SendKeys. Только латиница, цифры и дефис — без
// служебных символов SendKeys.
function typeIntoNote(pid: number, search: string, text: string): string {
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$ws = New-Object -ComObject WScript.Shell
function Find-Editor {
  $p = Get-Process -Id ${pid}
  if ($p.MainWindowHandle -eq 0) { return $null }
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
  $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($e in $all) { if ($e.Current.ClassName -like '*cm-content*') { return $e } }
  return $null
}
$editor = $null
for ($attempt = 1; $attempt -le 3 -and -not $editor; $attempt++) {
  for ($i = 0; $i -lt 40 -and -not $ws.AppActivate(${pid}); $i++) { Start-Sleep -Milliseconds 250 }
  Start-Sleep -Milliseconds (800 * $attempt)
  $ws.SendKeys('{ESC}'); Start-Sleep -Milliseconds 300
  $ws.SendKeys('^o'); Start-Sleep -Milliseconds 1500
  $ws.SendKeys('${search}'); Start-Sleep -Milliseconds 1500
  $ws.SendKeys('{ENTER}')
  for ($i = 0; $i -lt 12 -and -not $editor; $i++) { Start-Sleep -Milliseconds 500; $editor = Find-Editor }
}
if (-not $editor) {
  $p = Get-Process -Id ${pid}
  "нет редактора; окно: '$($p.MainWindowTitle)'"
  exit 0
}
$editor.SetFocus(); Start-Sleep -Milliseconds 500
$ws.SendKeys('^{END}'); $ws.SendKeys(' ${text}'); Start-Sleep -Milliseconds 3000
'введено'
`;
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' }).trim();
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
  // Учтённые записи вне хранилища (docs/SECURITY.md, «Записи вне хранилища»;
  // задача 0.6, docs/research/2026-10-26-traces-investigation.md):
  // - %LOCALAPPDATA%\Microsoft\Credentials\DFBE70A7E5CC19A398EBF1B96859CE5D —
  //   запись Windows `virtualapp/didlogical` (служба wlidsvc), не Rhizomind:
  //   на машинах CI Windows иногда создаёт её позже обычного. Допускается
  //   ровно этот новый файл, любой другой файл в папке валит тест.
  // - %LOCALAPPDATA%\Microsoft\TokenBroker\Cache\*.tbres — кэш брокера токенов
  //   Windows: фон Windows (плановое задание GAEvents) и OneAuth движка WebView2
  //   при первом запуске; решение 60 в работе. Новые, изменённые и удалённые файлы.
  // - %LOCALAPPDATA%\Rhizomind\vaults.json — список хранилищ (решение 46);
  //   тест кладёт его до запуска и восстанавливает после.
  const localAppData = isWindows ? real(env('LOCALAPPDATA')) : '';
  const credentialsFile = isWindows ? path.join(localAppData, 'Microsoft', 'Credentials', 'DFBE70A7E5CC19A398EBF1B96859CE5D') : '';
  const tokenBrokerCache = isWindows ? path.join(localAppData, 'Microsoft', 'TokenBroker', 'Cache') + path.sep : '';
  const profileDir = isWindows ? path.join(localAppData, 'Rhizomind') : '';
  const vaultsFile = path.join(profileDir, 'vaults.json');
  const isKnownTrace = (p: string) =>
    isWindows &&
    ((p === credentialsFile && !before.has(p)) ||
      (p.startsWith(tokenBrokerCache) && !p.slice(tokenBrokerCache.length).includes(path.sep) && p.endsWith('.tbres')) ||
      p === vaultsFile);
  const unexpected = (p: string) => !isHarnessNoise(p) && !isKnownTrace(p);

  // Тестовое хранилище с одной заметкой (вне проверяемых папок профиля).
  // Случайные имена без слова «rhizomind» — метки данных: их не должно быть
  // в реестре нигде, кроме самого хранилища и vaults.json.
  const hex = () => randomBytes(4).toString('hex');
  const vaultName = `trace-vault-${hex()}`;
  const noteKey = hex();
  const noteName = `заметка-${noteKey}`;
  const typedText = `trace-text-${hex()}`;
  const vaultDir = path.join(REG_DIR, vaultName);
  const noteFile = path.join(vaultDir, `${noteName}.md`);
  let savedVaults: Buffer | undefined;
  let createdProfileDir = false;
  let typing = '';
  // Остаток «аварийного» запуска: процесс 4000000000 не существует.
  const leftover = path.join(temp, 'rhizomind-webview-4000000000-0123456789abcdef');
  const evilUserData = path.join(temp, `rhizomind-e2e-evil-udf-${Date.now()}`);
  const DEBUG_PORT = 9333;

  let app: ChildProcess;
  let exitCode: Promise<number | null>;
  let before: Snapshot;
  let registryBefore: RegExport;
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
    // Хранилище и список хранилищ: приложение откроет его при запуске (последнее открытое).
    mkdirSync(vaultDir, { recursive: true });
    writeFileSync(noteFile, 'Заметка теста следов.\n');
    if (existsSync(vaultsFile)) savedVaults = readFileSync(vaultsFile);
    if (!existsSync(profileDir)) {
      mkdirSync(profileDir, { recursive: true });
      createdProfileDir = true;
    }
    const vaultId = randomBytes(8).toString('hex');
    writeFileSync(
      vaultsFile,
      JSON.stringify({ format_version: 1, vaults: [{ id: vaultId, name: vaultName, path: real(vaultDir) }], last_opened: vaultId }, null, 2),
    );

    processes(); // прогрев PowerShell до первого снимка
    const harness = calibrateHarnessNoise(roots);
    console.log(`калибровка: вызовы PowerShell теста без приложения меняют ${harness.length} путей: ${harness.join(', ')}`);
    registryBefore = registrySnapshot('before');
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
    typing = typeIntoNote(pid, noteKey, typedText);
    console.log(`ввод в заметку: ${typing}`);
    engine = descendants(pid, processes());
    during = snapshot(roots);
  }, 120_000);

  afterAll(() => {
    if (app && app.exitCode === null) app.kill();
    rmSync(leftover, { recursive: true, force: true });
    rmSync(evilUserData, { recursive: true, force: true });
    if (isWindows) {
      if (savedVaults) writeFileSync(vaultsFile, savedVaults);
      else rmSync(vaultsFile, { force: true });
      if (createdProfileDir) rmSync(profileDir, { recursive: true, force: true });
    }
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
    if (known.length > 0) console.log(`учтённая запись (SECURITY.md, «Записи вне хранилища»): ${known.join(', ')}`);
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
    if (known.length > 0) console.log(`учтённая запись после выхода (SECURITY.md, «Записи вне хранилища»): ${known.join(', ')}`);
    expect(changed.filter(unexpected)).toEqual([]);
    // Удалены могут быть только остатки папок движка прошлых запусков —
    // подложенный тестом и оставшиеся от сценарных тестов WebDriver, чьи
    // процессы драйвер завершил без выхода (уборка остатков при запуске, ADR 0007).
    // Имя папки — точно `rhizomind-webview-<номер процесса>-<16 шестнадцатеричных>`
    // (src-tauri/src/webview_traces/), и учтённые записи (кэш TokenBroker).
    const tempReal = real(temp) + path.sep;
    const LEFTOVER_NAME = /^rhizomind-webview-\d+-[0-9a-f]{16}$/;
    const isLeftover = (p: string) =>
      p.startsWith(tempReal) && LEFTOVER_NAME.test(p.slice(tempReal.length).split(path.sep)[0] ?? '');
    expect(removed.filter((p) => !isLeftover(p) && !isKnownTrace(p))).toEqual([]);
    expect(existsSync(leftover)).toBe(false);
    // Сценарий дошёл до конца: введённый текст записан в заметку до выхода.
    expect(typing).toBe('введено');
    expect(readFileSync(noteFile, 'utf8')).toContain(typedText);
  }, 60_000);

  it('реестр HKCU: нет путей и упоминаний приложения вне учтённых записей', () => {
    const registryAfter = registrySnapshot('after');
    const changes = diffRegistry(registryBefore, registryAfter);
    const data = [vaultName, noteName, typedText];
    const markers = { app: ['rhizomind', path.basename(dataDir), path.basename(evilUserData)], data };
    const { violations, known, background } = checkRegistryChanges(changes, markers);
    console.log(`реестр HKCU: ${changes.length} изменений; учтённые: ${known.length}; фон ОС: ${background.length}`);
    if (known.length > 0) console.log(`учтённые записи: ${[...new Set(known.map((k) => `${k.why}: ${k.key}`))].join('; ')}`);
    if (background.length > 0) console.log(`фон ОС по ключам: ${summarize(background)}`);
    expect(violations.map((v) => `${v.kind} ${v.key} ${v.name ?? ''} ${(v.data ?? '').slice(0, 120)}`)).toEqual([]);
    // Метки данных — по всему экспорту «после», не только в изменениях.
    expect(findMarkers(registryAfter, data).map((h) => `${h.marker}: ${h.key} ${h.name ?? ''}`)).toEqual([]);
  }, 60_000);
});
