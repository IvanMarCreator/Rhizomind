// Внешние изменения файлов при открытом приложении (задача 2.9, С3.6,
// инвариант 4, критерий приёмки С9.7: «ни один сценарий из 3.6 не приводит к
// потере текста»). Файлы меняются «снаружи» — прямо на диске из теста, как
// это делают Блокнот, Проводник или Git, — а приложение работает на
// настоящем ядре с наблюдателем за папкой:
// - правка без ввода — текст обновлён, курсор на месте, файл не переписан;
// - правка при несохранённом вводе и во время автосохранения — обе версии
//   на диске: ваша в заметке, с диска — копией «Имя (внешняя версия …).md»;
// - переименование и перенос — заметка открыта под новым именем, ввод
//   записан в новый файл;
// - удаление — заметка закрыта с уведомлением, файл не появляется снова;
// - правка снаружи и сразу закрытие окна — обе версии на диске после выхода.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { appPids, createTestVault, removeTestVault, startApp, waitForAppExit, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const TEXT = '[role="textbox"][aria-label="Текст заметки"]';
const isWindows = process.platform === 'win32';
const isLinux = process.platform === 'linux';

/** Медленный диск: запись заметки ждёт столько, мс (тестовая сборка, задача 0.7) — автосохранение «в полёте». */
const SLOW_WRITE_MS = 2_500;

async function openNote(browser: WebdriverIO.Browser, name: string): Promise<void> {
  const row = browser.$(`[role="treeitem"][aria-label="${name}"]`);
  await row.waitForExist({ timeout: 30_000 });
  await row.click();
  await browser.waitUntil(async () => (await headerPath(browser)) === name, {
    timeout: 10_000,
    timeoutMsg: `не открылась заметка «${name}»`,
  });
  await browser.$(TEXT).waitForExist({ timeout: 10_000 });
}

/** Путь над заметкой: «Папка / Имя». */
function headerPath(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() =>
    (document.querySelector('nav[aria-label="Путь к заметке"]')?.textContent ?? '').replace(/\s+/g, ' ').trim(),
  );
}

function editorText(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(
    (selector) => [...document.querySelectorAll(`${selector} .cm-line`)].map((line) => line.textContent ?? '').join('\n'),
    TEXT,
  );
}

async function waitForEditorText(browser: WebdriverIO.Browser, wanted: string, timeout = 10_000): Promise<void> {
  await browser.waitUntil(async () => (await editorText(browser)) === wanted, {
    timeout,
    timeoutMsg: `в редакторе не «${wanted}»`,
  });
}

/** Ввод в конец заметки, без пауз. Без двойных букв подряд: WebKitWebDriver теряет повтор («вв» → «в»). */
async function typeAtEnd(browser: WebdriverIO.Browser, text: string): Promise<void> {
  await browser.$(TEXT).click();
  await browser.keys(['Control', 'End']);
  await browser.keys(text);
}

async function waitForText(browser: WebdriverIO.Browser, text: string, timeout = 10_000): Promise<void> {
  await browser.waitUntil(() => browser.execute((wanted) => document.body.innerText.includes(wanted), text), {
    timeout,
    timeoutMsg: `нет текста «${text}»`,
  });
}

async function waitFor(check: () => boolean, what: string, timeout = 15_000): Promise<void> {
  const deadline = Date.now() + timeout;
  while (!check()) {
    if (Date.now() > deadline) throw new Error(`не дождались: ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

const read = (file: string) => (existsSync(file) ? readFileSync(file, 'utf8') : null);

/** Копии с версией с диска рядом с заметкой: «Имя (внешняя версия ГГГГ-ММ-ДД ЧЧ-ММ).md». */
function conflictCopies(dir: string, name: string): string[] {
  const pattern = new RegExp(`^${name} \\(внешняя версия \\d{4}-\\d{2}-\\d{2} \\d{2}-\\d{2}\\)( \\d+)?\\.md$`);
  return readdirSync(dir).filter((file) => pattern.test(file));
}

/** Закрыть окно процесса, как кнопкой «×» (см. close.test.ts). */
function closeLikeUser(pid: number): void {
  if (isWindows) {
    const out = execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', `(Get-Process -Id ${pid}).CloseMainWindow()`], {
      encoding: 'utf8',
    }).trim();
    expect(out).toBe('True');
  } else {
    execFileSync('python3', ['-I', path.join(import.meta.dirname, 'x11-close.py'), String(pid)]);
  }
}

describe.skipIf(!isWindows && !isLinux)('С3.6: внешние изменения — медленный диск (автосохранение в полёте)', () => {
  let session: Session;
  let vault = '';

  beforeAll(async () => {
    vault = createTestVault('Внешние изменения', {
      'Конфликт.md': 'начало\n',
      'Переименование.md': 'начало\n',
      'Удаление.md': 'начало\n',
    });
    mkdirSync(path.join(vault, 'Архив'));
    session = await startApp({ vault, slowWriteMs: SLOW_WRITE_MS });
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (vault) removeTestVault(vault);
  });

  it('ввод → автосохранение в полёте → правка снаружи: обе версии на диске, уведомление', async () => {
    const b = session.browser;
    await openNote(b, 'Конфликт');
    await typeAtEnd(b, 'мой набор');
    // Автосохранение уходит через 1 с и ждёт «медленный диск»; файл меняют
    // снаружи раньше, чем запись дошла до диска.
    writeFileSync(path.join(vault, 'Конфликт.md'), 'начало\nправка снаружи\n');

    await waitFor(() => conflictCopies(vault, 'Конфликт').length === 1, 'копия с версией с диска');
    await waitFor(() => read(path.join(vault, 'Конфликт.md')) === 'начало\nмой набор', 'ввод в заметке');
    const [copy] = conflictCopies(vault, 'Конфликт');
    expect(read(path.join(vault, copy ?? ''))).toBe('начало\nправка снаружи\n');
    await waitForText(b, 'Сохранены обе версии');
    expect(await editorText(b)).toBe('начало\nмой набор');
    await saveScreenshot(b, 'external-01-conflict-both-versions');
  });

  it('переименование и перенос снаружи при несохранённом вводе: заметка открыта под новым именем, ввод — в новом файле', async () => {
    const b = session.browser;
    await openNote(b, 'Переименование');
    await typeAtEnd(b, 'набор');
    renameSync(path.join(vault, 'Переименование.md'), path.join(vault, 'Архив', 'Новое имя.md'));

    await b.waitUntil(async () => (await headerPath(b)) === 'Архив / Новое имя', {
      timeout: 15_000,
      timeoutMsg: 'путь над заметкой не сменился',
    });
    expect(await editorText(b)).toBe('начало\nнабор');
    await waitFor(() => read(path.join(vault, 'Архив', 'Новое имя.md')) === 'начало\nнабор', 'ввод в новом файле');
    expect(existsSync(path.join(vault, 'Переименование.md'))).toBe(false);
    expect(conflictCopies(path.join(vault, 'Архив'), 'Новое имя')).toEqual([]);
    await saveScreenshot(b, 'external-02-renamed');
  });

  it('удаление снаружи: заметка закрыта с уведомлением, файл не появляется снова', async () => {
    const b = session.browser;
    await openNote(b, 'Удаление');
    await typeAtEnd(b, 'набор');
    rmSync(path.join(vault, 'Удаление.md'));

    await waitForText(b, 'Заметка «Удаление» удалена на диске.');
    await waitForText(b, 'Нет открытой заметки');
    await saveScreenshot(b, 'external-03-deleted');
    // Автосохранение и медленная запись прошли — файл не воскрес.
    await new Promise((resolve) => setTimeout(resolve, 1_000 + SLOW_WRITE_MS + 1_000));
    expect(existsSync(path.join(vault, 'Удаление.md'))).toBe(false);
    expect(readdirSync(vault).filter((file) => file.startsWith('Удаление'))).toEqual([]);
  });
});

describe.skipIf(!isWindows && !isLinux)('С3.6: правка без ввода и закрытие окна', () => {
  let session: Session | undefined;
  let vault = '';

  afterEach(async () => {
    await session?.stop().catch(() => undefined);
    session = undefined;
    if (vault) removeTestVault(vault);
    vault = '';
  });

  it('правка снаружи без ввода: текст обновлён, курсор на своей строке, файл не переписан', async () => {
    vault = createTestVault('Правка', { 'Правка.md': 'один\nдва\nтри\n' });
    session = await startApp({ vault });
    const b = session.browser;
    await openNote(b, 'Правка');
    // Курсор — в начало строки «три».
    await b.$(TEXT).click();
    await b.keys(['Control', 'Home']);
    // По одной клавише за вызов: WebKitWebDriver теряет повтор одинаковых клавиш подряд.
    await b.keys('ArrowDown');
    await b.keys('ArrowDown');

    writeFileSync(path.join(vault, 'Правка.md'), 'ноль\nодин\nдва\nтри\n');
    await waitForEditorText(b, 'ноль\nодин\nдва\nтри\n');
    await saveScreenshot(b, 'external-04-reloaded');
    // Ни записи назад, ни копии: файл ровно тот, что записали снаружи.
    await new Promise((resolve) => setTimeout(resolve, 2_000));
    expect(read(path.join(vault, 'Правка.md'))).toBe('ноль\nодин\nдва\nтри\n');
    expect(conflictCopies(vault, 'Правка')).toEqual([]);

    // Курсор остался в начале строки «три»: ввод встаёт туда.
    await b.keys('X');
    await waitFor(() => read(path.join(vault, 'Правка.md')) === 'ноль\nодин\nдва\nXтри\n', 'ввод с прежнего места курсора');
  });

  it('ввод, правка снаружи и сразу закрытие окна: после выхода обе версии на диске', async () => {
    vault = createTestVault('Закрытие', { 'Заметка.md': 'начало\n' });
    // Медленный диск: автосохранение, ушедшее до правки снаружи, ещё не
    // дошло до диска — правка снаружи гарантированно раньше записи.
    session = await startApp({ vault, slowWriteMs: SLOW_WRITE_MS });
    const b = session.browser;
    await openNote(b, 'Заметка');
    await typeAtEnd(b, 'набор перед закрытием');
    const [pid] = appPids();
    expect(pid).toBeDefined();
    writeFileSync(path.join(vault, 'Заметка.md'), 'начало\nснаружи\n');

    closeLikeUser(pid as number);
    expect(await waitForAppExit(30_000)).toBeGreaterThanOrEqual(0);

    expect(read(path.join(vault, 'Заметка.md'))).toBe('начало\nнабор перед закрытием');
    const copies = conflictCopies(vault, 'Заметка');
    expect(copies).toHaveLength(1);
    expect(read(path.join(vault, copies[0] ?? ''))).toBe('начало\nснаружи\n');
  });
});
