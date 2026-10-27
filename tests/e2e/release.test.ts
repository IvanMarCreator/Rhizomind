// Сценарий выпуска 0.1 от начала до конца (задача 3.1, спецификация С3.1–С3.5;
// карта покрытия — docs/reviews/2026-10-26-task-3.1.md). Один пользовательский
// путь на настоящем ядре, шаги идут по порядку и опираются на предыдущие:
// первый запуск → создание хранилища → дерево (создание, переименование тремя
// способами, проверка имени, Ctrl+Z, перетаскивание, удаление) → редактор
// (ввод, индикатор, автосохранение, запись при переходе) → быстрый переход и
// история → второе хранилище и переключатель → перезапуск программы: открыто
// последнее хранилище и последняя заметка. Снимок экрана — на каждом шаге
// (`release-NN-…`), ключевые экраны — в светлой и тёмной теме.
//
// Тест независим от тестов разработчиков: проверяет то, что видит
// пользователь, по спецификации. Уже покрытое отдельными сценариями
// (закрытие окна сразу после ввода — close.test.ts, повторный запуск —
// vaults.test.ts и single-instance.test.ts, настоящее перетаскивание мышью —
// src-tauri/tests/webview2_runtime.rs) здесь не повторяется.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { applicationPath, createTestProfile, startApp, waitForAppExit, type Session } from './driver.ts';
import { appWindowTitles } from './window-titles.ts';
import { saveScreenshot } from './screenshot.ts';

const MANAGER = '[data-testid="vault-manager"]';
const SHELL = '[data-testid="shell"]';
const TEXT = '[role="textbox"][aria-label="Текст заметки"]';
const STATUS = '[data-testid="save-status"]';
const NAME_FIELD = 'input[aria-label="Новое имя"]';
const SWITCHER = 'input[role="combobox"][aria-label="Быстрый переход"]';
const ROOT_ACTIONS = '[role="group"][aria-label="Создать в корне хранилища"]';

const isWindows = process.platform === 'win32';

const switcherLabel = (name: string) => `button[aria-label="Хранилище «${name}»"]`;
const row = (browser: WebdriverIO.Browser, name: string) => browser.$(`[role="treeitem"][aria-label="${name}"]`);

let step = 0;

/** Снимок шага после загрузки шрифтов: `release-NN-<имя>`. */
async function shot(browser: WebdriverIO.Browser, name: string): Promise<void> {
  step += 1;
  await browser.execute(async () => {
    await document.fonts.ready;
  });
  await saveScreenshot(browser, `release-${String(step).padStart(2, '0')}-${name}`);
}

/** Снимок шага в светлой и тёмной теме (тема — атрибутом data-theme, как в src/theme/theme.ts). */
async function shotThemes(browser: WebdriverIO.Browser, name: string): Promise<void> {
  step += 1;
  await browser.execute(async () => {
    await document.fonts.ready;
  });
  for (const theme of ['light', 'dark'] as const) {
    await browser.execute((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    await saveScreenshot(browser, `release-${String(step).padStart(2, '0')}-${name}-${theme}`);
  }
  await browser.execute(() => {
    delete document.documentElement.dataset.theme;
  });
}

/** Ждёт `count` окон и переходит в то, где есть `selector` (окна WebDriver видит вкладками). */
async function switchToWindowWith(browser: WebdriverIO.Browser, selector: string, count: number): Promise<void> {
  await browser.waitUntil(
    async () => {
      const handles = await browser.getWindowHandles();
      if (handles.length !== count) return false;
      for (const handle of handles) {
        await browser.switchToWindow(handle);
        if (await browser.$(selector).isExisting()) return true;
      }
      return false;
    },
    { timeout: 30_000, interval: 500, timeoutMsg: `нет окна ${selector} среди ${count}` },
  );
}

/** Нажимает пункт открытого меню, текст которого содержит `text`. */
async function clickMenuItem(browser: WebdriverIO.Browser, text: string): Promise<void> {
  const texts = await browser.execute(() =>
    [...document.querySelectorAll('[role="menuitemradio"], [role="menuitem"]')].map((item) =>
      (item.textContent ?? '').replace(/\s+/g, ' ').trim(),
    ),
  );
  const index = texts.findIndex((item) => item.includes(text));
  if (index < 0) throw new Error(`нет пункта меню «${text}» среди ${texts.join(' | ')}`);
  const items = await browser.$$('[role="menuitemradio"], [role="menuitem"]');
  await items[index]?.click();
}

/**
 * Ввод в поле имени без `setValue`: «очистить поле» WebDriver снимает фокус, а
 * поле имени по потере фокуса завершает переименование.
 */
async function typeName(browser: WebdriverIO.Browser, text: string): Promise<void> {
  await browser.keys(['Control', 'a']);
  await browser.keys(text);
}

/** Путь над заметкой (textContent: WebKitWebDriver отдаёт getText строки с flex не целиком). */
function headerPath(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() =>
    (document.querySelector('nav[aria-label="Путь к заметке"]')?.textContent ?? '').replace(/\s+/g, ' ').trim(),
  );
}

function editorText(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(
    (selector) =>
      [...document.querySelectorAll(`${selector} .cm-line`)].map((line) => line.textContent ?? '').join('\n'),
    TEXT,
  );
}

async function statusText(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute((selector) => document.querySelector(selector)?.textContent?.trim() ?? '', STATUS);
}

/** Заголовок и текст заметки видны на экране (не нулевого размера, в пределах окна). */
function noteVisible(browser: WebdriverIO.Browser): Promise<{ title: string; text: string }> {
  return browser.execute((selector) => {
    const box = (element: Element | null) => {
      if (!element) return 'нет элемента';
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      if (rect.width < 1 || rect.height < 1) return `нулевой размер ${rect.width}×${rect.height}`;
      if (rect.bottom < 0 || rect.top > window.innerHeight) return `вне окна: top=${rect.top}`;
      if (style.visibility === 'hidden' || style.opacity === '0') return 'скрыт стилем';
      return '';
    };
    return { title: box(document.querySelector('main h1')), text: box(document.querySelector(`${selector} .cm-line`)) };
  }, TEXT);
}

/** Начать запись смены индикатора сохранения (до ввода): какие состояния показывались. */
async function watchStatus(browser: WebdriverIO.Browser): Promise<void> {
  await browser.execute((selector) => {
    const seen: string[] = [];
    (window as unknown as { __statusSeen: string[] }).__statusSeen = seen;
    const record = () => {
      const text = document.querySelector(selector)?.textContent?.trim() ?? '';
      if (text && seen.at(-1) !== text) seen.push(text);
    };
    record();
    new MutationObserver(record).observe(document.body, { subtree: true, childList: true, characterData: true });
  }, STATUS);
}

function statusSeen(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute(() => (window as unknown as { __statusSeen?: string[] }).__statusSeen ?? []);
}

/** Перетаскивание строки событиями HTML5 (действия WebDriver их не порождают; см. tree.test.ts). */
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

async function waitForFile(file: string, present = true): Promise<void> {
  const deadline = Date.now() + 5_000;
  while (existsSync(file) !== present) {
    if (Date.now() > deadline) throw new Error(`${present ? 'нет файла' : 'файл не исчез'}: ${file}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

async function waitForContent(file: string, wanted: string, timeout = 5_000): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const text = existsSync(file) ? readFileSync(file, 'utf8') : '';
    if (text.includes(wanted)) return;
    if (Date.now() > deadline) throw new Error(`в ${file} нет «${wanted}»: ${JSON.stringify(text.slice(0, 200))}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/** Нажать кнопку «Новая заметка» / «Новая папка» в заголовке «ФАЙЛЫ» (видны при наведении на панель). */
async function createInRoot(browser: WebdriverIO.Browser, label: 'Новая заметка' | 'Новая папка'): Promise<void> {
  await browser.$('aside').moveTo();
  const button = browser.$(`${ROOT_ACTIONS} button[aria-label="${label}"]`);
  await button.waitForClickable({ timeout: 5_000 });
  await button.click();
  await browser.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
}

/** Завершить переименование в поле: Enter, поле закрылось. */
async function commitName(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await typeName(browser, name);
  await browser.$('[role="alert"]').waitForExist({ reverse: true, timeout: 5_000 });
  await browser.keys('Enter');
  await browser.$(NAME_FIELD).waitForExist({ reverse: true, timeout: 5_000 });
}

/** Сообщение под полем имени: ошибка (role=alert) или предупреждение (.warning). */
async function nameMessage(browser: WebdriverIO.Browser): Promise<string> {
  await browser.pause(300); // проверка имени — командой ядра
  return browser.execute(
    () =>
      document.querySelector('[role="alert"]')?.textContent?.trim() ??
      document.querySelector('.message.warning')?.textContent?.trim() ??
      '',
  );
}

// Заголовки окон ОС (находка Н2, задача 2.11): тестовая сборка добавляет к
// «Rhizomind» пометку (WEBDRIVER_BUILD_MARKER).
const TITLE_SUFFIX = ' — Rhizomind — ТЕСТОВАЯ СБОРКА (webdriver)';

/** Ждёт `count` показанных окон программы и возвращает их заголовки ОС. */
async function windowTitles(b: WebdriverIO.Browser, count: number, step: string): Promise<string[]> {
  let titles: string[] = [];
  await b.waitUntil(
    async () => {
      titles = appWindowTitles();
      return titles.length >= count;
    },
    { timeout: 10_000, timeoutMsg: `${step}: окон программы меньше ${count}: ${JSON.stringify(titles)}` },
  );
  // В журнал теста: видно, что заголовки прочитаны у ОС (X11 или UI Automation).
  // stderr: console.log успешных тестов vitest в журнал не выводит.
  process.stderr.write(`заголовки окон ОС (${process.platform}, ${step}): ${JSON.stringify(titles)}\n`);
  return titles;
}

/**
 * Ждёт, пока заголовки окон ОС станут `expected` (по алфавиту): заголовок с
 * заметкой ядро ставит после того, как окно сообщило показанную заметку
 * (задача 2.12, решение 82).
 */
async function waitForWindowTitles(b: WebdriverIO.Browser, expected: string[], step: string): Promise<void> {
  let titles: string[] = [];
  await b.waitUntil(
    async () => {
      titles = appWindowTitles();
      return JSON.stringify(titles) === JSON.stringify(expected);
    },
    { timeout: 10_000, timeoutMsg: `${step}: заголовки окон ${JSON.stringify(titles)}, ожидались ${JSON.stringify(expected)}` },
  );
  process.stderr.write(`заголовки окон ОС (${process.platform}, ${step}): ${JSON.stringify(titles)}\n`);
}

/** Ctrl+O, запрос — имя заметки, Enter: открыта заметка с путём `header` над текстом. */
async function openBySwitcher(b: WebdriverIO.Browser, name: string, header: string): Promise<void> {
  await b.keys(['Control', 'o']);
  await b.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
  await b.keys(name);
  await b.waitUntil(
    async () => (await b.execute(() => document.querySelector('[role="option"] .name')?.textContent)) === name,
    { timeout: 5_000, timeoutMsg: `быстрый переход не нашёл «${name}» первым` },
  );
  await b.keys('Enter');
  await b.$(SWITCHER).waitForExist({ reverse: true, timeout: 5_000 });
  await b.waitUntil(async () => (await headerPath(b)) === header, { timeout: 5_000 });
}

describe('сценарий выпуска 0.1: от первого запуска до перезапуска', () => {
  let session: Session | undefined;
  let parent = '';
  let profile = '';
  let work = '';
  let archive = '';

  beforeAll(async () => {
    parent = mkdtempSync(path.join(tmpdir(), 'rhizomind-e2e-release-'));
    profile = createTestProfile();
    work = path.join(parent, 'Рабочая база');
    archive = path.join(parent, 'Архив');
    session = await startApp({ profile, pickFolder: parent });
    await session.browser.$(MANAGER).waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (parent) rmSync(parent, { recursive: true, force: true });
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  const browser = () => {
    if (!session) throw new Error('нет сессии');
    return session.browser;
  };

  // ---- С3.1: первый запуск и создание хранилища ----------------------------

  it('С3.1: первый запуск — окно «Управление хранилищами» с пустым списком', async () => {
    const b = browser();
    expect(await b.$(MANAGER).getText()).toContain('Пока нет хранилищ');
    expect(await b.$$('li').length).toBe(0);
    await shotThemes(b, 'first-run');
    expect(await windowTitles(b, 1, 'первый запуск')).toEqual([`Управление хранилищами${TITLE_SUFFIX}`]);
  });

  it('С3.1: «Создать хранилище» → имя → «Обзор…» → «Создать»: окно хранилища, папка .rhizomind, пустое дерево', async () => {
    const b = browser();
    await b.$('button=Создать').click();
    const field = b.$('input:not([readonly])');
    await field.waitForDisplayed();
    await field.setValue('Рабочая база');
    await b.$('button=Обзор…').click();
    await b.waitUntil(async () => (await b.$('input[readonly]').getValue()) !== '', { timeout: 10_000 });
    await shot(b, 'create-form');
    await b.$('button=Создать').click();

    await switchToWindowWith(b, SHELL, 1);
    await b.$(switcherLabel('Рабочая база')).waitForDisplayed({ timeout: 20_000 });
    expect(existsSync(path.join(work, '.rhizomind'))).toBe(true);
    await b.$('[role="tree"]').waitForDisplayed({ timeout: 10_000 });
    expect(await b.$('aside').getText()).toContain('В хранилище пока нет файлов.');
    await shotThemes(b, 'vault-created');
  });

  // ---- С3.2: дерево файлов ---------------------------------------------------

  it('С3.2: новая заметка в корне — «Без названия», поле имени сразу открыто, заметка открыта', async () => {
    const b = browser();
    await createInRoot(b, 'Новая заметка');
    expect(await b.$(NAME_FIELD).getValue()).toBe('Без названия');
    await waitForFile(path.join(work, 'Без названия.md'));
    await shot(b, 'note-created');
    await commitName(b, 'Встреча');
    await waitForFile(path.join(work, 'Встреча.md'));
    expect(existsSync(path.join(work, 'Без названия.md'))).toBe(false);
    expect(await headerPath(b)).toBe('Встреча');
    // Новые заметки — UTF-8 без BOM (С3.3): пустой файл.
    expect(readFileSync(path.join(work, 'Встреча.md')).length).toBe(0);
  });

  it('С3.2: новая папка и вторая заметка; сортировка — папки сверху', async () => {
    const b = browser();
    await createInRoot(b, 'Новая папка');
    await commitName(b, 'Проекты');
    await waitForFile(path.join(work, 'Проекты'));
    await createInRoot(b, 'Новая заметка');
    await commitName(b, 'Итоги');
    await waitForFile(path.join(work, 'Итоги.md'));
    const names = await b.execute(() =>
      [...document.querySelectorAll('[role="treeitem"]')].map((element) => element.getAttribute('aria-label')),
    );
    expect(names).toEqual(['Проекты', 'Встреча', 'Итоги']);
    await shot(b, 'folder-and-notes');
  });

  it('С3.2: проверка имени при вводе — запрещённое, зарезервированное, занятое (без учёта регистра), «нельзя сослаться»', async () => {
    const b = browser();
    await row(b, 'Встреча').click();
    await b.keys('F2');
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });

    await typeName(b, 'План: работ');
    expect(await nameMessage(b)).toContain('нельзя использовать символы');
    await typeName(b, 'CON');
    expect(await nameMessage(b)).toContain('зарезервировано');
    await typeName(b, 'итоги');
    expect(await nameMessage(b)).toContain('уже есть элемент с таким именем');
    // Enter при ошибке не срабатывает.
    await b.keys('Enter');
    expect(await b.$(NAME_FIELD).isDisplayed()).toBe(true);
    await typeName(b, '#Тег');
    expect(await nameMessage(b)).toContain('На эту заметку нельзя будет сослаться');
    await shot(b, 'name-warning');
    await b.keys('Escape');
    await b.$(NAME_FIELD).waitForExist({ reverse: true, timeout: 5_000 });
    expect(existsSync(path.join(work, 'Встреча.md'))).toBe(true);
  });

  it('С3.2, решение 47: Ctrl+Z в открытом поле имени возвращает исходное имя', async () => {
    const b = browser();
    await row(b, 'Встреча').click();
    await b.keys('F2');
    const field = b.$(NAME_FIELD);
    await field.waitForDisplayed({ timeout: 5_000 });
    await typeName(b, 'Черновик');
    // Сначала обычная отмена набора браузером, затем — исходное имя.
    for (let i = 0; i < 4 && (await field.getValue()) !== 'Встреча'; i += 1) {
      await b.keys(['Control', 'z']);
      await b.pause(200);
    }
    expect(await field.getValue()).toBe('Встреча');
    await b.keys('Escape');
    await field.waitForExist({ reverse: true, timeout: 5_000 });
  });

  it('С3.2: переименование — способ 1, F2 в дереве: новое имя в дереве и в пути над заметкой', async () => {
    const b = browser();
    await row(b, 'Встреча').click();
    await b.keys('F2');
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
    await commitName(b, 'Встреча отдела');
    await waitForFile(path.join(work, 'Встреча отдела.md'));
    await row(b, 'Встреча отдела').waitForExist({ timeout: 5_000 });
    expect(await headerPath(b)).toBe('Встреча отдела');
    await shot(b, 'renamed-f2');
  });

  it('С3.2, решение 47: переименование — способ 2, двойной клик по строке дерева', async () => {
    const b = browser();
    await row(b, 'Встреча отдела').doubleClick();
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
    await commitName(b, 'Встреча команды');
    await waitForFile(path.join(work, 'Встреча команды.md'));
    expect(await headerPath(b)).toBe('Встреча команды');
    await shot(b, 'renamed-dblclick');
  });

  it('С3.2: переименование — способ 3, клик по заголовку заметки над текстом (Э4б)', async () => {
    const b = browser();
    await b.$('main h1 button[aria-label="Встреча команды"]').click();
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
    await shotThemes(b, 'title-rename-field');
    await commitName(b, 'Планёрка');
    await waitForFile(path.join(work, 'Планёрка.md'));
    await row(b, 'Планёрка').waitForExist({ timeout: 5_000 });
    expect(await headerPath(b)).toBe('Планёрка');
    await shot(b, 'renamed-title');
  });

  it('С3.2, решение 47: Ctrl+Z после переименования (фокус на заголовке) — прежнее имя', async () => {
    const b = browser();
    // После переименования из заголовка фокус возвращается на заголовок.
    await b.$('main h1 button[aria-label="Планёрка"]').click();
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
    await b.keys('Escape');
    await b.$(NAME_FIELD).waitForExist({ reverse: true, timeout: 5_000 });
    await b.keys(['Control', 'z']);
    await waitForFile(path.join(work, 'Встреча команды.md'));
    expect(existsSync(path.join(work, 'Планёрка.md'))).toBe(false);
    await row(b, 'Встреча команды').waitForExist({ timeout: 5_000 });
    expect(await headerPath(b)).toBe('Встреча команды');
    await shot(b, 'rename-undone');
  });

  it('С3.2: перетаскивание заметки в папку — файл перемещён, открытая заметка следует за путём', async () => {
    const b = browser();
    await dragRow(b, 'Встреча команды', 'Проекты');
    await waitForFile(path.join(work, 'Проекты', 'Встреча команды.md'));
    expect(existsSync(path.join(work, 'Встреча команды.md'))).toBe(false);
    await b.waitUntil(async () => (await headerPath(b)) === 'Проекты / Встреча команды', {
      timeout: 5_000,
      timeoutMsg: 'путь над заметкой не сменился',
    });
    await shot(b, 'moved');
  });

  // Меню открывается с клавиатуры (Shift+F10): после правого клика мышью
  // WebKitWebDriver перестаёт вводить не-ASCII символы в поля страницы
  // (проверено пробой: латиница вводится, кириллица — нет; в приложении ошибки
  // нет — тот же путь с клавиатуры работает). Правый клик — компонентные тесты дерева.
  it('С3.2: контекстное меню папки — создание, переименование, удаление; «Новая заметка» создаёт в этой папке', async () => {
    const b = browser();
    await row(b, 'Проекты').click();
    await b.keys(['Shift', 'F10']);
    const menu = b.$('[role="menu"]');
    await menu.waitForDisplayed({ timeout: 5_000 });
    const items = await b.execute(() =>
      [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map((item) =>
        (item.textContent ?? '').replace(/\s+/g, ' ').trim(),
      ),
    );
    expect(items).toEqual(['Новая заметка', 'Новая папка', 'Переименовать F2', 'Удалить Delete']);
    await shotThemes(b, 'context-menu');
    // Фокус — на первом пункте «Новая заметка».
    await b.keys('Enter');
    await b.$(NAME_FIELD).waitForDisplayed({ timeout: 5_000 });
    await waitForFile(path.join(work, 'Проекты', 'Без названия.md'));
    await commitName(b, 'Задачи');
    await waitForFile(path.join(work, 'Проекты', 'Задачи.md'));
    expect(await headerPath(b)).toBe('Проекты / Задачи');
    await row(b, 'Встреча команды').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Проекты / Встреча команды', { timeout: 5_000 });
  });

  it('С3.2: не-Markdown файлы видны с меткой типа и не открываются', async () => {
    const b = browser();
    writeFileSync(path.join(work, 'схема.png'), 'PNG');
    await row(b, 'схема').waitForExist({ timeout: 3_000 });
    expect(await row(b, 'схема').getText()).toContain('PNG');
    await row(b, 'схема').click();
    await b.pause(500);
    expect(await headerPath(b)).toBe('Проекты / Встреча команды');
    await shot(b, 'attachment');
  });

  // ---- С3.3: редактор ----------------------------------------------------------

  it('С3.3: ввод — индикатор «Сохраняется» → «Сохранено», автосохранение на диск', async () => {
    const b = browser();
    const file = path.join(work, 'Проекты', 'Встреча команды.md');
    await watchStatus(b);
    const text = b.$(TEXT);
    await text.waitForDisplayed({ timeout: 10_000 });
    await text.click();
    await b.keys('Решили запустить пилот');
    await waitForContent(file, 'Решили запустить пилот', 6_000);
    await b.waitUntil(async () => (await statusText(b)) === 'Сохранено', { timeout: 5_000 });
    const seen = await statusSeen(b);
    expect(seen, `состояния индикатора: ${seen.join(' → ')}`).toContain('Сохраняется');
    expect(seen.at(-1)).toBe('Сохранено');
    // Проверка орфографии движка выключена (может обращаться к сети).
    expect(await text.getAttribute('spellcheck')).toBe('false');
    // Новая заметка — перенос LF, без BOM.
    const bytes = readFileSync(file);
    expect(bytes.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf]))).toBe(false);
    expect(bytes.includes(Buffer.from('\r\n'))).toBe(false);
    expect(await noteVisible(b)).toEqual({ title: '', text: '' });
    // Длинная строка переносится в колонке 700 px (Э1), а не уходит за край.
    await b.keys(['Control', 'End']);
    await b.keys(' — ' + 'согласовать сроки, состав участников и порядок доступа '.repeat(4).trim());
    await b.waitUntil(async () => (await statusText(b)) === 'Сохранено', { timeout: 5_000 });
    const overflow = await b.execute((selector) => {
      const content = document.querySelector<HTMLElement>(selector);
      return content ? content.scrollWidth - content.clientWidth : -1;
    }, TEXT);
    expect(overflow).toBeLessThanOrEqual(1);
    await shotThemes(b, 'typed-saved');
  });

  it('С3.3: переход к другой заметке сразу после ввода — ввод записан', async () => {
    const b = browser();
    const file = path.join(work, 'Проекты', 'Встреча команды.md');
    await b.$(TEXT).click();
    await b.keys(['Control', 'End']);
    await b.keys(' до пятницы');
    // Без паузы: автосохранение (1 с) не успело, запись — при переходе.
    await row(b, 'Итоги').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Итоги', { timeout: 5_000 });
    await waitForContent(file, ' до пятницы');
    await shot(b, 'switched-note');
  });

  it('С3.3: файл больше лимита (10 МБ) открывается только для чтения с подсказкой', async () => {
    const b = browser();
    writeFileSync(path.join(work, 'Большой.md'), 'x'.repeat(11 * 1024 * 1024));
    await row(b, 'Большой').waitForExist({ timeout: 5_000 });
    await row(b, 'Большой').click();
    await b.waitUntil(async () => (await statusText(b)) === 'Только чтение', { timeout: 15_000 });
    expect(await b.$('main .notice .title').getText()).toContain('Только чтение: файл');
    await shot(b, 'read-only');
  });

  // ---- С3.5: быстрый переход и история ------------------------------------------

  it('С3.5: Ctrl+O — нечёткий поиск, Enter открывает', async () => {
    const b = browser();
    await row(b, 'Итоги').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Итоги', { timeout: 5_000 });
    await b.$(TEXT).click();
    await b.keys('Итоги недели');
    await b.waitUntil(async () => (await statusText(b)) === 'Сохранено', { timeout: 5_000 });

    await b.keys(['Control', 'o']);
    await b.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
    await b.keys('встркм');
    await b.waitUntil(
      () => b.execute(() => document.querySelector('[role="option"] .name')?.textContent === 'Встреча команды'),
      { timeout: 5_000, timeoutMsg: 'нечёткий поиск не нашёл «Встреча команды»' },
    );
    await shotThemes(b, 'quick-switcher');
    await b.keys('Enter');
    await b.$(SWITCHER).waitForExist({ reverse: true, timeout: 5_000 });
    await b.waitUntil(async () => (await headerPath(b)) === 'Проекты / Встреча команды', { timeout: 5_000 });
    expect(await editorText(b)).toContain(' до пятницы');
  });

  it('С3.5, решения 49 и 51: Ctrl+O — «Создать заметку „…“» последним пунктом, заметка создаётся и открывается', async () => {
    const b = browser();
    await b.keys(['Control', 'o']);
    await b.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
    await b.keys('Новая идея');
    const lastOption = () =>
      b.execute(() => {
        const options = document.querySelectorAll('[role="option"]');
        return (options[options.length - 1]?.textContent ?? '').replace(/\s+/g, ' ').trim();
      });
    await b.waitUntil(async () => (await lastOption()).includes('Создать заметку „Новая идея“'), {
      timeout: 5_000,
      timeoutMsg: 'нет пункта «Создать заметку» последним',
    });
    const create = b.$('[role="option"]:last-child');
    await shot(b, 'switcher-create');
    await create.click();
    await waitForFile(path.join(work, 'Новая идея.md'));
    await b.waitUntil(async () => (await headerPath(b)) === 'Новая идея', { timeout: 5_000 });
  });

  it('С3.5: Alt+← / Alt+→ и кнопки — назад и вперёд по истории окна', async () => {
    const b = browser();
    // История окна: … → «Итоги» → «Встреча команды» → «Новая идея».
    await b.$(TEXT).click();
    await b.keys(['Alt', 'ArrowLeft']);
    await b.waitUntil(async () => (await headerPath(b)) === 'Проекты / Встреча команды', {
      timeout: 5_000,
      timeoutMsg: `Alt+← не вернул: ${await headerPath(b)}`,
    });
    await b.$('button[aria-label="Назад (Alt+←)"]').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Итоги', { timeout: 5_000 });
    await shot(b, 'history-back');
    await b.$('button[aria-label="Вперёд (Alt+→)"]').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Проекты / Встреча команды', { timeout: 5_000 });
    await b.$(TEXT).click();
    await b.keys(['Alt', 'ArrowRight']);
    await b.waitUntil(async () => (await headerPath(b)) === 'Новая идея', { timeout: 5_000 });
  });

  // С3.5: «Ctrl+N: новая заметка» — команда реестра горячих клавиш (задача 2.8б,
  // находка Н1 закрыта); место — по настройке, по умолчанию корень хранилища.
  it('С3.5: Ctrl+N — новая заметка', async () => {
    const b = browser();
    await row(b, 'Итоги').click();
    await b.keys(['Control', 'n']);
    await waitForFile(path.join(work, 'Без названия.md'));
  });

  // ---- С3.2: удаление --------------------------------------------------------------

  it('С3.2: Delete — подтверждение, файл уходит из хранилища (в корзину ОС)', async () => {
    const b = browser();
    await row(b, 'Итоги').click();
    await b.keys('Delete');
    const dialog = b.$('[role="dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    expect(await dialog.getText()).toContain('Файл будет перемещён в корзину.');
    await shotThemes(b, 'delete-confirm');
    await dialog.$('button=Удалить').click();
    await waitForFile(path.join(work, 'Итоги.md'), false);
    await row(b, 'Итоги').waitForExist({ reverse: true, timeout: 5_000 });
    // Открытая заметка удалена в приложении — закрыта.
    await b.waitUntil(async () => (await b.$('main').getText()).includes('Нет открытой заметки'), { timeout: 5_000 });
    await shot(b, 'deleted');
  });

  // ---- С3.1: второе хранилище и переключатель ------------------------------------

  it('С3.1: «Управление хранилищами…» → создать второе хранилище — оно в новом окне', async () => {
    const b = browser();
    await b.$(switcherLabel('Рабочая база')).click();
    await b.$('[role="menu"]').waitForDisplayed({ timeout: 5_000 });
    await clickMenuItem(b, 'Управление хранилищами…');
    await switchToWindowWith(b, MANAGER, 2);
    await b.waitUntil(async () => (await b.$$('li').length) === 1, { timeout: 20_000 });
    await shot(b, 'manager-list');

    await b.$('button=Создать').click();
    const field = b.$('input:not([readonly])');
    await field.waitForDisplayed();
    await field.setValue('Архив');
    await b.$('button=Обзор…').click();
    await b.waitUntil(async () => (await b.$('input[readonly]').getValue()) !== '', { timeout: 10_000 });
    await b.$('button=Создать').click();

    await switchToWindowWith(b, switcherLabel('Архив'), 2);
    expect(existsSync(path.join(archive, '.rhizomind'))).toBe(true);
    await shot(b, 'second-vault');
  });

  it('С3.1: переключатель — уже открытое хранилище получает фокус, второго окна нет', async () => {
    const b = browser();
    await b.$(switcherLabel('Архив')).click();
    await b.$('[role="menu"]').waitForDisplayed({ timeout: 5_000 });
    await shotThemes(b, 'switcher-menu');
    await clickMenuItem(b, 'Рабочая база');
    await b.pause(1_500);
    expect(await b.getWindowHandles()).toHaveLength(2);
  });

  it('С3.3: заметка во втором хранилище — ввод сохранён до выхода из программы', async () => {
    const b = browser();
    await switchToWindowWith(b, switcherLabel('Архив'), 2);
    await createInRoot(b, 'Новая заметка');
    await commitName(b, 'Итоги года');
    await waitForFile(path.join(archive, 'Итоги года.md'));
    await b.$(TEXT).click();
    await b.keys('Год закрыт');
    await waitForContent(path.join(archive, 'Итоги года.md'), 'Год закрыт');
    await b.waitUntil(async () => (await statusText(b)) === 'Сохранено', { timeout: 5_000 });
    await shot(b, 'archive-note');
  });

  it('Н2, решение 82: заголовок окна — «<заметка> — <хранилище> — Rhizomind», следует за переходом', async () => {
    const b = browser();
    // Архив: открыта «Итоги года»; Рабочая база: открытую заметку удалили — без заметки.
    await waitForWindowTitles(b, [`Итоги года — Архив${TITLE_SUFFIX}`, `Рабочая база${TITLE_SUFFIX}`], 'два хранилища');
    // Переход к другой заметке в окне «Рабочая база» — заголовок меняется.
    await switchToWindowWith(b, switcherLabel('Рабочая база'), 2);
    await openBySwitcher(b, 'Новая идея', 'Новая идея');
    await waitForWindowTitles(
      b,
      [`Итоги года — Архив${TITLE_SUFFIX}`, `Новая идея — Рабочая база${TITLE_SUFFIX}`],
      'открыта заметка',
    );
    await openBySwitcher(b, 'Встреча команды', 'Проекты / Встреча команды');
    await waitForWindowTitles(
      b,
      [`Встреча команды — Рабочая база${TITLE_SUFFIX}`, `Итоги года — Архив${TITLE_SUFFIX}`],
      'переход к другой заметке',
    );
  });

  // ---- С3.1: перезапуск ---------------------------------------------------------------

  it('С3.1: перезапуск — открыто последнее хранилище и последняя открытая в нём заметка', async () => {
    await session?.stop();
    session = undefined;
    // Конец сессии WebDriver закрывает окна программы; программа выходит
    // после записи ввода (решение 53, предел 10 с).
    const waited = await waitForAppExit(20_000);
    expect(waited, 'программа не завершилась после закрытия окон').toBeGreaterThanOrEqual(0);
    session = await startApp({ profile });
    const b = browser();
    await b.$(switcherLabel('Архив')).waitForDisplayed({ timeout: 30_000 });
    await b.waitUntil(async () => (await headerPath(b)) === 'Итоги года', {
      timeout: 15_000,
      timeoutMsg: `открыта не последняя заметка: «${await headerPath(b)}»`,
    });
    await b.$(TEXT).waitForDisplayed({ timeout: 10_000 });
    expect(await editorText(b)).toContain('Год закрыт');
    // В переключателе — оба хранилища списка.
    await b.$(switcherLabel('Архив')).click();
    await b.$('[role="menu"]').waitForDisplayed({ timeout: 5_000 });
    const menu = await b.execute(() => document.querySelector('[role="menu"]')?.textContent ?? '');
    expect(menu).toContain('Рабочая база');
    expect(menu).toContain('Архив');
    await shotThemes(b, 'restarted');
    // Заголовок окна после перезапуска — с последней открытой заметкой (решение 82).
    await waitForWindowTitles(b, [`Итоги года — Архив${TITLE_SUFFIX}`], 'перезапуск');
    await b.keys('Escape');
    await b.$('[role="menu"]').waitForDisplayed({ reverse: true, timeout: 5_000 });
  }, 150_000);

  // Решение 58: повторный запуск при работающей программе — окно «Управление
  // хранилищами». Сигнал второго экземпляра есть только на Windows (на Linux
  // второй запуск завершается сам; single-instance.test.ts).
  it.skipIf(!isWindows)('решение 58: повторный запуск — окно «Управление хранилищами» в работающей программе', async () => {
    const b = browser();
    const env: NodeJS.ProcessEnv = Object.fromEntries(
      Object.entries(process.env).filter(([name]) => !name.startsWith('WEBVIEW2_') && !name.startsWith('RHIZOMIND_E2E_')),
    );
    const second = spawn(applicationPath(), [], { stdio: 'ignore', env });
    const code = await new Promise<number | null>((resolve) => second.once('exit', resolve));
    expect(code).toBe(0);
    await switchToWindowWith(b, MANAGER, 2);
    await b.waitUntil(async () => (await b.$$('li').length) === 2, { timeout: 20_000 });
    await shot(b, 'second-instance');
  });

  it('С3.1: «Убрать из списка» — хранилище уходит из списка, файлы на диске остаются', async () => {
    const b = browser();
    if (!(await b.$(MANAGER).isExisting())) {
      await b.$(switcherLabel('Архив')).click();
      await b.$('[role="menu"]').waitForDisplayed({ timeout: 5_000 });
      await clickMenuItem(b, 'Управление хранилищами…');
      await switchToWindowWith(b, MANAGER, 2);
    }
    await b.waitUntil(async () => (await b.$$('li').length) === 2, { timeout: 20_000 });
    const index = await b.execute(() =>
      [...document.querySelectorAll('li')].findIndex((item) => (item.textContent ?? '').includes('Рабочая база')),
    );
    await (await b.$$('li'))[index]?.moveTo();
    const more = b.$('button[aria-label="Действия с хранилищем «Рабочая база»"]');
    await more.waitForClickable({ timeout: 5_000 });
    await more.click();
    await clickMenuItem(b, 'Убрать из списка');
    await b.waitUntil(async () => (await b.$$('li').length) === 1, { timeout: 10_000 });
    expect(await b.$(MANAGER).getText()).not.toContain('Рабочая база');
    expect(existsSync(path.join(work, 'Проекты', 'Встреча команды.md'))).toBe(true);
    await shot(b, 'removed-from-list');
  });
});
