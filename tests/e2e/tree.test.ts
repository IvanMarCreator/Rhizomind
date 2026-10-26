// Сценарий задачи 2.3: дерево файлов (С3.2) на настоящем ядре — показ всех
// файлов, создание как в Obsidian, F2 с проверкой имени, перетаскивание,
// удаление в корзину, внешнее изменение папки (С3.6) и снимки К2 в двух темах.
//
// Тестовое хранилище открывается при запуске тестовой сборки без системного
// окна выбора папки: driver.ts передаёт его папку переменной
// RHIZOMIND_E2E_VAULT (только сборка с функцией `webdriver`, задача 1.3).
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

/** Тестовое хранилище: папки, заметки с числами в именах, вложения, скрытые элементы. */
function createVault(): string {
  const root = mkdtempSync(path.join(tmpdir(), 'rhizomind-e2e-tree-'));
  const files: Record<string, string> = {
    'Проекты/Альфа/Встреча.md': '# Встреча\n',
    'Проекты/Альфа/схема-процесса.png': 'PNG',
    'Проекты/Бета/План.md': '',
    'Входящие/Заметка 10.md': '',
    'Входящие/Заметка 2.md': '',
    'ТЗ.pdf': '%PDF-1.4',
    'Начало.md': '# Начало\n',
    '.скрытая/секрет.md': '',
  };
  for (const [name, text] of Object.entries(files)) {
    const file = path.join(root, ...name.split('/'));
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, text);
  }
  return root;
}

/**
 * Ввод в поле имени без `setValue`: команда WebDriver «очистить поле» по
 * стандарту снимает с поля фокус, а поле имени по потере фокуса завершает
 * переименование. Поэтому — выделить всё и набрать текст.
 */
async function typeName(browser: WebdriverIO.Browser, text: string): Promise<void> {
  await browser.keys(['Control', 'a']);
  await browser.keys(text);
}

/**
 * Перетаскивание строки дерева событиями HTML5 (dragstart → dragover → drop):
 * действия WebDriver не порождают событий перетаскивания HTML5 ни в
 * msedgedriver, ни в WebKitWebDriver.
 */
async function dragRow(browser: WebdriverIO.Browser, from: string, to: string): Promise<void> {
  await browser.execute(
    (source, target) => {
      const find = (name: string) => {
        const element = document.querySelector(`[role="treeitem"][aria-label="${name}"]`);
        if (!element) throw new Error(`нет строки ${name}`);
        return element;
      };
      const data = new DataTransfer();
      const fire = (element: Element, type: string) =>
        element.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: data }));
      const [start, end] = [find(source), find(target)];
      fire(start, 'dragstart');
      fire(end, 'dragenter');
      fire(end, 'dragover');
      fire(end, 'drop');
      fire(start, 'dragend');
    },
    from,
    to,
  );
}

const row = (browser: WebdriverIO.Browser, name: string) => browser.$(`[role="treeitem"][aria-label="${name}"]`);

async function rowNames(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('[role="treeitem"]')].map((element) => element.getAttribute('aria-label') ?? ''),
  );
}

describe('дерево файлов на настоящем ядре', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createVault();
    session = await startApp({ vault });
    await session.browser.$('[role="tree"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) rmSync(vault, { recursive: true, force: true });
  });

  it('все файлы, папки сверху, естественный порядок; скрытое не видно', async () => {
    const { browser } = session;
    try {
      expect(await rowNames(browser)).toEqual(['Входящие', 'Проекты', 'Начало', 'ТЗ']);
      await row(browser, 'Входящие').click();
      expect(await rowNames(browser)).toEqual(['Входящие', 'Заметка 2', 'Заметка 10', 'Проекты', 'Начало', 'ТЗ']);
      expect(await row(browser, 'ТЗ').getText()).toContain('PDF');
    } finally {
      await saveScreenshot(browser, 'tree-list');
    }
  });

  it('новая заметка кнопкой при наведении: файл на диске, поле имени, проверка имени, переименование', async () => {
    const { browser } = session;
    try {
      await row(browser, 'Проекты').moveTo();
      await row(browser, 'Проекты').$('button[aria-label="Новая заметка"]').click();
      const field = browser.$('input[aria-label="Новое имя"]');
      await field.waitForDisplayed();
      expect(await field.getValue()).toBe('Без названия');
      expect(existsSync(path.join(vault, 'Проекты', 'Без названия.md'))).toBe(true);

      await typeName(browser, 'План: работ');
      const alert = browser.$('[role="alert"]');
      await alert.waitForDisplayed();
      await saveScreenshot(browser, 'tree-rename-error');
      await browser.keys('Enter');
      expect(await field.isDisplayed()).toBe(true);

      await typeName(browser, 'План работ');
      await alert.waitForDisplayed({ reverse: true });
      await browser.keys('Enter');
      await field.waitForDisplayed({ reverse: true });
      expect(existsSync(path.join(vault, 'Проекты', 'План работ.md'))).toBe(true);
      expect(existsSync(path.join(vault, 'Проекты', 'Без названия.md'))).toBe(false);
      // Новое имя сразу и в пути над заметкой.
      // textContent, а не getText: WebKitWebDriver у строки пути с flex
      // отдаёт только разделитель «/» (на снимке путь виден целиком).
      const pathText = await browser.execute(
        () => document.querySelector('nav[aria-label="Путь к заметке"]')?.textContent ?? '',
      );
      expect(pathText).toContain('План работ');
    } finally {
      await saveScreenshot(browser, 'tree-created');
    }
  });

  it('Ctrl+Z в дереве отменяет последнее переименование (решение 47)', async () => {
    const { browser } = session;
    await row(browser, 'План работ').click();
    await browser.keys(['Control', 'z']);
    await browser.waitUntil(() => existsSync(path.join(vault, 'Проекты', 'Без названия.md')), { timeout: 5_000 });
    expect(existsSync(path.join(vault, 'Проекты', 'План работ.md'))).toBe(false);
  });

  it('кнопки корня — в заголовке «ФАЙЛЫ» при наведении на панель (решение 48)', async () => {
    const { browser } = session;
    await browser.$('aside').moveTo();
    await browser.$('[role="group"][aria-label="Создать в корне хранилища"] button[aria-label="Новая папка"]').click();
    await browser.waitUntil(() => existsSync(path.join(vault, 'Без названия')), { timeout: 5_000 });
    await browser.keys('Escape');
    await saveScreenshot(browser, 'tree-root-actions');
  });

  it('перетаскивание переносит файл в папку', async () => {
    const { browser } = session;
    await dragRow(browser, 'ТЗ', 'Входящие');
    await browser.waitUntil(() => existsSync(path.join(vault, 'Входящие', 'ТЗ.pdf')), { timeout: 5_000 });
    expect(existsSync(path.join(vault, 'ТЗ.pdf'))).toBe(false);
  });

  it('Delete с подтверждением убирает файл из хранилища', async () => {
    const { browser } = session;
    await row(browser, 'Начало').click();
    await browser.keys('Delete');
    const dialog = browser.$('[role="dialog"]');
    await dialog.waitForDisplayed();
    await saveScreenshot(browser, 'tree-delete-confirm');
    await dialog.$('button=Удалить').click();
    await browser.waitUntil(() => !existsSync(path.join(vault, 'Начало.md')), { timeout: 5_000 });
    await row(browser, 'Начало').waitForExist({ reverse: true });
  });

  it('изменение папки снаружи видно в дереве в течение секунды (С3.6)', async () => {
    const { browser } = session;
    writeFileSync(path.join(vault, 'Снаружи.md'), 'текст\n');
    await row(browser, 'Снаружи').waitForExist({ timeout: 1_500 });
    rmSync(path.join(vault, 'Снаружи.md'));
    await row(browser, 'Снаружи').waitForExist({ timeout: 1_500, reverse: true });
  });

  it.each(['light', 'dark'] as const)('тема %s: снимок дерева для сверки с Э1', async (theme) => {
    const { browser } = session;
    await browser.execute((name) => {
      document.documentElement.dataset.theme = name;
    }, theme);
    await row(browser, 'Проекты').moveTo();
    await saveScreenshot(browser, `tree-${theme}`);
  });
});
