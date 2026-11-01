// Сценарий задачи 2.17 (решения 94, 96): сочетания браузерного движка, не
// назначенные командам, ничего не делают в окне — ни печати (Ctrl+P), ни
// поиска движка (Ctrl+F), ни перезагрузки (F5), ни масштаба (Ctrl+=/−).
// Назначенное пользователем Ctrl+P работает как команда.
//
// Что видно отсюда: нажатия WebDriver доходят до страницы, запасной рубеж
// интерфейса гасит их (слушатель теста стоит после оболочки и видит
// `defaultPrevented`), печать не началась (`beforeprint`), страница не
// перезагрузилась (метка на месте), масштаб не изменился (`devicePixelRatio`).
// Настоящие нажатия клавиатуры (SendInput) при выключенных ускорителях
// WebView2 — в src-tauri/tests/webview2_runtime.rs.
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

type Browser = Session['browser'];

const SWITCHER = '[role="dialog"][aria-label="Быстрый переход"]';

/** Метка страницы, журнал нажатий (после оболочки) и событие печати. */
async function watchPage(browser: Browser): Promise<number> {
  return browser.execute(() => {
    const state = window as unknown as { __rzMark: boolean; __rzLog: string[] };
    state.__rzMark = true;
    state.__rzLog = [];
    window.addEventListener('beforeprint', () => state.__rzLog.push('beforeprint'));
    window.addEventListener('keydown', (event) => {
      state.__rzLog.push(`${event.ctrlKey ? 'Ctrl+' : ''}${event.code}:${event.defaultPrevented ? 'prevented' : 'open'}`);
    });
    return window.devicePixelRatio;
  });
}

async function pageState(browser: Browser): Promise<{ mark: boolean; log: string[]; dpr: number }> {
  return browser.execute(() => {
    const state = window as unknown as { __rzMark?: boolean; __rzLog?: string[] };
    return { mark: state.__rzMark === true, log: state.__rzLog ?? [], dpr: window.devicePixelRatio };
  });
}

function pause(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

describe('сочетания движка без назначения ничего не делают', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createTestVault('Сочетания', { 'Встреча.md': '# Встреча\n\nТекст.\n' });
    session = await startApp({ vault });
    await session.browser.$('[role="treeitem"][aria-label="Встреча"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('Ctrl+P, Ctrl+F, F5, Ctrl+=, Ctrl+− в тексте заметки: гасятся, печати, перезагрузки и масштаба нет', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Встреча"]').click();
    await browser.$('.cm-content').waitForDisplayed({ timeout: 10_000 });
    await browser.$('.cm-content').click();
    const dpr = await watchPage(browser);

    for (const chord of [
      ['Control', 'p'],
      ['Control', 'f'],
      ['F5'],
      ['Control', '='],
      ['Control', '-'],
    ]) {
      await browser.keys(chord);
      await pause(500);
    }
    await pause(1000);
    await saveScreenshot(browser, 'engine-shortcuts-none');

    const state = await pageState(browser);
    expect(state.mark, 'страница перезагрузилась').toBe(true);
    for (const entry of ['Ctrl+KeyP', 'Ctrl+KeyF', 'F5', 'Ctrl+Equal', 'Ctrl+Minus']) {
      expect(state.log, entry).toContain(`${entry}:prevented`);
    }
    expect(state.log).not.toContain('beforeprint');
    expect(state.dpr).toBe(dpr);
    expect(await browser.$('[role="dialog"]').isExisting()).toBe(false);
  });

  it('команды по умолчанию работают: Ctrl+O открывает быстрый переход, Esc закрывает', async () => {
    const { browser } = session;
    await browser.$('.cm-content').click();
    await browser.keys(['Control', 'o']);
    await browser.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
    await browser.keys('Escape');
    await browser.$(SWITCHER).waitForExist({ reverse: true, timeout: 5_000 });
  });

  it('правка текста не гасится: Ctrl+A и ввод заменяют текст заметки', async () => {
    const { browser } = session;
    await browser.$('.cm-content').click();
    await watchPage(browser);
    await browser.keys(['Control', 'a']);
    await browser.keys('Z');
    await browser.waitUntil(async () => (await browser.$('.cm-content').getText()).trim() === 'Z', {
      timeout: 5_000,
      timeoutMsg: 'Ctrl+A не выделил текст',
    });
  });
});

describe('Ctrl+P, назначенное быстрому переходу (решение 96)', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createTestVault('Назначено', { 'Начало.md': '# Начало\n' });
    mkdirSync(path.join(vault, '.rhizomind'));
    writeFileSync(
      path.join(vault, '.rhizomind', 'settings.json'),
      JSON.stringify({ format_version: 1, hotkeys: { 'quick_switcher.open': ['Ctrl+KeyP'] } }),
    );
    session = await startApp({ vault });
    await session.browser.$('[role="treeitem"][aria-label="Начало"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('Ctrl+P открывает быстрый переход, печати нет', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Начало"]').click();
    await browser.$('.cm-content').waitForDisplayed({ timeout: 10_000 });
    await browser.$('.cm-content').click();
    await watchPage(browser);

    await browser.keys(['Control', 'p']);
    await browser.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
    await saveScreenshot(browser, 'engine-shortcuts-assigned');

    expect((await pageState(browser)).log).not.toContain('beforeprint');
    await browser.keys('Escape');
  });
});
