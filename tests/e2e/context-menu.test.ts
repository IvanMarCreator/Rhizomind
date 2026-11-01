// Сценарий задачи 2.19 (решение 97): меню браузерного движка выключено,
// вместо него в тексте заметки — своё меню правки текста («Вырезать»,
// «Копировать», «Вставить», «Выделить всё»); в дереве — своё меню; в
// остальных местах правый щелчок ничего не делает.
//
// Буфер обмена — общий системный (решение 98): «Копировать» из меню кладёт
// выделение в буфер ОС — проверяется вставкой Ctrl+V; «Вставить» из меню
// берёт текст командой ядра `clipboard_read_text`. Настоящий правый щелчок
// мышью (SendInput) при выключенном меню движка — в
// src-tauri/tests/webview2_runtime.rs.
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const MENU = '[role="menu"][aria-label="Правка текста"]';

describe('своё контекстное меню вместо меню движка', () => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = createTestVault('Меню', { 'Заметка.md': 'альфа' });
    session = await startApp({ vault });
    await session.browser.$('[role="treeitem"][aria-label="Заметка"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('правый щелчок в тексте: своё меню; «Копировать», Ctrl+V и «Вставить» из меню', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Заметка"]').click();
    const content = browser.$('.cm-content');
    await content.waitForDisplayed({ timeout: 10_000 });
    await content.click();
    await browser.keys(['Control', 'a']);

    await browser.$('.cm-line').click({ button: 'right' });
    const menu = browser.$(MENU);
    await menu.waitForDisplayed({ timeout: 5_000 });
    await saveScreenshot(browser, 'context-menu-text');
    const items = await browser.execute((selector) =>
      [...document.querySelectorAll(`${selector} [role="menuitem"]`)].map(
        (item) => `${item.querySelector('.label')?.textContent ?? ''}:${item.getAttribute('aria-disabled') ?? 'false'}`,
      ),
    MENU);
    expect(items).toEqual(['Вырезать:false', 'Копировать:false', 'Вставить:false', 'Выделить всё:false']);

    await menu.$('[role="menuitem"]*=Копировать').click();
    await menu.waitForExist({ reverse: true, timeout: 5_000 });

    await browser.keys(['Control', 'End']);
    await browser.keys(' ');
    await browser.keys(['Control', 'v']);
    await browser.waitUntil(async () => (await content.getText()).trim() === 'альфа альфа', {
      timeout: 5_000,
      timeoutMsg: `после вставки: «${await content.getText()}»`,
    });

    // «Вставить» из меню: щелчок правее конца строки ставит туда курсор.
    await browser.keys(' ');
    await browser.$('.cm-line').click({ button: 'right' });
    await menu.waitForDisplayed({ timeout: 5_000 });
    await menu.$('[role="menuitem"]*=Вставить').click();
    await menu.waitForExist({ reverse: true, timeout: 5_000 });
    await browser.waitUntil(async () => (await content.getText()).trim() === 'альфа альфа альфа', {
      timeout: 5_000,
      timeoutMsg: `после «Вставить» из меню: «${await content.getText()}»`,
    });
  });

  it('правый щелчок по ленте и шапке заметки ничего не открывает', async () => {
    const { browser } = session;
    await browser.$('[role="toolbar"][aria-label="Лента иконок"]').click({ button: 'right' });
    await browser.$('header[aria-label="Заметка"]').click({ button: 'right' });
    await browser.pause(500);
    expect(await browser.$('[role="menu"]').isExisting()).toBe(false);
  });

  it('в дереве — меню дерева, как раньше', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Заметка"]').click({ button: 'right' });
    const menu = browser.$('[role="menu"][aria-label="Действия с файлами"]');
    await menu.waitForDisplayed({ timeout: 5_000 });
    await saveScreenshot(browser, 'context-menu-tree');
    await browser.keys('Escape');
    try {
      await menu.waitForExist({ reverse: true, timeout: 5_000 });
    } catch {
      const state = await browser.execute(() => ({
        menus: document.querySelectorAll('[role="menu"]').length,
        active: document.activeElement?.outerHTML.slice(0, 160) ?? null,
        focused: document.hasFocus(),
      }));
      throw new Error(`меню дерева не закрылось по Esc: ${JSON.stringify(state)}`);
    }
  });
});
