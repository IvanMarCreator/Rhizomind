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
      // Запуск без хранилища — окно «Управление хранилищами» (задача 2.8а).
      await browser.$('[data-testid="vault-manager"]').waitForDisplayed({ timeout: 20_000 });

      expect(await browser.getTitle()).toBe('Rhizomind');

      const look = await browser.execute(() => ({
        dark: window.matchMedia('(prefers-color-scheme: dark)').matches,
        background: getComputedStyle(document.body).backgroundColor,
      }));
      expect(look.background).toBe(look.dark ? BACKGROUND.dark : BACKGROUND.light);

      // Пустое окно без полосы прокрутки (обзор 0.3, снимок Windows): страница
      // не выше окна, изолированный iframe Tauri места не занимает.
      const page = await browser.execute(() => {
        const root = document.scrollingElement ?? document.documentElement;
        const frame = document.querySelector('iframe#__tauri_isolation__');
        return {
          scroll: root.scrollHeight,
          view: root.clientHeight,
          frame: frame ? frame.getBoundingClientRect().height : 0,
        };
      });
      expect(page.scroll).toBeLessThanOrEqual(page.view);
      expect(page.frame).toBe(0);
    } finally {
      await saveScreenshot(browser, 'window-opened');
    }
  });
});
