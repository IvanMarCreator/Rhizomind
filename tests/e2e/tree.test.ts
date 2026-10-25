// Сценарий задачи 2.3: дерево файлов (С3.2) на настоящем ядре — показ всех
// файлов, создание как в Obsidian, F2 с проверкой имени, перетаскивание,
// удаление в корзину, внешнее изменение папки (С3.6) и снимки К2 в двух темах.
//
// ПРОПУЩЕН до слияния задачи 1.7 (docs/plans/2026-10-24-release-0.1-plan.md,
// «1.7. Команды ядра на настоящих модулях»): окно сценарного теста открывается
// без хранилища (tests/e2e/shell.test.ts), а открыть тестовое хранилище без
// системного диалога выбора папки и получать события `vault:changed` можно
// будет только с 1.7 (и списком хранилищ 1.3). После слияния оркестратор
// включает тест: убирает `.skip` и реализует openTestVault() тем способом,
// который даст 1.7 (например, заранее записанный список хранилищ во временном
// профиле с этим хранилищем последним открытым).
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

/** Открыть хранилище `root` в окне приложения — способ даст задача 1.7. */
async function openTestVault(_session: Session, _root: string): Promise<void> {
  throw new Error('Открытие тестового хранилища появится с задачей 1.7');
}

const row = (browser: WebdriverIO.Browser, name: string) => browser.$(`[role="treeitem"][aria-label="${name}"]`);

async function rowNames(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('[role="treeitem"]')].map((element) => element.getAttribute('aria-label') ?? ''),
  );
}

describe.skip('дерево файлов на настоящем ядре (включить после 1.7)', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createVault();
    session = await startApp();
    await openTestVault(session, vault);
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

      await field.setValue('План: работ');
      const alert = browser.$('[role="alert"]');
      await alert.waitForDisplayed();
      await saveScreenshot(browser, 'tree-rename-error');
      await browser.keys('Enter');
      expect(await field.isDisplayed()).toBe(true);

      await field.setValue('План работ');
      await alert.waitForDisplayed({ reverse: true });
      await browser.keys('Enter');
      await field.waitForDisplayed({ reverse: true });
      expect(existsSync(path.join(vault, 'Проекты', 'План работ.md'))).toBe(true);
      expect(existsSync(path.join(vault, 'Проекты', 'Без названия.md'))).toBe(false);
      // Новое имя сразу и в пути над заметкой.
      expect(await browser.$('nav[aria-label="Путь к заметке"]').getText()).toContain('План работ');
    } finally {
      await saveScreenshot(browser, 'tree-created');
    }
  });

  it('перетаскивание переносит файл в папку', async () => {
    const { browser } = session;
    await row(browser, 'ТЗ').dragAndDrop(row(browser, 'Входящие'));
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
