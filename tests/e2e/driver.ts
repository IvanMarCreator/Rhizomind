// Запуск приложения под tauri-driver и подключение клиента WebDriver (WebdriverIO).
// tauri-driver запускает собранный исполняемый файл и передаёт команды движку:
// на Windows — msedgedriver (путь в MSEDGEDRIVER или в PATH), на Linux — WebKitWebDriver.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, openSync, rmSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { remote } from 'webdriverio';

const ROOT = path.join(import.meta.dirname, '..', '..');
// Порт tauri-driver; RHIZOMIND_E2E_DRIVER_PORT — чтобы сценарные тесты двух
// рабочих копий на одной машине не делили порты.
const PORT = Number(process.env.RHIZOMIND_E2E_DRIVER_PORT ?? 4444);
// Порт драйвера движка за tauri-driver (--native-port; по умолчанию 4445).
const NATIVE_PORT = PORT + 1;
// Создание сессии в CI на Linux занимает ~30 с (холодный запуск WebKitGTK в
// Xvfb): предел с запасом, но меньше hookTimeout (120 с).
const SESSION_TIMEOUT_MS = 100_000;

export const SCREENSHOT_DIR = path.join(ROOT, 'test-results', 'e2e');

/** Папка сборки Cargo: CARGO_TARGET_DIR (общая папка сборки) или src-tauri/target (.cargo/config.toml). */
function targetDir(): string {
  const fromEnv = process.env.CARGO_TARGET_DIR;
  return fromEnv ? path.resolve(ROOT, fromEnv) : path.join(ROOT, 'src-tauri', 'target');
}

/**
 * Исполняемый файл приложения.
 * - `webdriver` — тестовая сборка с портом отладки для WebDriver (`npm run build:e2e`):
 *   всегда в отдельной папке `src-tauri/target-e2e`, чтобы тестовый файл не оказался
 *   там, откуда берут выпускной;
 * - `release` — выпускная сборка (`npm run build`, папка сборки Cargo): на ней идут
 *   тест следов и флагов, потому что msedgedriver подменяет папку данных.
 */
export function applicationPath(kind: 'webdriver' | 'release' = 'webdriver'): string {
  const exe = process.platform === 'win32' ? 'rhizomind.exe' : 'rhizomind';
  const dir = kind === 'webdriver' ? path.join(ROOT, 'src-tauri', 'target-e2e') : targetDir();
  return path.join(dir, 'release', exe);
}

function tauriDriverPath(): string {
  const exe = process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver';
  return process.env.TAURI_DRIVER ?? path.join(os.homedir(), '.cargo', 'bin', exe);
}

function waitForPort(port: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = connect(port, '127.0.0.1');
      socket.once('connect', () => {
        socket.end();
        resolve();
      });
      socket.once('error', () => {
        socket.destroy();
        if (Date.now() > deadline) reject(new Error(`tauri-driver не открыл порт ${port}`));
        else setTimeout(attempt, 250);
      });
    };
    attempt();
  });
}

function portIsBusy(port: number): Promise<boolean> {
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

/**
 * Ждёт, пока порты прошлой сессии освободятся: драйвер движка (WebKitWebDriver,
 * msedgedriver) завершается после tauri-driver не сразу, и новая сессия сразу
 * за прошлой падала с UND_ERR_SOCKET (сценарный тест дерева после теста оболочки).
 */
async function waitForPortsFree(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while ((await portIsBusy(PORT)) || (await portIsBusy(NATIVE_PORT))) {
    if (Date.now() > deadline) throw new Error(`порты ${PORT}/${NATIVE_PORT} заняты прошлой сессией`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

/** Останавливает tauri-driver и ждёт его завершения. */
function stopDriver(driver: ChildProcess): Promise<void> {
  return new Promise((resolve) => {
    if (driver.exitCode !== null || driver.signalCode !== null) {
      resolve();
      return;
    }
    const timer = setTimeout(resolve, 10_000);
    driver.once('exit', () => {
      clearTimeout(timer);
      resolve();
    });
    driver.kill();
  });
}

export interface Session {
  browser: WebdriverIO.Browser;
  stop: () => Promise<void>;
}

export interface StartOptions {
  /**
   * Папка хранилища, которое тестовая сборка откроет при запуске — без
   * системного окна выбора папки (переменная RHIZOMIND_E2E_VAULT, только в
   * сборке с функцией `webdriver`: src-tauri/src/windows/mod.rs, модуль e2e).
   * Несколько папок попадают в список хранилищ; открывается последняя.
   */
  vault?: string | string[];
  /**
   * Ответ системного окна выбора папки для «Создать хранилище» и «Открыть
   * папку как хранилище» (переменная RHIZOMIND_E2E_PICK_FOLDER, только в
   * тестовой сборке; задача 2.8а).
   */
  pickFolder?: string;
  /**
   * «Медленный диск»: пауза перед каждой записью заметки, мс (переменная
   * RHIZOMIND_E2E_SLOW_WRITE_MS, только в тестовой сборке; задача 0.7 —
   * предел ожидания при закрытии окна).
   */
  slowWriteMs?: number;
  /**
   * Папки хранилищ считаются лежащими на внешнем носителе (переменная
   * RHIZOMIND_E2E_REMOVABLE, только в тестовой сборке; решение владельца 66).
   */
  removable?: boolean;
  /**
   * Файл-признак «носитель вынут» (RHIZOMIND_E2E_UNPLUG_FLAG, только в
   * тестовой сборке; решение 66): пока файл есть, запись отклоняется.
   */
  unplugFlag?: string;
  /**
   * Папка профиля (список хранилищ `vaults.json`) из прошлого запуска —
   * сценарий «перезапуск с последним хранилищем» (задача 3.1). Без неё у
   * каждого запуска своя пустая временная папка. Переданную папку `stop()`
   * не удаляет: её удаляет тест.
   */
  profile?: string;
}

/** Новая папка профиля для [`StartOptions.profile`]; удаляет тест. */
export function createTestProfile(): string {
  return mkdtempSync(path.join(os.tmpdir(), 'rhizomind-e2e-profile-'));
}

/**
 * Тестовое хранилище `name` (имя папки — имя хранилища) в своей временной
 * папке, с заметками `notes`. Удаляется [`removeTestVault`].
 */
export function createTestVault(name: string, notes: Record<string, string> = {}): string {
  const root = path.join(mkdtempSync(path.join(os.tmpdir(), 'rhizomind-e2e-vault-')), name);
  mkdirSync(root);
  for (const [file, text] of Object.entries(notes)) writeFileSync(path.join(root, file), text);
  return root;
}

/** Удаляет хранилище из [`createTestVault`] вместе с его временной папкой. */
export function removeTestVault(root: string): void {
  rmSync(path.dirname(root), { recursive: true, force: true });
}

export async function startApp(options: StartOptions = {}): Promise<Session> {
  mkdirSync(SCREENSHOT_DIR, { recursive: true });
  // Список хранилищ тестовой сборки — во временной папке, а не в
  // %LOCALAPPDATA%\Rhizomind пользователя (RHIZOMIND_E2E_PROFILE_DIR).
  const profile = options.profile ?? createTestProfile();
  const env: NodeJS.ProcessEnv = { ...process.env, RHIZOMIND_E2E_PROFILE_DIR: profile };
  const vaults = typeof options.vault === 'string' ? [options.vault] : (options.vault ?? []);
  if (vaults.length > 0) env.RHIZOMIND_E2E_VAULT = vaults.join(path.delimiter);
  else delete env.RHIZOMIND_E2E_VAULT;
  if (options.pickFolder) env.RHIZOMIND_E2E_PICK_FOLDER = options.pickFolder;
  else delete env.RHIZOMIND_E2E_PICK_FOLDER;
  if (options.slowWriteMs !== undefined) env.RHIZOMIND_E2E_SLOW_WRITE_MS = String(options.slowWriteMs);
  else delete env.RHIZOMIND_E2E_SLOW_WRITE_MS;
  if (options.removable) env.RHIZOMIND_E2E_REMOVABLE = '1';
  else delete env.RHIZOMIND_E2E_REMOVABLE;
  if (options.unplugFlag) env.RHIZOMIND_E2E_UNPLUG_FLAG = options.unplugFlag;
  else delete env.RHIZOMIND_E2E_UNPLUG_FLAG;
  const removeProfile = () => {
    if (!options.profile) rmSync(profile, { recursive: true, force: true });
  };
  await waitForPortsFree(30_000);
  const args = ['--port', String(PORT), '--native-port', String(NATIVE_PORT)];
  if (process.env.MSEDGEDRIVER) args.push('--native-driver', process.env.MSEDGEDRIVER);
  // Журнал tauri-driver и драйвера движка — рядом со снимками (артефакт CI).
  const driverLog = path.join(SCREENSHOT_DIR, `tauri-driver-${process.platform}.log`);
  const logFd = openSync(driverLog, 'a');
  const driver: ChildProcess = spawn(tauriDriverPath(), args, { stdio: ['ignore', logFd, logFd], env });
  try {
    await waitForPort(PORT, 30_000);
  } catch (error) {
    driver.kill();
    removeProfile();
    throw error;
  }

  let browser: WebdriverIO.Browser;
  let timer: NodeJS.Timeout | undefined;
  try {
    // Зависание при создании сессии превращается в ошибку, а не в бесконечное
    // ожидание CI: одна попытка без повторов и общий предел.
    const session = remote({
      hostname: '127.0.0.1',
      port: PORT,
      logLevel: 'warn',
      connectionRetryTimeout: 90_000,
      connectionRetryCount: 0,
      capabilities: {
        // tauri-driver принимает путь к приложению в своём расширении возможностей.
        'tauri:options': { application: applicationPath() },
      } as WebdriverIO.Capabilities,
    });
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(new Error(`сессия WebDriver не создана за ${SESSION_TIMEOUT_MS / 1000} с; журнал — ${driverLog}`)),
        SESSION_TIMEOUT_MS,
      );
    });
    browser = await Promise.race([session, deadline]);
  } catch (error) {
    // Сессия не создана — afterAll не сможет остановить драйвер, останавливаем здесь.
    driver.kill();
    removeProfile();
    throw error;
  } finally {
    clearTimeout(timer);
  }

  return {
    browser,
    stop: async () => {
      try {
        await browser.deleteSession();
      } finally {
        await stopDriver(driver);
        await waitForPortsFree(15_000).catch(() => undefined);
        removeProfile();
      }
    },
  };
}
