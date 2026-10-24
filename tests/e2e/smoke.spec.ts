import { _electron as electron, expect, test } from '@playwright/test';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '../..');

// Под root (контейнеры разработки) Chromium не запускается без --no-sandbox.
// Это ключ только для теста: в приложении песочница не отключается.
const extraArgs = process.getuid?.() === 0 ? ['--no-sandbox'] : [];

test('окно открывается и показывает интерфейс через app://', async () => {
  const app = await electron.launch({ args: [...extraArgs, ROOT], cwd: ROOT });
  try {
    const page = await app.firstWindow();
    await page.waitForLoadState('domcontentloaded');

    await expect(page).toHaveTitle('Rhizomind');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Rhizomind');
    expect(await page.evaluate(() => location.protocol)).toBe('app:');
    expect(await page.evaluate(() => window.rhizomind.apiVersion)).toBe(1);

    await page.screenshot({ path: path.join(ROOT, 'test-results', 'smoke.png') });
  } finally {
    await app.close();
  }
});
