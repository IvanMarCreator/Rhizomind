// Защищённая оболочка (план 0.1, задача 0.3; ADR 0007, 0009): что не может
// сделать скрипт на странице интерфейса. Тестовая сборка (функция webdriver)
// с теми же обработчиками и CSP, что выпускная.
import { existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { SCREENSHOT_DIR, startApp, type Session } from './driver.ts';

// Адрес своего интерфейса: Windows — http://tauri.localhost, Linux — tauri://localhost.
const OWN_URL = /^(http:\/\/tauri\.localhost|tauri:\/\/localhost)\//;
const EXTERNAL = 'https://example.com/';

describe('защищённая оболочка', () => {
  let session: Session;
  let step = 0;

  beforeAll(async () => {
    session = await startApp();
    await (await session.browser.$('h1')).waitForDisplayed({ timeout: 20_000 });
  });

  afterEach(async (context) => {
    step += 1;
    const name = context.task.name.replace(/[^\p{L}\p{N}]+/gu, '-').slice(0, 60);
    await session?.browser.saveScreenshot(path.join(SCREENSHOT_DIR, `shell-${step}-${name}-${process.platform}.png`));
  });

  afterAll(async () => {
    await session?.stop();
  });

  async function expectStillOnOwnPage(): Promise<void> {
    const { browser } = session;
    await browser.pause(1500);
    expect(await browser.getUrl()).toMatch(OWN_URL);
    expect(await (await browser.$('h1')).getText()).toBe('Rhizomind');
  }

  it('переход на внешний адрес отклоняется (location и ссылка)', async () => {
    const { browser } = session;
    await browser.execute((url) => {
      window.location.href = url;
    }, EXTERNAL);
    await expectStillOnOwnPage();

    await browser.execute((url) => {
      const link = document.createElement('a');
      link.id = 'e2e-external-link';
      link.href = url;
      link.textContent = 'внешняя ссылка';
      document.body.append(link);
    }, EXTERNAL);
    await (await browser.$('#e2e-external-link')).click();
    await expectStillOnOwnPage();
  });

  it('window.open не открывает новое окно', async () => {
    const { browser } = session;
    const before = await browser.getWindowHandles();
    await browser.execute((url) => {
      window.open(url, '_blank');
    }, EXTERNAL);
    await browser.pause(1500);
    expect(await browser.getWindowHandles()).toEqual(before);
    await expectStillOnOwnPage();
  });

  it('загрузка файла отклоняется', async () => {
    const { browser } = session;
    const name = `rhizomind-e2e-download-${Date.now()}.txt`;
    await browser.execute((fileName) => {
      const link = document.createElement('a');
      link.id = 'e2e-download';
      link.download = fileName;
      link.href = URL.createObjectURL(new Blob(['rhizomind'], { type: 'text/plain' }));
      link.textContent = 'скачать';
      document.body.append(link);
    }, name);
    await (await browser.$('#e2e-download')).click();
    await browser.pause(2500);
    const downloads = path.join(os.homedir(), 'Downloads');
    expect(existsSync(path.join(downloads, name))).toBe(false);
    expect(existsSync(path.join(downloads, `${name}.crdownload`))).toBe(false);
    await expectStillOnOwnPage();
  });

  it('CSP запрещает fetch наружу', async () => {
    const result = await session.browser.execute(async (url) => {
      const violations: string[] = [];
      document.addEventListener('securitypolicyviolation', (e) => {
        violations.push(`${e.effectiveDirective} ${e.blockedURI}`);
      });
      let outcome: string;
      try {
        await window.fetch(url);
        outcome = 'загружено';
      } catch (error) {
        outcome = `ошибка: ${String(error)}`;
      }
      await new Promise((resolve) => setTimeout(resolve, 300));
      return { outcome, violations };
    }, EXTERNAL);
    expect(result.outcome).toMatch(/^ошибка/);
    expect(result.violations.some((v) => v.startsWith('connect-src'))).toBe(true);
  });

  it('разрешения браузера отклоняются (уведомления)', async () => {
    const permission = await session.browser.execute(async () => Notification.requestPermission());
    expect(permission).toBe('denied');
  });

  it('вызов ядра идёт через изолированный iframe и его фильтр', async () => {
    const result = await session.browser.execute(async () => {
      const internals = (
        window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string) => Promise<unknown> } }
      ).__TAURI_INTERNALS__;
      const iframe = document.getElementById('__tauri_isolation__');
      try {
        await internals.invoke('plugin:app|version');
        return { iframe: iframe !== null, error: '' };
      } catch (error) {
        return { iframe: iframe !== null, error: String(error) };
      }
    });
    expect(result.iframe).toBe(true);
    // Фильтр (src-isolation/index.js) подменил команду — ядро отвечает ошибкой
    // про подменённое имя, а не про plugin:app|version.
    expect(result.error).toContain('__rhizomind_isolation_rejected__');
  });
});
