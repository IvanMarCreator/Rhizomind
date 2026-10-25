// Запуск приложения под tauri-driver и подключение клиента WebDriver (WebdriverIO).
// tauri-driver запускает собранный исполняемый файл и передаёт команды движку:
// на Windows — msedgedriver (путь в MSEDGEDRIVER или в PATH), на Linux — WebKitWebDriver.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { connect } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { remote } from 'webdriverio';

const ROOT = path.join(import.meta.dirname, '..', '..');
const PORT = 4444;

export const SCREENSHOT_DIR = path.join(ROOT, 'test-results', 'e2e');

/** Папка сборки Cargo: CARGO_TARGET_DIR (общая папка сборки) или src-tauri/target (.cargo/config.toml). */
function targetDir(): string {
  const fromEnv = process.env.CARGO_TARGET_DIR;
  return fromEnv ? path.resolve(ROOT, fromEnv) : path.join(ROOT, 'src-tauri', 'target');
}

export function applicationPath(): string {
  const exe = process.platform === 'win32' ? 'rhizomind.exe' : 'rhizomind';
  return path.join(targetDir(), 'release', exe);
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
  const driver: ChildProcess = spawn(tauriDriverPath(), args, { stdio: ['ignore', 'inherit', 'inherit'] });
  await waitForPort(PORT, 30_000);

  let browser: WebdriverIO.Browser;
  try {
    browser = await remote({
      hostname: '127.0.0.1',
      port: PORT,
      logLevel: 'warn',
      capabilities: {
        // tauri-driver принимает путь к приложению в своём расширении возможностей.
        'tauri:options': { application: applicationPath() },
      } as WebdriverIO.Capabilities,
    });
  } catch (error) {
    // Сессия не создана — afterAll не сможет остановить драйвер, останавливаем здесь.
    driver.kill();
    throw error;
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
