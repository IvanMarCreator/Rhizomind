// Тест следов и флагов на ВЫПУСКНОЙ сборке (Windows; план 0.1, задача 0.3,
// ADR 0007). Без WebDriver: msedgedriver подменил бы папку данных.
//
// Сценарий (задача 0.6б; с 0.2 — задача 0.2-8.1, С2-4, С2-6, С2-9 п. 8):
// приложение открывает тестовое хранилище без индекса из vaults.json, в
// заметку вводится текст; после сборки индекса (`.rhizomind/index.json`)
// заметка с обратными ссылками переименовывается в дереве с ответом «Обновить
// ссылки» и галочкой «Больше не спрашивать» (настройка становится «Всегда
// обновлять ссылки»); другая заметка переименовывается снаружи (`rename`, как
// `mv`) — ссылки обновляются без вопроса; окно закрывается. Внутри хранилища
// `index.json` и `link-update.json` (во время плана) появляются только в
// `.rhizomind/`, после выхода `.rhizomind/tmp/` пуста и журнала нет. Снимки
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
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  watch,
  writeFileSync,
  type FSWatcher,
} from 'node:fs';
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
];

// Исключения файлового снимка с точными путями (docs/SECURITY.md, строка теста следов):
const LOCAL_APP_DATA = isWindows ? realpathSync.native(env('LOCALAPPDATA')) : '';
const NOISE_FILES: ((p: string) => boolean)[] = [
  // Журнал загрузки .NET для powershell.exe (UI Automation при вводе текста).
  (p) =>
    path.dirname(p) === path.join(LOCAL_APP_DATA, 'Microsoft', 'CLR_v4.0', 'UsageLogs') && /^powershell[^\\]*\.log$/i.test(path.basename(p)),
  // Слепая зона: база кэша WinINet (ESE) — её меняют фоновые задачи Windows и
  // без приложения (задача 0.6); база заблокирована, содержимое не прочесть.
  // Rhizomind и WebView2 WinINet не используют; проверка — вручную Process
  // Monitor, когда появятся внешние ссылки.
  (p) => p.startsWith(path.join(LOCAL_APP_DATA, 'Microsoft', 'Windows', 'WebCache') + path.sep),
  // Файлы куста HKCU\Software\Classes (UsrClass.dat и журналы): меняются при
  // любой записи в эту ветку реестра; сами записи проверяет снимок реестра.
  (p) => path.dirname(p) === path.join(LOCAL_APP_DATA, 'Microsoft', 'Windows') && /^UsrClass\.dat/i.test(path.basename(p)),
];

// Точные пути, которые меняет PowerShell без приложения (например, список
// переходов PowerShell в Recent\CustomDestinations): калибровка до запуска.
const calibratedNoise = new Set<string>();

const isHarnessNoise = (p: string) =>
  calibratedNoise.has(p) || TEST_HARNESS_PATHS.some((h) => p.includes(h)) || NOISE_FILES.some((f) => f(p));

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
  "no-editor; window: '$($p.MainWindowTitle)'"
  exit 0
}
$editor.SetFocus(); Start-Sleep -Milliseconds 500
$ws.SendKeys('^{END}'); $ws.SendKeys(' ${text}'); Start-Sleep -Milliseconds 3000
'typed-ok'
`;
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' }).trim();
}

// Переименование в дереве выпускной сборки (как в relink-app.test.ts, но без
// WebDriver): строку `old` находит UI Automation (у строки дерева имя — её
// aria-label), щелчок мышью по её середине выбирает строку и ставит фокус в
// дерево, F2 — поле «Новое имя», новое имя — SendKeys (только латиница, цифры,
// дефис). В окне вопроса — галочка «Больше не спрашивать» (TogglePattern) и
// «Обновить ссылки» (InvokePattern). Кириллица подписей и имён передаётся в
// PowerShell base64, а не строкой команды.
function renameInTree(pid: number, oldName: string, newName: string): string {
  const args = Buffer.from(
    JSON.stringify({ old: oldName, name: newName, field: 'Новое имя', dontAsk: 'Больше не спрашивать', update: 'Обновить ссылки' }),
    'utf8',
  ).toString('base64');
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Name Mouse -Namespace RhzTrace -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, System.UIntPtr extra);
'@
[RhzTrace.Mouse]::SetProcessDPIAware() | Out-Null
$a = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${args}')) | ConvertFrom-Json
$ws = New-Object -ComObject WScript.Shell
$AE = [System.Windows.Automation.AutomationElement]
$CT = [System.Windows.Automation.ControlType]
function Find-Named($type, $name) {
  $p = Get-Process -Id ${pid}
  if ($p.MainWindowHandle -eq 0) { return $null }
  $root = $AE::FromHandle($p.MainWindowHandle)
  $cond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $type)),
    (New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $name)))
  return $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
}
function Wait-Named($type, $name, $seconds) {
  for ($i = 0; $i -lt ($seconds * 4); $i++) {
    $e = Find-Named $type $name
    if ($e -and $e.Current.IsEnabled) { return $e }
    Start-Sleep -Milliseconds 250
  }
  return $null
}
for ($i = 0; $i -lt 40 -and -not $ws.AppActivate(${pid}); $i++) { Start-Sleep -Milliseconds 250 }
Start-Sleep -Milliseconds 500
$ws.SendKeys('{ESC}'); Start-Sleep -Milliseconds 300
$field = $null
for ($attempt = 1; $attempt -le 3 -and -not $field; $attempt++) {
  $row = Wait-Named $CT::TreeItem $a.old 10
  if (-not $row) { continue }
  $r = $row.Current.BoundingRectangle
  [RhzTrace.Mouse]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
  [RhzTrace.Mouse]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [RhzTrace.Mouse]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds (700 * $attempt)
  $ws.SendKeys('{F2}')
  $field = Wait-Named $CT::Edit $a.field 5
}
if (-not $field) { 'no-rename-field'; exit 0 }
$field.SetFocus(); Start-Sleep -Milliseconds 300
$ws.SendKeys('^a'); $ws.SendKeys($a.name); Start-Sleep -Milliseconds 300
$ws.SendKeys('{ENTER}')
# Пока индекс строится, галочка недоступна (решение 114): ждём её доступности.
$check = Wait-Named $CT::CheckBox $a.dontAsk 60
if (-not $check) { 'no-question'; exit 0 }
$toggle = $check.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
if ($toggle.Current.ToggleState -ne 'On') { $toggle.Toggle() }
Start-Sleep -Milliseconds 300
if ($toggle.Current.ToggleState -ne 'On') { 'dont-ask-not-checked'; exit 0 }
$button = Wait-Named $CT::Button $a.update 10
if (-not $button) { 'no-update-button'; exit 0 }
$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
'renamed-ok'
`;
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' }).trim();
}

// --- метки данных в файлах --------------------------------------------------

/** Метки данных в путях и содержимом файлов (UTF-8 и UTF-16): «путь: метка». */
function markersInFiles(paths: string[], markers: string[]): { hits: string[]; unreadable: string[] } {
  const hits: string[] = [];
  const unreadable: string[] = [];
  const needles = markers.map((m) => ({ m, utf8: Buffer.from(m, 'utf8'), utf16: Buffer.from(m, 'utf16le') }));
  for (const p of paths) {
    for (const m of markers) if (p.toLowerCase().includes(m.toLowerCase())) hits.push(`${p}: ${m} (в пути)`);
    if (p.endsWith(path.sep)) continue;
    let bytes: Buffer;
    try {
      if (statSync(p).size > 64 * 1024 * 1024) {
        unreadable.push(`${p} (больше 64 МБ)`);
        continue;
      }
      bytes = readFileSync(p);
    } catch {
      unreadable.push(p); // заблокирован или исчез
      continue;
    }
    for (const { m, utf8, utf16 } of needles) if (bytes.includes(utf8) || bytes.includes(utf16)) hits.push(`${p}: ${m}`);
  }
  return { hits, unreadable };
}

/** Все файлы папки (относительные пути), с вложенными. */
function filesIn(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => !e.isDirectory())
    .map((e) => path.relative(dir, path.join(e.parentPath, e.name)));
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
  // - %LOCALAPPDATA%\Microsoft\TokenBroker\Cache\01c5cb21ab1d4fd56e65159d6c36cd5db1f647e1.tbres —
  //   кэш брокера токенов, который в любой момент создаёт или переписывает
  //   плановое задание Windows GAEvents (taskhostw.exe) и без приложения
  //   (задача 0.6). Допускается только новый или изменённый этот файл.
  //   Всё остальное в TokenBroker\Cache валит тест: OneAuth движка WebView2
  //   пишет другие файлы (e8ddd4cb…, 5a2a7058…, 8ae9f0c3…) и удаляет этот —
  //   с решением 64 (ядро завершает процесс браузера при выходе) их быть не должно.
  // - %LOCALAPPDATA%\Rhizomind\vaults.json — список хранилищ (решение 46);
  //   тест кладёт его до запуска и восстанавливает после.
  // - %LOCALAPPDATA%\D3DSCache\<16 шестнадцатеричных>\<файл> — кэш шейдеров
  //   Direct3D (процесс GPU движка WebView2), без данных хранилища (решение 117,
  //   повторный ручной аудит 01.11.2026). Допускаются только файлы ровно на этой
  //   глубине; кэши драйверов других видеокарт (NVIDIA, AMD) не учтены и валят тест.
  // - %LOCALAPPDATA%\Microsoft\OneAuth\accounts\<шестнадцатеричное имя> — файл
  //   учётной записи, который пишет библиотека OneAuth движка WebView2 (решение 118).
  //   Допускаются только новые и изменённые такие файлы (и сами папки OneAuth,
  //   accounts); любой другой файл в OneAuth и удаление — нарушение.
  const localAppData = isWindows ? real(env('LOCALAPPDATA')) : '';
  const credentialsFile = isWindows ? path.join(localAppData, 'Microsoft', 'Credentials', 'DFBE70A7E5CC19A398EBF1B96859CE5D') : '';
  const backgroundTokenFile = isWindows
    ? path.join(localAppData, 'Microsoft', 'TokenBroker', 'Cache', '01c5cb21ab1d4fd56e65159d6c36cd5db1f647e1.tbres')
    : '';
  const profileDir = isWindows ? path.join(localAppData, 'Rhizomind') : '';
  const vaultsFile = path.join(profileDir, 'vaults.json');
  const shaderCacheDir = isWindows ? path.join(localAppData, 'D3DSCache') : '';
  const isShaderCacheFile = (p: string) =>
    path.dirname(path.dirname(p)) === shaderCacheDir && /^[0-9a-f]{16}$/i.test(path.basename(path.dirname(p))) && !p.endsWith(path.sep);
  const isShaderCacheDir = (p: string) =>
    p === shaderCacheDir + path.sep || (path.dirname(p.slice(0, -1)) === shaderCacheDir && /^[0-9a-f]{16}\\$/i.test(p.slice(shaderCacheDir.length + 1)));
  const oneAuthDir = isWindows ? path.join(localAppData, 'Microsoft', 'OneAuth') : '';
  const oneAuthAccounts = path.join(oneAuthDir, 'accounts');
  const isOneAuthTrace = (p: string) =>
    p === oneAuthDir + path.sep ||
    p === oneAuthAccounts + path.sep ||
    (path.dirname(p) === oneAuthAccounts && /^[0-9a-f]+$/i.test(path.basename(p)));
  const isKnownTrace = (p: string) =>
    isWindows &&
    ((p === credentialsFile && !before.has(p)) ||
      p === backgroundTokenFile ||
      p === vaultsFile ||
      isShaderCacheFile(p) ||
      isShaderCacheDir(p) ||
      isOneAuthTrace(p));
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
  // Сценарий 0.2: «узел» с тегом и заголовком (метки), на него ссылаются две
  // заметки; его переименовывают в дереве. Вторую цель переименовывают снаружи.
  // Новые имена — латиница: их вводит SendKeys.
  const hubName = `узел-${hex()}`;
  const hubNewName = `hub-renamed-${hex()}`;
  const tag = `tag${hex()}`;
  const heading = `Раздел ${hex()}`;
  const extName = `ext-${hex()}`;
  const extNewName = `ext-moved-${hex()}`;
  const referrers = ['Ссылки/Первая.md', 'Вторая.md'];
  const referrerText = (hub: string, ext: string, i: number) => `Заметка ${i}: [[${hub}]], [[${hub}#${heading}|раздел]] и [[${ext}]].\n`;
  const hubText = `# ${heading}\n\nТекст узла #${tag}.\n`;
  // Метки данных: не должны встретиться вне хранилища нигде (файлы, реестр).
  const dataMarkers = [noteName, typedText, hubName, hubNewName, tag, heading, extName, extNewName];
  const serviceDir = path.join(vaultDir, '.rhizomind');
  // События файлов в хранилище (fs.watch): где появлялись index.json и link-update.json.
  const vaultEvents = new Set<string>();
  let vaultWatcher: FSWatcher | undefined;
  let renaming = '';
  // Подмена vaults.json — только при явной переменной (в CI выставлена):
  // на машине владельца тест не трогает его список хранилищ без спроса.
  // Прежний файл сохраняется на диск рядом и восстанавливается в конце или,
  // если процесс прервали, при следующем запуске теста.
  const vaultScenario = process.env.RHIZOMIND_TRACES_VAULT_SCENARIO === '1';
  const vaultsBackup = path.join(profileDir, 'vaults.json.traces-backup');
  let typing = '';

  /** Возвращает прежний vaults.json из копии на диске, если она есть. */
  function restoreVaults(): void {
    if (!isWindows || !existsSync(vaultsBackup)) return;
    const saved = JSON.parse(readFileSync(vaultsBackup, 'utf8')) as { existed: boolean; profileExisted: boolean; content: string };
    if (saved.existed) writeFileSync(vaultsFile, Buffer.from(saved.content, 'base64'));
    else rmSync(vaultsFile, { force: true });
    rmSync(vaultsBackup, { force: true });
    if (!saved.profileExisted) rmSync(profileDir, { recursive: true, force: true });
  }
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
    restoreVaults(); // остаток прерванного прошлого запуска
    if (vaultScenario) {
      // Хранилище и список хранилищ: приложение откроет его при запуске (последнее открытое).
      // Хранилище без индекса: `.rhizomind/` создаст приложение.
      mkdirSync(path.join(vaultDir, 'Ссылки'), { recursive: true });
      writeFileSync(noteFile, 'Заметка теста следов.\n');
      writeFileSync(path.join(vaultDir, `${hubName}.md`), hubText);
      writeFileSync(path.join(vaultDir, `${extName}.md`), `Внешняя цель.\n`);
      referrers.forEach((name, i) => writeFileSync(path.join(vaultDir, name), referrerText(hubName, extName, i)));
      const existed = existsSync(vaultsFile);
      const profileExisted = existsSync(profileDir);
      mkdirSync(profileDir, { recursive: true });
      writeFileSync(
        vaultsBackup,
        JSON.stringify({ existed, profileExisted, content: existed ? readFileSync(vaultsFile).toString('base64') : '' }),
      );
      const vaultId = randomBytes(8).toString('hex');
      writeFileSync(
        vaultsFile,
        JSON.stringify({ format_version: 1, vaults: [{ id: vaultId, name: vaultName, path: real(vaultDir) }], last_opened: vaultId }, null, 2),
      );
    } else {
      console.log('сценарий с тестовым хранилищем пропущен: нет RHIZOMIND_TRACES_VAULT_SCENARIO=1 (vaults.json не подменяется)');
    }

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
    if (vaultScenario) {
      vaultWatcher = watch(vaultDir, { recursive: true }, (_event, name) => {
        if (name) vaultEvents.add(String(name));
      });
    }

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
    if (vaultScenario) {
      typing = typeIntoNote(pid, noteKey, typedText);
      console.log(`ввод в заметку: ${typing}`);
      // Индекс собран и записан: первая запись — не раньше 30 с после открытия.
      const indexFile = path.join(serviceDir, 'index.json');
      await waitFor('сборка индекса (.rhizomind/index.json)', 90_000, () => (existsSync(indexFile) ? true : undefined));
      console.log(`индекс: ${statSync(indexFile).size} байт в .rhizomind/index.json`);

      // Переименование в приложении: «Обновить ссылки» + «Больше не спрашивать».
      renaming = renameInTree(pid, hubName, hubNewName);
      console.log(`переименование в дереве: ${renaming}`);
      await waitFor('ссылки обновлены после переименования в дереве', 30_000, () =>
        existsSync(path.join(vaultDir, `${hubNewName}.md`)) &&
        referrers.every((name, i) => readFileSync(path.join(vaultDir, name), 'utf8') === referrerText(hubNewName, extName, i))
          ? true
          : undefined,
      );
      const settingsFile = path.join(serviceDir, 'settings.json');
      await waitFor('настройка «Всегда обновлять ссылки»', 15_000, () =>
        existsSync(settingsFile) && (JSON.parse(readFileSync(settingsFile, 'utf8')) as { rename_links?: string }).rename_links === 'update'
          ? true
          : undefined,
      );

      // Переименование снаружи (как `mv`) в режиме «Всегда обновлять».
      renameSync(path.join(vaultDir, `${extName}.md`), path.join(vaultDir, `${extNewName}.md`));
      await waitFor('ссылки обновлены после переименования снаружи', 45_000, () =>
        referrers.every((name, i) => readFileSync(path.join(vaultDir, name), 'utf8') === referrerText(hubNewName, extNewName, i))
          ? true
          : undefined,
      );
      console.log('переименование снаружи: ссылки обновлены без вопроса');
    }
    engine = descendants(pid, processes());
    during = snapshot(roots);
  }, 300_000);

  afterAll(() => {
    vaultWatcher?.close();
    if (app && app.exitCode === null) app.kill();
    rmSync(leftover, { recursive: true, force: true });
    rmSync(evilUserData, { recursive: true, force: true });
    if (isWindows) restoreVaults();
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
    // Процессы движка после выхода. Ядро завершает процесс браузера
    // (TerminateProcess, решение 64), а его дочерние (renderer, gpu-process,
    // utility) завершаются сами, заметив обрыв связи с браузером, — чуть позже
    // выхода rhizomind.exe: после TerminateProcess `wait_tree_exit_handle`
    // их не ждёт (браузер уже не жив — дочерние не перечисляются). Предел —
    // норма ручного аудита (docs/security/network-audit.md): «процессы
    // Rhizomind живы через 30 с — нарушение».
    const exited = Date.now();
    const engineAlive = () => processes().filter((p) => engine.some((e) => e.ProcessId === p.ProcessId && e.Name === p.Name));
    const describeProcess = (p: ProcessInfo) => {
      const type = /--type=([\w-]+)/.exec(engine.find((e) => e.ProcessId === p.ProcessId)?.CommandLine ?? '')?.[1] ?? 'browser';
      return `${p.ProcessId} ${p.Name} (${type})`;
    };
    const lingering = engineAlive().map(describeProcess);
    let alive = lingering;
    while (alive.length > 0 && Date.now() - exited < 30_000) {
      await new Promise((r) => setTimeout(r, 500));
      alive = engineAlive().map(describeProcess);
    }
    const lingerMs = Date.now() - exited;
    if (lingering.length > 0) {
      console.log(
        `::notice title=traces::после выхода rhizomind.exe процессы движка завершались ещё ${lingerMs} мс: ${lingering.join(', ')}`,
      );
    } else {
      console.log('процессы движка: все завершены к выходу rhizomind.exe');
    }
    expect(alive, 'процессы движка живы через 30 с после выхода').toEqual([]);
    expect(existsSync(dataDir), `осталась ${dataDir}`).toBe(false);

    const after = snapshot(roots);
    const { changed, removed } = diff(before, after);
    const known = changed.filter(isKnownTrace);
    if (known.length > 0) console.log(`учтённая запись после выхода (SECURITY.md, «Записи вне хранилища»): ${known.join(', ')}`);
    const newOutside = changed.filter(unexpected);
    console.log(`::notice title=traces::новых записей вне хранилища: ${newOutside.length} (учтённых: ${known.length})`);
    expect(newOutside).toEqual([]);
    // Метки данных — ни в путях, ни в содержимом новых и изменённых файлов
    // вне хранилища (во время работы — кроме временной папки движка, её уже нет).
    const own = real(dataDir) + path.sep;
    const touched = [...new Set([...changed, ...diff(before, during).changed.filter((p) => !p.startsWith(own))])];
    const { hits, unreadable } = markersInFiles(touched, dataMarkers);
    console.log(`метки данных вне хранилища: проверено ${touched.length} путей, не прочитано ${unreadable.length}${unreadable.length > 0 ? `: ${unreadable.join(', ')}` : ''}`);
    expect(hits).toEqual([]);
    // Удалены могут быть только остатки папок движка прошлых запусков —
    // подложенный тестом и оставшиеся от сценарных тестов WebDriver, чьи
    // процессы драйвер завершил без выхода (уборка остатков при запуске, ADR 0007).
    // Имя папки — точно `rhizomind-webview-<номер процесса>-<16 шестнадцатеричных>`
    // (src-tauri/src/webview_traces/). Удаление файла кэша TokenBroker (так
    // делает OneAuth движка) — нарушение.
    const tempReal = real(temp) + path.sep;
    const LEFTOVER_NAME = /^rhizomind-webview-\d+-[0-9a-f]{16}$/;
    const isLeftover = (p: string) =>
      p.startsWith(tempReal) && LEFTOVER_NAME.test(p.slice(tempReal.length).split(path.sep)[0] ?? '');
    // Файлы-замки кэша шейдеров Direct3D (`*.lock`) Windows удаляет сама (решение 117).
    expect(removed.filter((p) => !isLeftover(p) && !isShaderCacheFile(p))).toEqual([]);
    expect(existsSync(leftover)).toBe(false);
    // Сценарий дошёл до конца: введённый текст записан в заметку до выхода.
    if (vaultScenario) {
      expect(typing).toBe('typed-ok');
      expect(readFileSync(noteFile, 'utf8')).toContain(typedText);
      expect(renaming).toBe('renamed-ok');
    }
  }, 90_000);

  it.skipIf(!vaultScenario)('служебные файлы 0.2 — только в .rhizomind/; после выхода tmp/ пуста, журнала нет', () => {
    vaultWatcher?.close();
    const sep = path.sep;
    const serviceOnly = ['index.json', 'link-update.json'];
    // Пути из событий и с диска: index.json и link-update.json — только `.rhizomind/<имя>`.
    const seen = [...vaultEvents].filter((p) => serviceOnly.includes(path.basename(p)));
    const onDisk = filesIn(vaultDir).filter((p) => serviceOnly.includes(path.basename(p)));
    console.log(`события хранилища: ${vaultEvents.size} путей; index.json/link-update.json: ${[...new Set([...seen, ...onDisk])].join(', ')}`);
    expect([...seen, ...onDisk].filter((p) => !serviceOnly.some((n) => p === `.rhizomind${sep}${n}`))).toEqual([]);
    // Журнал был во время плана и удалён по завершении (С2-4).
    expect(seen).toContain(`.rhizomind${sep}link-update.json`);
    expect(existsSync(path.join(serviceDir, 'link-update.json'))).toBe(false);
    expect(existsSync(path.join(serviceDir, 'index.json'))).toBe(true);
    expect(filesIn(path.join(serviceDir, 'tmp'))).toEqual([]);
    // Индекс: имена, тег и заголовок в нём есть (он и есть индекс), но не текст заметки.
    const index = readFileSync(path.join(serviceDir, 'index.json'), 'utf8');
    expect(index).toContain(tag);
    expect(index).not.toContain(typedText);
    // Файлы хранилища после сценария.
    expect(existsSync(path.join(vaultDir, `${hubNewName}.md`))).toBe(true);
    expect(existsSync(path.join(vaultDir, `${extNewName}.md`))).toBe(true);
  });

  it('реестр HKCU: нет путей и упоминаний приложения вне учтённых записей', () => {
    const registryAfter = registrySnapshot('after');
    const changes = diffRegistry(registryBefore, registryAfter);
    const data = [vaultName, ...dataMarkers];
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
