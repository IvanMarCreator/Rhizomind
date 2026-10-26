// Быстрый переход (С3.5, решения 50–51) на настоящем ядре и настоящей
// клавиатуре движка (исправления по ручной проверке КТ1): Tab и Shift+Tab не
// уводят фокус из поля, Tab подставляет имя выбранного пункта; пояснение у
// неактивного пункта «Создать заметку» видно целиком; шестерёнка на ленте —
// явно неактивна до окна настроек.
//
// Компонентный тест (QuickSwitcher.component.test.ts) отправляет keydown прямо
// в поле: он не проверяет, где фокус в момент нажатия, и не проходит через
// обработку клавиш движком. Здесь клавиши идут через WebDriver (msedgedriver —
// тем же путём, что ввод с клавиатуры в WebView2).
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const INPUT = 'input[role="combobox"][aria-label="Быстрый переход"]';

/** Где фокус: для сообщений об ошибке. */
async function focusInfo(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => {
    const element = document.activeElement;
    if (!element) return 'нет';
    return `${element.tagName.toLowerCase()} role=${element.getAttribute('role') ?? ''} label=${element.getAttribute('aria-label') ?? ''}`;
  });
}

async function focusInInput(browser: WebdriverIO.Browser): Promise<boolean> {
  return browser.execute((selector) => document.activeElement === document.querySelector(selector), INPUT);
}

async function inputValue(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute((selector) => document.querySelector<HTMLInputElement>(selector)?.value ?? '', INPUT);
}

async function selectedName(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => document.querySelector('[role="option"][aria-selected="true"] .name')?.textContent ?? '');
}

async function optionNames(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('[role="option"] .name')].map((element) => element.textContent ?? ''),
  );
}

describe('быстрый переход на настоящей клавиатуре движка', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createTestVault('Переход', {
      'Встреча планирования.md': '# Встреча планирования\n',
      'Встреча с отделом.md': '# Встреча с отделом\n',
      'Начало.md': '# Начало\n',
    });
    session = await startApp({ vault });
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Начало"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('Ctrl+O из открытой заметки: фокус в поле; Tab подставляет имя выбранного стрелкой пункта и не уводит фокус', async () => {
    const { browser } = session;
    try {
      await browser.$('[role="treeitem"][aria-label="Начало"]').click();
      await browser.$('.cm-content').waitForDisplayed({ timeout: 10_000 });
      await browser.$('.cm-content').click();
      await browser.keys(['Control', 'o']);
      await browser.$(INPUT).waitForDisplayed({ timeout: 5_000 });
      expect(await focusInInput(browser), `фокус после открытия: ${await focusInfo(browser)}`).toBe(true);

      await browser.keys('встр');
      await browser.waitUntil(async () => (await optionNames(browser)).length >= 2, {
        timeout: 5_000,
        timeoutMsg: 'нет двух пунктов «Встреча…»',
      });
      await browser.keys('ArrowDown');
      const chosen = await selectedName(browser);
      expect(chosen.startsWith('Встреча')).toBe(true);

      await browser.keys('Tab');
      expect(await focusInInput(browser), `фокус после Tab: ${await focusInfo(browser)}`).toBe(true);
      expect(await inputValue(browser)).toBe(chosen);

      await browser.keys(['Shift', 'Tab']);
      expect(await focusInInput(browser), `фокус после Shift+Tab: ${await focusInfo(browser)}`).toBe(true);
      expect(await inputValue(browser)).toBe(chosen);
    } finally {
      await saveScreenshot(browser, 'switcher-tab');
      await browser.keys('Escape');
    }
  });

  it('открыт кнопкой на ленте: Tab сразу подставляет имя первого пункта, фокус в поле', async () => {
    const { browser } = session;
    try {
      await browser.$(INPUT).waitForExist({ reverse: true, timeout: 5_000 });
      await browser.$('button[aria-label="Быстрый переход (Ctrl+O)"]').click();
      await browser.$(INPUT).waitForDisplayed({ timeout: 5_000 });
      await browser.waitUntil(async () => (await optionNames(browser)).length >= 3, {
        timeout: 5_000,
        timeoutMsg: 'список заметок не пришёл',
      });
      const first = await selectedName(browser);
      await browser.keys('Tab');
      expect(await focusInInput(browser), `фокус после Tab: ${await focusInfo(browser)}`).toBe(true);
      expect(await inputValue(browser)).toBe(first);
      // Повторные Tab по-прежнему не уводят фокус.
      await browser.keys('Tab');
      await browser.keys('Tab');
      expect(await focusInInput(browser), `фокус после трёх Tab: ${await focusInfo(browser)}`).toBe(true);
    } finally {
      await browser.keys('Escape');
    }
  });

  it('пояснение у неактивного пункта «Создать заметку» с «/» видно целиком (решение 50)', async () => {
    const { browser } = session;
    try {
      await browser.$(INPUT).waitForExist({ reverse: true, timeout: 5_000 });
      await browser.keys(['Control', 'o']);
      await browser.$(INPUT).waitForDisplayed({ timeout: 5_000 });
      await browser.keys('Папка/Новая заметка с длинным именем');
      const reason = browser.$('[role="option"][aria-disabled="true"] .reason');
      await reason.waitForDisplayed({ timeout: 5_000 });
      const text = await reason.getProperty('textContent');
      expect(text).toBe('Косая черта в имени недопустима: сначала создайте папку');
      const clipped = await browser.execute(() => {
        const element = document.querySelector<HTMLElement>('[role="option"][aria-disabled="true"] .reason');
        if (!element) return 'нет пояснения';
        return element.scrollWidth > element.clientWidth + 1 || element.scrollHeight > element.clientHeight + 1
          ? `обрезано: ${element.scrollWidth}×${element.scrollHeight} в ${element.clientWidth}×${element.clientHeight}`
          : '';
      });
      expect(clipped).toBe('');
    } finally {
      await saveScreenshot(browser, 'switcher-create-blocked');
      await browser.keys('Escape');
    }
  });

  it('шестерёнка на ленте явно неактивна, с подсказкой', async () => {
    const { browser } = session;
    const gear = browser.$('[role="toolbar"] button[aria-label^="Настройки"]');
    await gear.waitForDisplayed();
    expect(await gear.getAttribute('aria-disabled')).toBe('true');
    expect(await gear.getAttribute('title')).toBe('Настройки появятся в следующей версии');
  });
});
