// Запуск приложения под tauri-driver и подключение клиента WebDriver (WebdriverIO).
// tauri-driver запускает собранный исполняемый файл и передаёт команды движку:
// на Windows — msedgedriver (путь в MSEDGEDRIVER или в PATH), на Linux — WebKitWebDriver.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, openSync, rmSync } from 'node:fs';
import { connect } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { remote } from 'webdriverio';

const ROOT = path.join(import.meta.dirname, '..', '..');
const PORT = 4444;
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

export interface Session {
  browser: WebdriverIO.Browser;
  stop: () => Promise<void>;
}

export interface StartOptions {
  /**
   * Папка хранилища, которое тестовая сборка откроет при запуске — без
   * системного окна выбора папки (переменная RHIZOMIND_E2E_VAULT, только в
   * сборке с функцией `webdriver`: src-tauri/src/windows/mod.rs, модуль e2e).
   */
  vault?: string;
}

export async function startApp(options: StartOptions = {}): Promise<Session> {
  mkdirSync(SCREENSHOT_DIR, { recursive: true });
  // Список хранилищ тестовой сборки — во временной папке, а не в
  // %LOCALAPPDATA%\Rhizomind пользователя (RHIZOMIND_E2E_PROFILE_DIR).
  const profile = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-e2e-profile-'));
  const env: NodeJS.ProcessEnv = { ...process.env, RHIZOMIND_E2E_PROFILE_DIR: profile };
  if (options.vault) env.RHIZOMIND_E2E_VAULT = options.vault;
  else delete env.RHIZOMIND_E2E_VAULT;
  const removeProfile = () => rmSync(profile, { recursive: true, force: true });
  const args = ['--port', String(PORT)];
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
        driver.kill();
        removeProfile();
      }
    },
  };
}
