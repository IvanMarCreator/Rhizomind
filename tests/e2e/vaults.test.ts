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

/** Снимки в светлой и тёмной теме (тема — атрибутом data-theme, как в src/theme/theme.ts). */
async function screenshotThemes(browser: WebdriverIO.Browser, name: string): Promise<void> {
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
    await saveScreenshot(browser, 'vaults-created');
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
    const items = await browser.$$('[role="menuitemradio"]').map(async (item) => ({
      text: (await item.getText()).replace(/\s+/g, ' ').trim(),
      checked: await item.getAttribute('aria-checked'),
    }));
    expect(items).toEqual([
      { text: 'Р Регламенты отдела', checked: 'false' },
      { text: 'Р Рабочая база', checked: 'true' },
    ]);
    expect(await browser.$('[role="menuitem"]').getText()).toContain('Управление хранилищами…');
    await screenshotThemes(browser, 'vaults-switcher');
  });

  it('другое хранилище из переключателя — в новом окне', async () => {
    const { browser } = session;
    await browser.$('[role="menuitemradio"]').click(); // «Регламенты отдела»

    await switchToWindowWith(browser, switcherLabel('Регламенты отдела'), 2);
    await browser.$('[role="treeitem"][aria-label="Регламент доступа"]').waitForDisplayed({ timeout: 20_000 });
    await saveScreenshot(browser, 'vaults-switched');
  });

  it('уже открытое хранилище — фокус на его окно, второго окна нет', async () => {
    const { browser } = session;
    await browser.$(switcherLabel('Регламенты отдела')).click();
    await browser.$('[role="menu"]').waitForDisplayed();
    await browser.$('[role="menuitemradio"][aria-checked="false"]').click(); // «Рабочая база»

    await browser.pause(1500);
    expect(await browser.getWindowHandles()).toHaveLength(2);
  });

  it('«Управление хранилищами…» — окно управления со списком (Э11а)', async () => {
    const { browser } = session;
    await browser.$(switcherLabel('Регламенты отдела')).click();
    await browser.$('[role="menuitem"]').click();

    await switchToWindowWith(browser, MANAGER, 3);
    const rows = await browser.$$('li').map((row) => row.getText());
    expect(rows).toHaveLength(2);
    expect(rows.join('\n')).toContain('Регламенты отдела');
    expect(rows.join('\n')).toContain('открыто');
    await screenshotThemes(browser, 'vaults-manager-list');
  });

  it('«Открыть папку как хранилище» — хранилище в новом окне, окно управления закрыто', async () => {
    const { browser } = session;
    await browser.$('button[aria-label="Открыть папку как хранилище"]').click();

    // Окно управления закрылось, появилось окно «Архив 2025»: окон снова три.
    await switchToWindowWith(browser, switcherLabel('Архив 2025'), 3);
    expect(existsSync(path.join(archive, '.rhizomind'))).toBe(true);
    await saveScreenshot(browser, 'vaults-opened-folder');
  });
});
