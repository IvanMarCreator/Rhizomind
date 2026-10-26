// Закрытие окна хранилища (задача 0.7, решение владельца 53, инвариант 4):
// окно исчезает сразу, а программа завершается только после того, как ввод
// записан на диск; ожидание ограничено (10 с — CLOSE_TIMEOUT в
// src-tauri/src/windows/closing.rs).
//
// Предел ожидания ядра (окно уничтожается без ответа интерфейса) проверяют
// модульные тесты src-tauri/src/windows/tests.rs: интерфейс сам укладывается
// в свой крайний срок 7 с и возвращает окно (тест «медленный диск»).
//
// Окно закрывается так же, как пользователь кнопкой «×»: Windows — WM_CLOSE
// главному окну процесса (CloseMainWindow), Linux — WM_DELETE_WINDOW по
// протоколу X11 (x11-close.py). Команда WebDriver «закрыть окно» этого не
// воспроизводит.
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, readlinkSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { applicationPath, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';

const isWindows = process.platform === 'win32';
const isLinux = process.platform === 'linux';

const NOTE = 'Заметка';
const TYPED = 'typed-right-before-close-0123456789';

function powershell(command: string): string {
  return execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8' }).trim();
}

/** Процесс тестовой сборки (его запускает драйвер движка, а не тест). */
function appPid(): number {
  const exe = applicationPath('webdriver');
  let pids: number[];
  if (isWindows) {
    const out = powershell(
      `@(Get-Process -Name rhizomind -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq '${exe.replace(/'/g, "''")}' } | ForEach-Object { $_.Id }) -join ','`,
    );
    pids = out === '' ? [] : out.split(',').map(Number);
  } else {
    const target = realpathSync(exe);
    pids = readdirSync('/proc')
      .filter((name) => /^\d+$/.test(name))
      .filter((name) => {
        try {
          return readlinkSync(`/proc/${name}/exe`) === target;
        } catch {
          return false;
        }
      })
      .map(Number);
  }
  expect(pids, 'процесс программы один').toHaveLength(1);
  return pids[0] as number;
}

/** Процесс ещё работает (на Linux зомби — уже завершён). */
function isRunning(pid: number): boolean {
  if (isLinux) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
      return !/^\d+ \(.*\) Z/.test(stat);
    } catch {
      return false;
    }
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Закрыть окно процесса, как кнопкой «×». */
function closeLikeUser(pid: number): void {
  if (isWindows) {
    expect(powershell(`(Get-Process -Id ${pid}).CloseMainWindow()`)).toBe('True');
  } else {
    execFileSync('python3', ['-I', path.join(import.meta.dirname, 'x11-close.py'), String(pid)]);
  }
}

/** Ждёт выхода процесса; возвращает, сколько ждали (мс). */
async function waitForExit(pid: number, timeoutMs: number): Promise<number> {
  const started = Date.now();
  while (isRunning(pid)) {
    if (Date.now() - started > timeoutMs) throw new Error(`процесс ${pid} не завершился за ${timeoutMs} мс`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return Date.now() - started;
}

/** Открыть заметку в дереве и набрать текст в конце, без пауз. */
async function typeIntoNote(browser: WebdriverIO.Browser, open = true): Promise<void> {
  if (open) {
    const row = browser.$(`[role="treeitem"][aria-label="${NOTE}"]`);
    await row.waitForExist({ timeout: 30_000 });
    await row.click();
  }
  const text = browser.$('[role="textbox"][aria-label="Текст заметки"]');
  await text.waitForExist({ timeout: 10_000 });
  await text.click();
  await browser.keys(['Control', 'End']);
  await browser.keys(TYPED);
}

/** Ждёт текста на странице (уведомление внизу окна). */
async function waitForText(browser: WebdriverIO.Browser, text: string, timeout: number): Promise<void> {
  await browser.waitUntil(() => browser.execute((wanted) => document.body.innerText.includes(wanted), text), {
    timeout,
    timeoutMsg: `нет текста «${text}»`,
  });
}

/** Сессия WebDriver после выхода программы уже недействительна. */
async function stopQuietly(session: Session | undefined): Promise<void> {
  await session?.stop().catch(() => undefined);
}

describe.skipIf(!isWindows && !isLinux)('закрытие окна: ввод не теряется', () => {
  let session: Session | undefined;
  let vault = '';

  afterEach(async () => {
    await stopQuietly(session);
    session = undefined;
    if (vault) removeTestVault(vault);
    vault = '';
  });

  it('ввод → немедленное закрытие окна → после выхода процесса файл содержит ввод', async () => {
    vault = createTestVault('Закрытие', { [`${NOTE}.md`]: 'начало\n' });
    session = await startApp({ vault });
    await typeIntoNote(session.browser);
    const pid = appPid();

    closeLikeUser(pid);
    await waitForExit(pid, 30_000);

    expect(readFileSync(path.join(vault, `${NOTE}.md`), 'utf8')).toContain(TYPED);
  });

  it('запись зависла («медленный диск»): окно возвращается к крайнему сроку, повторное закрытие закрывает без сохранения', async () => {
    vault = createTestVault('Медленный диск', { [`${NOTE}.md`]: 'начало\n' });
    session = await startApp({ vault, slowWriteMs: 120_000 });
    await typeIntoNote(session.browser);
    const pid = appPid();

    const started = Date.now();
    closeLikeUser(pid);
    // Интерфейс ждёт записи не дольше 7 с (до предела ядра 10 с), потом
    // возвращает окно с причиной «диск не отвечает», программа работает.
    await waitForText(session.browser, 'диск не отвечает', 15_000);
    const waited = Date.now() - started;
    expect(waited).toBeGreaterThanOrEqual(6_000);
    expect(isRunning(pid)).toBe(true);

    // Осознанное повторное закрытие (через ≥ 2 с) — без сохранения.
    await new Promise((resolve) => setTimeout(resolve, 3_000));
    closeLikeUser(pid);
    await waitForExit(pid, 30_000);
    expect(readFileSync(path.join(vault, `${NOTE}.md`), 'utf8')).toBe('начало\n');
  }, 120_000);

  // Решение владельца 66: хранилище на внешнем носителе пропало — ввод
  // записать некуда, окно закрывается (с системным сообщением; в тестовой
  // сборке оно уходит в stderr), программа завершается.
  it('внешний носитель вынут: окно закрывается, программа завершается', async () => {
    vault = createTestVault('Флешка', { [`${NOTE}.md`]: 'начало\n' });
    const flag = `${vault}.unplugged`;
    session = await startApp({ vault, removable: true, unplugFlag: flag });
    const row = session.browser.$(`[role="treeitem"][aria-label="${NOTE}"]`);
    await row.waitForExist({ timeout: 30_000 });
    await row.click();
    await session.browser.$('[role="textbox"][aria-label="Текст заметки"]').waitForExist({ timeout: 10_000 });
    // «Вынуть носитель»: подмена тестовой сборки (запись отклоняется, папка
    // считается недоступной) — на Windows папку под наблюдателем не переименовать.
    writeFileSync(flag, '');
    await typeIntoNote(session.browser, false);
    const pid = appPid();
    try {
      closeLikeUser(pid);
      await waitForExit(pid, 30_000);
    } finally {
      rmSync(flag, { force: true });
    }
    expect(readFileSync(path.join(vault, `${NOTE}.md`), 'utf8')).toBe('начало\n');
  }, 120_000);

  // Внутренний диск (Windows: том DRIVE_FIXED): запись не удалась, носитель
  // на месте — окно возвращается с уведомлением, программа работает.
  it.skipIf(!isWindows)('внутренний диск: запись не удалась — окно возвращается, программа работает', async () => {
    vault = createTestVault('Внутренний', { [`${NOTE}.md`]: 'начало\n' });
    const flag = `${vault}.unplugged`;
    session = await startApp({ vault, unplugFlag: flag });
    const row = session.browser.$(`[role="treeitem"][aria-label="${NOTE}"]`);
    await row.waitForExist({ timeout: 30_000 });
    await row.click();
    await session.browser.$('[role="textbox"][aria-label="Текст заметки"]').waitForExist({ timeout: 10_000 });
    // Запись отклоняется подменой, том — несъёмный (C:): носитель не «пропал».
    writeFileSync(flag, '');
    await typeIntoNote(session.browser, false);
    const pid = appPid();
    try {
      closeLikeUser(pid);
      await new Promise((resolve) => setTimeout(resolve, 12_000));
      expect(isRunning(pid), 'программа работает').toBe(true);
      await waitForText(session.browser, 'Окно не закрыто', 5_000);
    } finally {
      rmSync(flag, { force: true });
    }
  }, 120_000);
});
