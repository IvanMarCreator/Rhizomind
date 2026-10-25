import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

// Фон окна из docs/design/tokens.md (bg/app): светлая и тёмная тема.
const BACKGROUND = { light: 'rgb(255, 255, 255)', dark: 'rgb(28, 29, 32)' } as const;

describe('главное окно', () => {
  let session: Session;

  beforeAll(async () => {
    session = await startApp();
  });

  afterAll(async () => {
    await session?.stop();
  });

  it('открывается с заголовком «Rhizomind» на фоне темы и сохраняет снимок экрана', async () => {
    const { browser } = session;
    try {
      // Оболочка окна (задача 2.2) вместо прежнего заголовка <h1>.
      await browser.$('[data-testid="shell"]').waitForDisplayed({ timeout: 20_000 });

      expect(await browser.getTitle()).toBe('Rhizomind');

      const look = await browser.execute(() => ({
        dark: window.matchMedia('(prefers-color-scheme: dark)').matches,
        background: getComputedStyle(document.body).backgroundColor,
      }));
      expect(look.background).toBe(look.dark ? BACKGROUND.dark : BACKGROUND.light);
    } finally {
      await saveScreenshot(browser, 'window-opened');
    }
  });
});
