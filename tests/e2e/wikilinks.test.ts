// Вики-ссылки на настоящем ядре (задача 2.6, С3.4): вид разрешённой и
// неразрешённой ссылки, щелчок по неразрешённой создаёт заметку в папке
// текущей и открывает её, переход по ссылке на заголовок, автодополнение
// после `[[`. Снимки — в светлой и тёмной теме.
import { existsSync, readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const TEXT = '[role="textbox"][aria-label="Текст заметки"]';

const HOME = [
  '# Дневник',
  '',
  'Сегодня: [[Альфа/Встреча#Итоги|итоги встречи]], [[Новая идея]] и [[План]].',
  '',
  'Последняя строка.',
  '',
].join('\n');

const MEETING = ['# Встреча', '', 'Обсудили сроки.', '', '## Итоги', '', 'Решили начать в понедельник.', ''].join('\n');

let step = 0;

async function shotThemes(browser: WebdriverIO.Browser, name: string): Promise<void> {
  step += 1;
  for (const theme of ['light', 'dark'] as const) {
    await browser.execute((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    await saveScreenshot(browser, `wikilinks-${String(step).padStart(2, '0')}-${name}-${theme}`);
  }
  await browser.execute(() => {
    delete document.documentElement.dataset.theme;
  });
}

function lineText(browser: WebdriverIO.Browser, fragment: string): Promise<string> {
  return browser.execute(
    (selector, wanted) =>
      [...document.querySelectorAll(`${selector} .cm-line`)]
        .map((line) => line.textContent ?? '')
        .find((text) => text.includes(wanted)) ?? `нет строки «${wanted}»`,
    TEXT,
    fragment,
  );
}

async function cursorTo(browser: WebdriverIO.Browser, fragment: string): Promise<void> {
  const index = await browser.execute(
    (selector, wanted) =>
      [...document.querySelectorAll(`${selector} .cm-line`)].findIndex((line) => (line.textContent ?? '').includes(wanted)),
    TEXT,
    fragment,
  );
  expect(index, `строка «${fragment}»`).toBeGreaterThanOrEqual(0);
  await (await browser.$$(`${TEXT} .cm-line`))[index]?.click();
  await browser.keys('End');
}

/** Имя открытой заметки (заголовок над текстом). */
function currentTitle(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => document.querySelector('.note-title h1')?.textContent?.trim() ?? '');
}

describe('вики-ссылки', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    vault = createTestVault('Ссылки', {
      'Дневник/Сегодня.md': HOME,
      'Проекты/Альфа/Встреча.md': MEETING,
      'План.md': '# План\n',
    });
    profile = createTestProfile();
    session = await startApp({ vault, profile });
    const { browser } = session;
    const folder = browser.$('[role="treeitem"][aria-label="Дневник"]');
    await folder.waitForDisplayed({ timeout: 20_000 });
    await folder.click();
    const row = browser.$('[role="treeitem"][aria-label="Сегодня"]');
    await row.waitForDisplayed({ timeout: 10_000 });
    await row.click();
    await browser.$(TEXT).waitForDisplayed({ timeout: 10_000 });
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (vault) removeTestVault(vault);
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  const browser = () => {
    if (!session) throw new Error('нет сессии');
    return session.browser;
  };

  it('ссылки показаны по С3.4: псевдоним, имя без пути; неразрешённая — приглушённая', async () => {
    const b = browser();
    await cursorTo(b, 'Последняя строка.');
    expect(await lineText(b, 'Сегодня:')).toBe('Сегодня: итоги встречи, Новая идея и План.');
    await b.waitUntil(
      () => b.execute((s) => document.querySelectorAll(`${s} .cm-rz-wikilink-unresolved`).length, TEXT).then((n) => n === 1),
      { timeout: 5_000, timeoutMsg: 'неразрешённая ссылка не отмечена' },
    );
    const unresolved = await b.execute((s) => document.querySelector(`${s} .cm-rz-wikilink-unresolved`)?.textContent ?? '', TEXT);
    expect(unresolved).toBe('Новая идея');
    await shotThemes(b, 'links');
    await cursorTo(b, 'Сегодня:');
    expect(await lineText(b, 'Сегодня:')).toBe('Сегодня: [[Альфа/Встреча#Итоги|итоги встречи]], [[Новая идея]] и [[План]].');
    await shotThemes(b, 'links-cursor');
  });

  it('щелчок по неразрешённой ссылке создаёт заметку в папке текущей и открывает её', async () => {
    const b = browser();
    await cursorTo(b, 'Последняя строка.');
    await b.$(`${TEXT} [data-rz-wiki="Новая идея"]`).click();
    const created = path.join(vault, 'Дневник', 'Новая идея.md');
    await b.waitUntil(() => existsSync(created), { timeout: 5_000, timeoutMsg: 'заметка не создана' });
    expect(readFileSync(created, 'utf8')).toBe('');
    await b.$('[role="treeitem"][aria-label="Новая идея"]').waitForDisplayed({ timeout: 5_000 });
    await b.waitUntil(async () => (await currentTitle(b)) === 'Новая идея', {
      timeout: 5_000,
      timeoutMsg: 'новая заметка не открыта',
    });
    await shotThemes(b, 'created');
  });

  it('назад к заметке со ссылкой: созданная ссылка разрешена; щелчок по ссылке на заголовок — переход к нему', async () => {
    const b = browser();
    await b.keys(['Alt', 'ArrowLeft']);
    await b.waitUntil(async () => (await lineText(b, 'Сегодня:')).includes('Сегодня'), { timeout: 5_000 });
    await cursorTo(b, 'Последняя строка.');
    await b.waitUntil(
      () => b.execute((s) => document.querySelectorAll(`${s} .cm-rz-wikilink-unresolved`).length, TEXT).then((n) => n === 0),
      { timeout: 5_000, timeoutMsg: 'созданная заметка — ссылка всё ещё неразрешённая' },
    );
    await b.$(`${TEXT} [data-rz-wiki="Альфа/Встреча#Итоги|итоги встречи"]`).click();
    await b.waitUntil(async () => (await lineText(b, 'Обсудили')).includes('Обсудили'), { timeout: 5_000, timeoutMsg: 'заметка не открыта' });
    // Курсор на строке заголовка: её разметка видна.
    await b.waitUntil(async () => (await lineText(b, 'Итоги')) === '## Итоги', { timeout: 5_000, timeoutMsg: 'нет перехода к заголовку' });
    await shotThemes(b, 'heading');
  });

  it('автодополнение после [[: Enter вставляет кратчайший путь и ]] — в файл', async () => {
    const b = browser();
    await cursorTo(b, 'Решили начать');
    await b.keys(['Enter']);
    await b.keys([...'См. [[пла']);
    await b.$('[role="listbox"] [role="option"]').waitForDisplayed({ timeout: 5_000 });
    await shotThemes(b, 'suggest');
    await b.keys(['Enter']);
    expect(await lineText(b, 'См.')).toBe('См. [[План]]');
    const file = path.join(vault, 'Проекты', 'Альфа', 'Встреча.md');
    await b.waitUntil(() => readFileSync(file, 'utf8').includes('См. [[План]]'), { timeout: 5_000, timeoutMsg: 'вставка не записана' });
  });
});
