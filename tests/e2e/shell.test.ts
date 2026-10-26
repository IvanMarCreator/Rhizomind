// Сценарий задачи 2.2: оболочка окна по концепту К2 на настоящем ядре.
// Окно сценарного теста открывается с пустым хранилищем «Рабочая база»: видны
// лента, боковая панель с переключателем хранилищ и пустое состояние (окно без
// хранилища — «Управление хранилищами», задача 2.8а: tests/e2e/vaults.test.ts). Снимки — в
// светлой и тёмной теме (тема ставится атрибутом data-theme, как её ставит
// src/theme/theme.ts), сравниваются с кадром Э1 и docs/design/tokens.md.
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

// bg/app, bg/ribbon, bg/sidebar из tokens.md.
const COLORS = {
  light: { app: 'rgb(255, 255, 255)', ribbon: 'rgb(238, 240, 242)', sidebar: 'rgb(246, 247, 248)' },
  dark: { app: 'rgb(28, 29, 32)', ribbon: 'rgb(19, 20, 22)', sidebar: 'rgb(23, 24, 26)' },
} as const;

describe('оболочка окна', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    // Окно без хранилища — окно «Управление хранилищами» (2.8а): оболочка —
    // в окне тестового хранилища.
    vault = createTestVault('Рабочая база');
    session = await startApp({ vault });
    await session.browser.$('[data-testid="shell"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('каркас К2: лента, боковая панель, строка над заметкой, пустое состояние', async () => {
    const { browser } = session;
    try {
      for (const label of ['Файлы', 'Быстрый переход (Ctrl+O)', 'Настройки', 'Назад (Alt+←)', 'Вперёд (Alt+→)']) {
        expect(await browser.$(`button[aria-label="${label}"]`).isDisplayed()).toBe(true);
      }
      expect(await browser.$('button[aria-label="Хранилище «Рабочая база»"]').isDisplayed()).toBe(true);
      expect(await browser.$('aside[aria-label="Боковая панель"] h2').getText()).toBe('ФАЙЛЫ');
      expect(await browser.$('main').getText()).toContain('Нет открытой заметки');

      // Шрифт интерфейса — встроенный Inter, загружен из файла приложения.
      const fonts = await browser.execute(async () => {
        await document.fonts.ready;
        return {
          inter: [...document.fonts].some((face) => face.family.includes('Inter') && face.status === 'loaded'),
          family: getComputedStyle(document.body).fontFamily,
        };
      });
      expect(fonts.family).toMatch(/^"?Inter/);
      expect(fonts.inter).toBe(true);
    } finally {
      await saveScreenshot(browser, 'shell-system');
    }
  });

  it.each(['light', 'dark'] as const)('тема %s: цвета из tokens.md, снимок экрана', async (theme) => {
    const { browser } = session;
    try {
      const look = await browser.execute((name) => {
        document.documentElement.dataset.theme = name;
        const color = (selector: string) => {
          const element = document.querySelector(selector);
          return element ? getComputedStyle(element).backgroundColor : '';
        };
        return { app: color('main'), ribbon: color('[role="toolbar"]'), sidebar: color('aside') };
      }, theme);
      expect(look).toEqual(COLORS[theme]);
    } finally {
      await saveScreenshot(browser, `shell-${theme}`);
    }
  });
});
