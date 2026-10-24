import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { SCREENSHOT_DIR, startApp, type Session } from './driver.ts';

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
    const heading = await browser.$('h1');
    await heading.waitForDisplayed({ timeout: 20_000 });

    expect(await browser.getTitle()).toBe('Rhizomind');
    expect(await heading.getText()).toBe('Rhizomind');

    const look = await browser.execute(() => ({
      dark: window.matchMedia('(prefers-color-scheme: dark)').matches,
      background: getComputedStyle(document.body).backgroundColor,
    }));
    expect(look.background).toBe(look.dark ? BACKGROUND.dark : BACKGROUND.light);

    await browser.saveScreenshot(path.join(SCREENSHOT_DIR, `window-opened-${process.platform}.png`));
  });
});
