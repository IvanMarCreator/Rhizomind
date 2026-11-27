import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const PALETTE = '[role="dialog"]:has(input[aria-label="Найти команду…"])';
const EDIT_NAME = 'input[aria-label="Новое имя"]';
const TEXT = 'Русский текст без разметки';

function vaultFor(theme: string, hotkeys: Record<string, string[]> = {}) {
  const vault = createTestVault('Палитра', { 'Заметка.md': TEXT });
  mkdirSync(path.join(vault, 'Архив'));
  mkdirSync(path.join(vault, '.rhizomind'));
  writeFileSync(path.join(vault, '.rhizomind', 'settings.json'),
    JSON.stringify({ format_version: 1, theme, editor_mode: 'source', hotkeys }));
  return vault;
}

describe.each(['light', 'dark'])('палитра в окне, тема %s', (theme) => {
  let session: Session;
  let vault: string;

  beforeAll(async () => {
    vault = vaultFor(theme);
    session = await startApp({ vault });
    await session.browser.$('[role="treeitem"][aria-label="Заметка"]').waitForDisplayed({ timeout: 20_000 });
  });
  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('F12 из редактора, русский запрос и Esc сохраняют выделение и текст', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Заметка"]').click();
    const editor = browser.$('.cm-content');
    await editor.waitForDisplayed();
    await editor.click();
    await browser.keys(['Control', 'a']);
    const selection = () => browser.execute(() => document.getSelection()?.toString());
    expect(await selection()).toBe(TEXT);
    await browser.keys('F12');
    const dialog = browser.$(PALETTE);
    await dialog.waitForDisplayed();
    expect(await browser.execute(() => document.documentElement.dataset.theme)).toBe(theme);
    const unavailable = await dialog.$$('[role="option"][aria-disabled="true"]').map((option) => option.getText());
    expect(unavailable.some((text) => text.includes('Переименовать'))).toBe(true);
    await saveScreenshot(browser, `palette-${theme}`);
    await browser.keys('режим');
    expect(await dialog.$$('[role="option"]').length).toBe(1);
    await browser.keys('Escape');
    await dialog.waitForExist({ reverse: true });
    expect(await browser.execute(() => document.activeElement?.classList.contains('cm-content'))).toBe(true);
    expect(await selection()).toBe(TEXT);
    expect(readFileSync(path.join(vault, 'Заметка.md'), 'utf8')).toBe(TEXT);
  });

  it('действие дерева, Ctrl+O, F2 и однократное создание по Enter и по Ctrl+N', async () => {
    const { browser } = session;
    const folder = browser.$('[role="treeitem"][aria-label="Архив"]');
    const field = browser.$(EDIT_NAME);
    const dialog = browser.$(PALETTE);
    await folder.click();
    await browser.keys('F12');
    await dialog.waitForDisplayed();
    await browser.keys('именова');
    await browser.keys('Enter');
    await field.waitForDisplayed();
    expect(await field.getValue()).toBe('Архив');
    await browser.keys('Escape');
    await folder.click();
    await browser.keys('F2');
    await field.waitForDisplayed();
    await browser.keys('Escape');

    await browser.keys(['Control', 'o']);
    const switcher = browser.$('[role="dialog"][aria-label="Быстрый переход"]');
    await switcher.waitForDisplayed();
    await browser.keys('F12');
    expect(await dialog.isExisting()).toBe(false);
    await browser.keys('Escape');

    await browser.keys('F12');
    await dialog.waitForDisplayed();
    await browser.keys('ArrowDown');
    expect(await dialog.$('[aria-selected="true"]').getText()).toContain('Новая заметка');
    await browser.keys('Enter');
    await field.waitForDisplayed();
    const notes = () => readdirSync(vault).filter((name) => name.endsWith('.md'));
    expect(notes()).toHaveLength(2);
    await browser.keys('Escape');
    await browser.keys(['Control', 'n']);
    await field.waitForDisplayed();
    expect(notes()).toHaveLength(3);
    await browser.keys('Escape');
  });
});

it('сохранённая F12 переименовывает строку дерева; палитра открывается переназначенной клавишей', async () => {
  const vault = vaultFor('light', { 'tree.rename': ['F12'], 'palette.open': ['F9'] });
  let session: Session | undefined;
  try {
    session = await startApp({ vault });
    const { browser } = session;
    const folder = browser.$('[role="treeitem"][aria-label="Архив"]');
    await folder.waitForDisplayed({ timeout: 20_000 });
    await folder.click();
    await browser.keys('F12');
    await browser.$(EDIT_NAME).waitForDisplayed();
    expect(await browser.$(PALETTE).isExisting()).toBe(false);
    await browser.keys('Escape');
    await browser.keys('F9');
    const dialog = browser.$(PALETTE);
    await dialog.waitForDisplayed();
    await browser.keys('именова');
    expect(await dialog.$('[role="option"]').getText()).toContain('F12');
    await browser.keys('Escape');
  } finally {
    await session?.stop();
    removeTestVault(vault);
  }
});
