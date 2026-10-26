// Сценарии С3.1 (задача 2.8а) на настоящем ядре: первый запуск — окно
// «Управление хранилищами» с пустым списком (Э11б); создание хранилища (Э11в);
// «Открыть папку как хранилище»; переключатель слева внизу (Э10б) открывает
// другое хранилище в новом окне, а уже открытое — фокусом, без второго окна.
//
// Системное окно выбора папки тестовая сборка заменяет ответом из переменной
// RHIZOMIND_E2E_PICK_FOLDER (driver.ts, pickFolder; только функция webdriver).
// Снимки экрана — для QA: сравнение с кадрами Э10б, Э11а–Э11в в двух темах.
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const MANAGER = '[data-testid="vault-manager"]';
const SHELL = '[data-testid="shell"]';

/** Снимок после загрузки шрифтов (новое окно рисует текст не сразу). */
async function shot(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await browser.execute(async () => {
    await document.fonts.ready;
  });
  await saveScreenshot(browser, name);
}

/** Снимки в светлой и тёмной теме (тема — атрибутом data-theme, как в src/theme/theme.ts). */
async function screenshotThemes(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await browser.execute(async () => {
    await document.fonts.ready;
  });
  for (const theme of ['light', 'dark'] as const) {
    await browser.execute((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    await saveScreenshot(browser, `${name}-${theme}`);
  }
  await browser.execute(() => {
    delete document.documentElement.dataset.theme;
  });
}

/**
 * Ждёт, пока у приложения станет `count` окон, и переходит в окно, где
 * показан `selector` (новые окна WebDriver видит как новые вкладки).
 */
async function switchToWindowWith(
  browser: WebdriverIO.Browser,
  selector: string,
  count: number,
  text?: string,
): Promise<string> {
  let found = '';
  await browser.waitUntil(
    async () => {
      const handles = await browser.getWindowHandles();
      if (handles.length !== count) return false;
      for (const handle of handles) {
        await browser.switchToWindow(handle);
        const element = await browser.$(selector);
        if (!(await element.isExisting())) continue;
        if (text && !(await element.getText()).includes(text)) continue;
        found = handle;
        return true;
      }
      return false;
    },
    { timeout: 30_000, interval: 500, timeoutMsg: `нет окна ${selector} (${text ?? ''}) среди ${count}` },
  );
  return found;
}

const switcherLabel = (name: string) => `button[aria-label="Хранилище «${name}»"]`;

/** Пункты открытого меню переключателя: текст без лишних пробелов и отметка. */
function menuItems(browser: WebdriverIO.Browser): Promise<{ text: string; checked: string | null }[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('[role="menuitemradio"], [role="menuitem"]')].map((item) => ({
      text: (item.textContent ?? '').replace(/\s+/g, ' ').trim(),
      checked: item.getAttribute('aria-checked'),
    })),
  );
}

/** Нажимает пункт открытого меню, текст которого содержит `text`. */
async function clickMenuItem(browser: WebdriverIO.Browser, text: string): Promise<void> {
  const index = (await menuItems(browser)).findIndex((item) => item.text.includes(text));
  if (index < 0) throw new Error(`нет пункта меню «${text}»`);
  const items = await browser.$$('[role="menuitemradio"], [role="menuitem"]');
  await items[index]?.click();
}

describe('первый запуск и создание хранилища', () => {
  let session: Session;
  let parent: string;

  beforeAll(async () => {
    parent = mkdtempSync(path.join(tmpdir(), 'rhizomind-e2e-parent-'));
    session = await startApp({ pickFolder: parent });
    await session.browser.$(MANAGER).waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (parent) rmSync(parent, { recursive: true, force: true });
  });

  it('первый запуск — окно «Управление хранилищами» с пустым списком (Э11б)', async () => {
    const { browser } = session;
    try {
      const manager = browser.$(MANAGER);
      expect(await manager.getText()).toContain('Пока нет хранилищ');
      expect(await browser.$('button=Создать').isDisplayed()).toBe(true);
      expect(await browser.$('button[aria-label="Открыть папку как хранилище"]').isDisplayed()).toBe(true);
      expect(await browser.$$('li').length).toBe(0);
    } finally {
      await screenshotThemes(browser, 'vaults-first-run');
    }
  });

  it('создание (Э11в): имя → «Создать» → хранилище в новом окне, окно управления закрыто', async () => {
    const { browser } = session;
    await browser.$('button=Создать').click();
    const field = browser.$('input[type="text"]');
    await field.waitForDisplayed();
    await field.setValue('Проекты отдела');
    await screenshotThemes(browser, 'vaults-create-form');

    await browser.$('button=Создать').click();

    await switchToWindowWith(browser, SHELL, 1);
    await browser.$(switcherLabel('Проекты отдела')).waitForDisplayed({ timeout: 20_000 });
    const root = path.join(parent, 'Проекты отдела');
    expect(existsSync(path.join(root, '.rhizomind'))).toBe(true);
    await shot(browser, 'vaults-created');
  });
});

describe('«Открыть папку как хранилище» и переключатель', () => {
  let session: Session;
  let work: string;
  let rules: string;
  let archive: string;

  beforeAll(async () => {
    work = createTestVault('Рабочая база', { 'Встреча.md': '# Встреча\n' });
    rules = createTestVault('Регламенты отдела', { 'Регламент доступа.md': '' });
    archive = createTestVault('Архив 2025', { 'Итоги.md': '' });
    // В списке — «Регламенты отдела» и «Рабочая база»; открывается последнее.
    session = await startApp({ vault: [rules, work], pickFolder: archive });
    await session.browser.$(switcherLabel('Рабочая база')).waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    for (const root of [work, rules, archive]) if (root) removeTestVault(root);
  });

  it('переключатель (Э10б): хранилища списка, текущее отмечено, «Управление хранилищами…»', async () => {
    const { browser } = session;
    await browser.$(switcherLabel('Рабочая база')).click();
    const menu = browser.$('[role="menu"]');
    await menu.waitForDisplayed();
    expect(await menuItems(browser)).toEqual([
      { text: 'Р Регламенты отдела', checked: 'false' },
      { text: 'Р Рабочая база', checked: 'true' },
      { text: 'Управление хранилищами…', checked: null },
    ]);
    await screenshotThemes(browser, 'vaults-switcher');
  });

  it('другое хранилище из переключателя — в новом окне', async () => {
    const { browser } = session;
    await clickMenuItem(browser, 'Регламенты отдела');

    await switchToWindowWith(browser, switcherLabel('Регламенты отдела'), 2);
    await browser.$('[role="treeitem"][aria-label="Регламент доступа"]').waitForDisplayed({ timeout: 20_000 });
    await shot(browser, 'vaults-switched');
  });

  it('уже открытое хранилище — фокус на его окно, второго окна нет', async () => {
    const { browser } = session;
    await browser.$(switcherLabel('Регламенты отдела')).click();
    await browser.$('[role="menu"]').waitForDisplayed();
    await clickMenuItem(browser, 'Рабочая база');

    await browser.pause(1500);
    expect(await browser.getWindowHandles()).toHaveLength(2);
  });

  it('«Управление хранилищами…» — окно управления со списком (Э11а)', async () => {
    const { browser } = session;
    await browser.$(switcherLabel('Регламенты отдела')).click();
    await browser.$('[role="menu"]').waitForDisplayed();
    await clickMenuItem(browser, 'Управление хранилищами…');

    await switchToWindowWith(browser, MANAGER, 3);
    // Новое окно: список приходит отдельной командой, шрифт — файлом.
    await browser.waitUntil(async () => (await browser.$$('li').length) === 2, { timeout: 20_000 });
    const rows = await browser.execute(async () => {
      await document.fonts.ready;
      return [...document.querySelectorAll('li')].map((row) => (row.textContent ?? '').replace(/\s+/g, ' '));
    });
    expect(rows.join('\n')).toContain('Регламенты отдела');
    expect(rows.join('\n')).toContain('открыто');
    await screenshotThemes(browser, 'vaults-manager-list');

    // Меню «…» строки (Э11а): кнопки видны при наведении; «Убрать из списка» с пояснением.
    const more = browser.$('button[aria-label="Действия с хранилищем «Рабочая база»"]');
    await (await browser.$$('li'))[1]?.moveTo();
    await more.waitForClickable();
    await more.click();
    const menu = browser.$('[role="menu"]');
    await menu.waitForDisplayed();
    expect(await menu.getText()).toContain('Файлы и папка на диске не удаляются');
    await screenshotThemes(browser, 'vaults-manager-menu');
    await browser.keys('Escape');
    await menu.waitForDisplayed({ reverse: true });
  });

  it('«Открыть папку как хранилище» — хранилище в новом окне, окно управления закрыто', async () => {
    const { browser } = session;
    await browser.$('button[aria-label="Открыть папку как хранилище"]').click();

    // Окно управления закрылось, появилось окно «Архив 2025»: окон снова три.
    await switchToWindowWith(browser, switcherLabel('Архив 2025'), 3);
    expect(existsSync(path.join(archive, '.rhizomind'))).toBe(true);
    await shot(browser, 'vaults-opened-folder');
  });
});
