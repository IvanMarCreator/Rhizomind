// Запуск приложения под tauri-driver и подключение клиента WebDriver (WebdriverIO).
// tauri-driver запускает собранный исполняемый файл и передаёт команды движку:
// на Windows — msedgedriver (путь в MSEDGEDRIVER или в PATH), на Linux — WebKitWebDriver.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, openSync } from 'node:fs';
import { connect } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { remote } from 'webdriverio';

const ROOT = path.join(import.meta.dirname, '..', '..');
const PORT = 4444;
const SESSION_TIMEOUT_MS = 60_000;

export const SCREENSHOT_DIR = path.join(ROOT, 'test-results', 'e2e');

/**
 * Исполняемый файл приложения.
 * - `webdriver` — тестовая сборка с портом отладки для WebDriver (`npm run build:e2e`,
 *   отдельная папка `src-tauri/target-e2e`);
 * - `release` — выпускная сборка (`npm run build`): на ней идут тест следов и флагов,
 *   потому что msedgedriver подменяет папку данных своей `--user-data-dir`.
 */
export function applicationPath(kind: 'webdriver' | 'release' = 'webdriver'): string {
  const exe = process.platform === 'win32' ? 'rhizomind.exe' : 'rhizomind';
  const target = kind === 'webdriver' ? 'target-e2e' : 'target';
  return path.join(ROOT, 'src-tauri', target, 'release', exe);
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

export async function startApp(): Promise<Session> {
  mkdirSync(SCREENSHOT_DIR, { recursive: true });
  const args = ['--port', String(PORT)];
  if (process.env.MSEDGEDRIVER) args.push('--native-driver', process.env.MSEDGEDRIVER);
  // Журнал tauri-driver и драйвера движка — рядом со снимками (артефакт CI).
  const driverLog = path.join(SCREENSHOT_DIR, `tauri-driver-${process.platform}.log`);
  const logFd = openSync(driverLog, 'a');
  const driver: ChildProcess = spawn(tauriDriverPath(), args, {
    stdio: ['ignore', logFd, logFd],
    // Тестовая сборка пишет сюда, какие переменные WEBVIEW2_* передал драйвер.
    env: { ...process.env, RHIZOMIND_E2E_LOG_DIR: SCREENSHOT_DIR },
  });
  try {
    await waitForPort(PORT, 30_000);
  } catch (error) {
    driver.kill();
    throw error;
  }

  let browser: WebdriverIO.Browser;
  let timer: NodeJS.Timeout | undefined;
  try {
    // Зависание при создании сессии превращается в ошибку за минуту, а не в
    // бесконечное ожидание CI: одна повторная попытка, общий предел.
    const session = remote({
      hostname: '127.0.0.1',
      port: PORT,
      logLevel: 'warn',
      connectionRetryTimeout: 30_000,
      connectionRetryCount: 1,
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
      }
    },
  };
}
