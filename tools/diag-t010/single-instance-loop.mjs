// ВРЕМЕННО (задача 0.10, диагностика): повтор сценария «один экземпляр»
// N раз с замерами. Удаляется после диагностики.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const exe = path.resolve(process.argv[2]);
const rounds = Number(process.argv[3] ?? 30);
const USER32 =
  'Add-Type -Namespace Rhz -Name U -MemberDefinition \'' +
  '[DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int c);' +
  '[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr h);' +
  '[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);' +
  '[DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(System.IntPtr h, out int p);' +
  '[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(System.IntPtr h, System.Text.StringBuilder s, int n);' +
  '[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(System.IntPtr h, System.Text.StringBuilder s, int n);' +
  'public delegate bool EnumProc(System.IntPtr h, System.IntPtr l);' +
  '[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, System.IntPtr l);\'';
const windowsOf = (pid) =>
  ps(
    `${USER32}; $o = New-Object System.Collections.ArrayList; [Rhz.U]::EnumWindows({ param($h, $l) $p = 0; [void][Rhz.U]::GetWindowThreadProcessId($h, [ref]$p); if ($p -eq ${pid}) { $c = New-Object System.Text.StringBuilder 256; $t = New-Object System.Text.StringBuilder 256; [void][Rhz.U]::GetClassName($h, $c, 256); [void][Rhz.U]::GetWindowText($h, $t, 256); [void]$o.Add(\"$h $c '$t' vis=$([Rhz.U]::IsWindowVisible($h)) icon=$([Rhz.U]::IsIconic($h))\") }; $true }, [System.IntPtr]::Zero) | Out-Null; $o -join ' | '`,
  );
const ps = (c) => execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', c], { encoding: 'utf8' }).trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function waitFor(ms, probe) {
  const end = Date.now() + ms;
  for (;;) {
    const v = probe();
    if (v !== undefined) return v;
    if (Date.now() > end) return undefined;
    await sleep(200);
  }
}
let failures = 0;
for (let i = 0; i < rounds; i++) {
  const scratch = mkdtempSync(path.join(os.tmpdir(), 'rhz-diag-si-'));
  const env = (t) => {
    const e = { ...process.env, RHIZOMIND_E2E_PROFILE_DIR: path.join(scratch, 'p'), TEMP: t, TMP: t };
    delete e.RHIZOMIND_E2E_VAULT;
    return e;
  };
  const t1 = path.join(scratch, 't1');
  const t2 = path.join(scratch, 't2');
  for (const d of [t1, t2, path.join(scratch, 'p')]) mkdirSync(d, { recursive: true });
  const first = spawn(exe, [], { stdio: ['ignore', 'ignore', 'pipe'], env: env(t1) });
  let err1 = '';
  first.stderr.on('data', (d) => (err1 += d));
  const t0 = Date.now();
  const hwnd = await waitFor(30000, () => {
    const h = ps(`(Get-Process -Id ${first.pid}).MainWindowHandle`);
    return h !== '' && h !== '0' ? h : undefined;
  });
  const shownAt = Date.now() - t0;
  const before = windowsOf(first.pid);
  ps(`${USER32}; [Rhz.U]::ShowWindow([System.IntPtr]${hwnd}, 6) | Out-Null`);
  const iconic = () => ps(`${USER32}; [Rhz.U]::IsIconic([System.IntPtr]${hwnd})`) === 'True';
  const minimized = await waitFor(10000, () => (iconic() ? true : undefined));
  const s0 = Date.now();
  const second = spawn(exe, [], { stdio: ['ignore', 'ignore', 'pipe'], env: env(t2) });
  let err2 = '';
  second.stderr.on('data', (d) => (err2 += d));
  const code = await new Promise((r) => {
    const tm = setTimeout(() => r('timeout'), 20000);
    second.once('exit', (c) => {
      clearTimeout(tm);
      r(c);
    });
  });
  const secondMs = Date.now() - s0;
  const r0 = Date.now();
  const restored = await waitFor(10000, () => (iconic() ? undefined : true));
  const restoreMs = Date.now() - r0;
  const ok = restored === true && code === 0;
  if (!ok) failures++;
  if (ok) console.log(`[diag-si]   окна: ${before}; stderr первого: «${err1.trim()}»`);
  console.log(
    `[diag-si] ${i}: ${ok ? 'OK' : 'СБОЙ'} окно ${shownAt} мс, свёрнуто ${minimized}, второй: код ${code} за ${secondMs} мс, stderr «${err2.trim()}»; развернулось ${restored} за ${restoreMs} мс`,
  );
  if (!ok) {
    console.log(`[diag-si]   свёрнуто окно ${hwnd}; окна при сворачивании: ${before}`);
    console.log(`[diag-si]   окна сейчас: ${windowsOf(first.pid)}`);
    console.log(`[diag-si]   stderr первого: «${err1.trim()}»; temp2: ${readdirSync(t2).join(',')}`);
    try {
      console.log('[diag-si]   окна процесса:', ps(`Get-Process -Id ${first.pid} | Select-Object Id,MainWindowHandle,Responding | Format-List | Out-String`));
    } catch (e) {
      console.log('[diag-si]   ', String(e));
    }
  }
  if (second.exitCode === null) second.kill();
  try {
    ps(`(Get-Process -Id ${first.pid}).CloseMainWindow() | Out-Null`);
  } catch {
    // процесс уже завершился
  }
  await new Promise((r) => {
    if (first.exitCode !== null) return r();
    const tm = setTimeout(() => {
      first.kill();
      r();
    }, 20000);
    first.once('exit', () => {
      clearTimeout(tm);
      r();
    });
  });
  await sleep(500);
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
console.log(`[diag-si] итого сбоев: ${failures} из ${rounds}`);
