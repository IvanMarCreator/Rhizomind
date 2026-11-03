// Общие шаги сценариев контрольной точки КТ1 выпуска 0.2 (задача 0.2-8.3):
// правая панель, обратные ссылки, теги, блок свойств, текст на диске.
// Снимки — в светлой и тёмной теме, имя `<префикс>-<номер>-<шаг>-<тема>`.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { expect } from 'vitest';
import { saveScreenshot } from './screenshot.ts';

export const TEXT = '[role="textbox"][aria-label="Текст заметки"]';
const ROOT = path.join(import.meta.dirname, '..', '..');

/** Снимки шага в обеих темах; номер шага растёт сам в пределах префикса. */
export function themeShots(prefix: string): (browser: WebdriverIO.Browser, name: string) => Promise<string[]> {
  let step = 0;
  return async (browser, name) => {
    step += 1;
    await browser.execute(async () => {
      await document.fonts.ready;
    });
    const files: string[] = [];
    for (const theme of ['light', 'dark'] as const) {
      await browser.execute((value) => {
        document.documentElement.dataset.theme = value;
      }, theme);
      files.push(await saveScreenshot(browser, `${prefix}-${String(step).padStart(2, '0')}-${name}-${theme}`));
    }
    await browser.execute(() => {
      delete document.documentElement.dataset.theme;
    });
    return files;
  };
}

/** Путь над заметкой: «Папка / Имя». */
export function headerPath(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() =>
    (document.querySelector('nav[aria-label="Путь к заметке"]')?.textContent ?? '').replace(/\s+/g, ' ').trim(),
  );
}

/** Открыть заметку щелчком в дереве (фокус остаётся в дереве). */
export async function openFromTree(browser: WebdriverIO.Browser, name: string, header = name): Promise<void> {
  const row = browser.$(`[role="treeitem"][aria-label="${name}"]`);
  await row.waitForDisplayed({ timeout: 30_000 });
  await row.click();
  await browser.waitUntil(async () => (await headerPath(browser)) === header, {
    timeout: 10_000,
    timeoutMsg: `не открылась заметка «${header}»`,
  });
  await browser.$(TEXT).waitForExist({ timeout: 10_000 });
}

/** Текст строк редактора (скрытая разметка в DOM не попадает). */
export function editorLines(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute((selector) => [...document.querySelectorAll(`${selector} .cm-line`)].map((line) => line.textContent ?? ''), TEXT);
}

/** Поставить курсор в конец строки, где есть `fragment` (щелчок по строке и End). */
export async function cursorTo(browser: WebdriverIO.Browser, fragment: string): Promise<void> {
  const index = (await editorLines(browser)).findIndex((line) => line.includes(fragment));
  expect(index, `строка «${fragment}»`).toBeGreaterThanOrEqual(0);
  await (await browser.$$(`${TEXT} .cm-line`))[index]?.click();
  await browser.keys('End');
}

/**
 * Где курсор: текст строки и то, что в строке после курсора. По выделению
 * DOM: CodeMirror держит его совпадающим со своим, пока текст в фокусе.
 */
export function caret(browser: WebdriverIO.Browser): Promise<{ focused: boolean; line: string; after: string }> {
  return browser.execute((selector) => {
    const content = document.querySelector(selector);
    const selection = document.getSelection();
    const node = selection?.focusNode ?? null;
    const element = node === null ? null : node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
    const line = element?.closest('.cm-line') ?? null;
    if (!selection || !node || !line) return { focused: document.activeElement === content, line: '', after: '' };
    const range = document.createRange();
    range.setStart(node, selection.focusOffset);
    range.setEnd(line, line.childNodes.length);
    return { focused: document.activeElement === content, line: line.textContent ?? '', after: range.toString() };
  }, TEXT);
}

// ---- Правая панель ---------------------------------------------------------

export function panelOpen(browser: WebdriverIO.Browser): Promise<boolean> {
  return browser.execute(() => document.querySelector('aside.right-panel') !== null);
}

/** Открыть правую панель (кнопка справа в строке над заметкой) и выбрать вид. */
export async function showView(browser: WebdriverIO.Browser, view: 'Обратные ссылки' | 'Теги'): Promise<void> {
  if (!(await panelOpen(browser))) await browser.$('button.panel-toggle').click();
  await browser.$('aside.right-panel').waitForExist({ timeout: 5_000 });
  await browser.$(`[role="tab"][aria-label="${view}"]`).click();
  await browser.waitUntil(
    () => browser.execute((v) => document.querySelector('[role="tab"][aria-selected="true"]')?.getAttribute('aria-label') === v, view),
    { timeout: 5_000, timeoutMsg: `вид «${view}» не выбран` },
  );
}

/** Заголовок панели: название вида и число, например «Теги 3». */
export function panelHeading(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => (document.querySelector('aside.right-panel .heading')?.textContent ?? '').replace(/\s+/g, ' ').trim());
}

/** Текст тела панели (состояния «Индекс строится… N %», «Нет обратных ссылок»). */
export function panelBody(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => (document.querySelector('aside.right-panel .body')?.textContent ?? '').replace(/\s+/g, ' ').trim());
}

export interface BacklinkGroup {
  name: string;
  folder: string;
  total: string;
  lines: string[];
  links: string[];
}

/** Группы вида «Обратные ссылки» (текст — через textContent: getText в WebKitWebDriver пуст внутри кнопок). */
export function backlinkGroups(browser: WebdriverIO.Browser): Promise<BacklinkGroup[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('.backlinks .groups > .group')].map((group) => ({
      name: group.querySelector('.note .name')?.textContent ?? '',
      folder: group.querySelector('.note .folder')?.textContent ?? '',
      total: group.querySelector('.links-total')?.textContent ?? '',
      lines: [...group.querySelectorAll('.lines .line')].map((line) => line.textContent ?? ''),
      links: [...group.querySelectorAll('.lines .line mark.link')].map((mark) => mark.textContent ?? ''),
    })),
  );
}

export interface TagRow {
  label: string;
  expanded: boolean;
  notes: string[];
}

/** Строки вида «Теги»: подпись «#тег, заметок: N», раскрыт ли, заметки под ним. */
export function tagRows(browser: WebdriverIO.Browser): Promise<TagRow[]> {
  return browser.execute(() =>
    [...document.querySelectorAll('.tags .list > li')].map((item) => {
      const row = item.querySelector('button.tag-row');
      return {
        label: row?.getAttribute('aria-label') ?? '',
        expanded: row?.getAttribute('aria-expanded') === 'true',
        notes: [...item.querySelectorAll('.notes .note-row')].map((note) => (note.textContent ?? '').replace(/\s+/g, ' ').trim()),
      };
    }),
  );
}

/** Щелчок по строке тега в виде «Теги». */
export async function clickTag(browser: WebdriverIO.Browser, label: string): Promise<void> {
  await browser.$(`.tags button.tag-row[aria-label="${label}"]`).click();
}

// ---- Диск -------------------------------------------------------------------

/**
 * Текст файла; файла нет — пустая строка. Ядро пишет заметку атомарно: на
 * Windows между заменой файл на миг отсутствует (ENOENT) или занят (EBUSY).
 */
export function readIfExists(file: string): string {
  try {
    return readFileSync(file, 'utf8');
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === 'ENOENT' || code === 'EBUSY') return '';
    throw error;
  }
}

/** Ждёт, пока `check(текст файла)` станет истинным; иначе ошибка с текстом файла. */
export async function waitForFileText(file: string, check: (text: string) => boolean, what: string, timeout = 5_000): Promise<string> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const text = readIfExists(file);
    if (check(text)) return text;
    if (Date.now() > deadline) throw new Error(`${what}: в ${file} ${JSON.stringify(text.slice(0, 300))}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

// ---- Генератор хранилища ------------------------------------------------------

/**
 * gen-vault (tools/gen-vault): RHIZOMIND_E2E_GEN_VAULT или выпускная сборка в
 * папке Cargo — в CI её собирает шаг «Генератор тестового хранилища» того же
 * задания, что и сценарные тесты.
 */
export function genVaultPath(): string {
  if (process.env.RHIZOMIND_E2E_GEN_VAULT) return process.env.RHIZOMIND_E2E_GEN_VAULT;
  const target = process.env.CARGO_TARGET_DIR ? path.resolve(ROOT, process.env.CARGO_TARGET_DIR) : path.join(ROOT, 'src-tauri', 'target');
  return path.join(target, 'release', process.platform === 'win32' ? 'gen-vault.exe' : 'gen-vault');
}

/** Хранилище генератора в `root` (папки ещё нет): `notes` заметок, зерно `seed`. */
export function generateVault(root: string, notes: number, seed = 1): number {
  const exe = genVaultPath();
  if (!existsSync(exe)) throw new Error(`нет генератора хранилища ${exe} (cargo build -p gen-vault --release)`);
  const started = Date.now();
  execFileSync(exe, ['--notes', String(notes), '--seed', String(seed), '--out', root], { stdio: 'inherit' });
  return Date.now() - started;
}
