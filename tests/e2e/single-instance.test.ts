// Один экземпляр программы на ВЫПУСКНОЙ сборке (задача 0.5, решения 57 и 58;
// src-tauri/src/single_instance/). Без WebDriver: второй процесс запускается
// напрямую, как двойной щелчок по ярлыку.
//
// Второй запуск завершается сам (код 0), ничего не записав: его временная
// папка (TEMP/TMPDIR) остаётся пустой — проверка экземпляра идёт раньше
// создания временной папки WebView2; список хранилищ не меняется. Первый
// экземпляр продолжает работать. Windows: свёрнутое окно первого
// разворачивается — сигнал дошёл; процесс программы один.
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { applicationPath } from './driver.ts';

const isWindows = process.platform === 'win32';
const isLinux = process.platform === 'linux';

async function waitFor<T>(what: string, timeoutMs: number, probe: () => T | undefined): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`не дождались: ${what}`);
    await new Promise((r) => setTimeout(r, 300));
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

/** Отпечаток папки: относительные пути, размеры и время изменения. */
function fingerprint(root: string): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    if (!existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        out.push(`${path.relative(root, full)}/`);
        walk(full);
      } else {
        const st = statSync(full);
        out.push(`${path.relative(root, full)} ${st.size} ${st.mtimeMs}`);
      }
    }
  };
  walk(root);
  return out.sort();
}

// --- Windows: окно первого экземпляра через PowerShell -----------------------

const USER32 =
  'Add-Type -Namespace Rhz -Name U -MemberDefinition \'' +
  '[DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int c);' +
  '[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr h);\'';

function powershell(command: string): string {
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8' }).trim();
}

function mainWindow(pid: number): string {
  return powershell(`(Get-Process -Id ${pid}).MainWindowHandle`);
}

function isMinimized(handle: string): boolean {
  return powershell(`${USER32}; [Rhz.U]::IsIconic([System.IntPtr]${handle})`) === 'True';
}

function minimize(handle: string): void {
  // 6 = SW_MINIMIZE
  powershell(`${USER32}; [Rhz.U]::ShowWindow([System.IntPtr]${handle}, 6) | Out-Null`);
}

function programProcesses(exe: string): number[] {
  const out = powershell(
    `@(Get-Process -Name rhizomind -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq '${exe.replace(/'/g, "''")}' } | ForEach-Object { $_.Id }) -join ','`,
  );
  return out === '' ? [] : out.split(',').map(Number);
}

describe.skipIf(!isWindows && !isLinux)('один экземпляр программы (выпускная сборка)', () => {
  const exe = applicationPath('release');
  const scratch = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-single-'));
  // Linux: список хранилищ и замок — в своей папке профиля (XDG_DATA_HOME),
  // а не в профиле машины; у каждого процесса своя временная папка.
  const dataHome = path.join(scratch, 'data');
  const firstTemp = path.join(scratch, 'temp-first');
  const secondTemp = path.join(scratch, 'temp-second');
  // Windows: список хранилищ в профиле ОС (dirs::data_local_dir — известная
  // папка, переменной окружения не подменяется).
  const profileDir = isWindows
    ? path.join(process.env.LOCALAPPDATA ?? '', 'Rhizomind')
    : path.join(dataHome, 'Rhizomind');
  const envFor = (temp: string): NodeJS.ProcessEnv =>
    isWindows
      ? { ...process.env, TEMP: temp, TMP: temp }
      : { ...process.env, TMPDIR: temp, XDG_DATA_HOME: dataHome };

  let first: ChildProcess;
  let firstWindow = '';
  let second: ChildProcess;
  let secondExit: number | null | 'timeout';
  let profileBefore: string[] = [];
  let profileAfter: string[] = [];

  beforeAll(async () => {
    expect(existsSync(exe), `${exe}: нужна выпускная сборка (npm run build)`).toBe(true);
    for (const dir of [dataHome, firstTemp, secondTemp]) mkdirSync(dir, { recursive: true });

    first = spawn(exe, [], { stdio: 'ignore', env: envFor(firstTemp) });
    const pid = first.pid ?? 0;
    // Первый прошёл проверку экземпляра и создал временную папку движка.
    await waitFor('временная папка движка первого экземпляра', 30_000, () =>
      readdirSync(firstTemp).find((n) => n.startsWith(`rhizomind-webview-${pid}-`)),
    );
    if (isWindows) {
      firstWindow = await waitFor('окно первого экземпляра', 30_000, () => {
        const handle = mainWindow(pid);
        return handle !== '' && handle !== '0' ? handle : undefined;
      });
      minimize(firstWindow);
      await waitFor('окно свёрнуто', 10_000, () => (isMinimized(firstWindow) ? true : undefined));
    } else {
      await new Promise((r) => setTimeout(r, 3000));
    }

    profileBefore = fingerprint(profileDir);
    second = spawn(exe, [], { stdio: 'ignore', env: envFor(secondTemp) });
    secondExit = await exitOf(second, 20_000);
    profileAfter = fingerprint(profileDir);
  }, 120_000);

  afterAll(async () => {
    if (second && second.exitCode === null) second.kill();
    if (first && first.exitCode === null) {
      if (isWindows) {
        try {
          powershell(`(Get-Process -Id ${first.pid}).CloseMainWindow() | Out-Null`);
        } catch {
          // процесс уже завершился
        }
      } else {
        first.kill('SIGTERM');
      }
      if ((await exitOf(first, 30_000)) === 'timeout') first.kill();
    }
    rmSync(scratch, { recursive: true, force: true });
  }, 60_000);

  it('второй запуск завершается сам с кодом 0', () => {
    expect(secondExit).toBe(0);
  });

  it('второй запуск ничего не записал: временная папка пуста, список хранилищ не изменился', () => {
    expect(readdirSync(secondTemp)).toEqual([]);
    expect(profileAfter).toEqual(profileBefore);
  });

  it('первый экземпляр продолжает работать', () => {
    expect(first.exitCode).toBeNull();
    if (isWindows) expect(programProcesses(exe)).toEqual([first.pid]);
  });

  it.skipIf(!isWindows)('окно первого экземпляра выведено вперёд: свёрнутое развернулось', async () => {
    await waitFor('окно развёрнуто', 10_000, () => (isMinimized(firstWindow) ? undefined : true));
    expect(mainWindow(first.pid ?? 0)).toBe(firstWindow);
  });
});
