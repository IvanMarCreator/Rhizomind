// Сценарий задачи 2.8б (С3.1, решение 72): окно «Настройки хранилища» на
// настоящем ядре. Настройка действует сразу и пишется в
// `.rhizomind/settings.json` хранилища; горячая клавиша переназначается и
// работает. Снимки разделов — в светлой и тёмной теме, сравниваются с
// кадрами Э12.1–Э12.6 (Э12.2б, Э12.5б–г).
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const SECTIONS = [
  ['general', 'Общие'],
  ['editor', 'Редактор'],
  ['files', 'Файлы и ссылки'],
  ['appearance', 'Оформление'],
  ['hotkeys', 'Горячие клавиши'],
  ['about', 'О программе'],
] as const;

describe('окно «Настройки хранилища»', () => {
  let session: Session;
  let vault: string;

  const settingsFile = () => path.join(vault, '.rhizomind', 'settings.json');
  const savedSettings = (): Record<string, unknown> =>
    existsSync(settingsFile()) ? (JSON.parse(readFileSync(settingsFile(), 'utf8')) as Record<string, unknown>) : {};

  async function waitSaved(check: (settings: Record<string, unknown>) => boolean) {
    await session.browser.waitUntil(() => Promise.resolve(check(savedSettings())), {
      timeout: 10_000,
      timeoutMsg: `settings.json: ${JSON.stringify(savedSettings())}`,
    });
  }

  async function openSettings(section: string) {
    const { browser } = session;
    const dialog = browser.$('[role="dialog"][aria-label^="Настройки хранилища"]');
    if (!(await dialog.isExisting())) {
      await browser.$('[role="toolbar"] button[aria-label="Настройки"]').click();
      await dialog.waitForDisplayed();
    }
    await dialog.$(`nav button=${section}`).click();
    return dialog;
  }

  async function closeSettings() {
    const { browser } = session;
    await browser.$('[role="dialog"] button[aria-label="Закрыть"]').click();
    await browser.$('[role="dialog"][aria-label^="Настройки хранилища"]').waitForDisplayed({ reverse: true });
  }

  beforeAll(async () => {
    vault = createTestVault('Рабочая база', { 'Встреча.md': '# Встреча\n\nТекст заметки.\n' });
    session = await startApp({ vault });
    await session.browser.$('[data-testid="shell"]').waitForDisplayed({ timeout: 20_000 });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it.each(['light', 'dark'] as const)('разделы по макету Э12, тема %s: снимки', async (theme) => {
    const { browser } = session;
    for (const [id, label] of SECTIONS) {
      await openSettings(label);
      await browser.execute((name) => {
        document.documentElement.dataset.theme = name;
      }, theme);
      expect(await browser.$('#settings-section-title').getText()).toBe(label);
      await saveScreenshot(browser, `settings-${id}-${theme}`);
    }
    await closeSettings();
  });

  it('тема «Тёмная» — сразу и в settings.json хранилища', async () => {
    const { browser } = session;
    const dialog = await openSettings('Оформление');
    await dialog.$('[role="radio"]*=Тёмная').click();
    expect(await browser.execute(() => document.documentElement.dataset.theme)).toBe('dark');
    await waitSaved((settings) => settings.theme === 'dark' && settings.format_version === 1);
    await closeSettings();
  });

  it('ширина текста и размер шрифта применяются к открытой заметке сразу', async () => {
    const { browser } = session;
    await browser.$('[role="treeitem"]*=Встреча').click();
    await browser.$('.cm-editor').waitForDisplayed();

    const dialog = await openSettings('Редактор');
    await dialog.$('[role="radio"]=Во всю ширину').click();
    const field = dialog.$('input[aria-label="Размер шрифта текста"]');
    await field.setValue('20');
    await browser.keys('Enter');
    await saveScreenshot(browser, 'settings-editor-changed');
    await closeSettings();

    const look = await browser.execute(() => ({
      font: getComputedStyle(document.querySelector('.cm-editor') as Element).fontSize,
      column: getComputedStyle(document.querySelector('article.column') as Element).maxWidth,
    }));
    expect(look).toEqual({ font: '20px', column: 'none' });
    await waitSaved((settings) => settings.text_width === 'full' && settings.font_size === 20);
    await saveScreenshot(browser, 'settings-editor-applied');
  });

  it('горячие клавиши: Ctrl+O → Ctrl+P, конфликт с Ctrl+N; назначение работает и хранится', async () => {
    const { browser } = session;
    const dialog = await openSettings('Горячие клавиши');

    await dialog.$('button[aria-label="Изменить сочетание Ctrl+O"]').click();
    await saveScreenshot(browser, 'settings-hotkeys-recording');
    await browser.keys(['Control', 'p']);
    await dialog.$('button[aria-label="Изменить сочетание Ctrl+P"]').waitForDisplayed();
    await waitSaved((settings) => JSON.stringify(settings.hotkeys) === '{"quick_switcher.open":["Ctrl+KeyP"]}');

    await dialog.$('button[aria-label="Изменить сочетание F2"]').click();
    await browser.keys(['Control', 'n']);
    await dialog.$('[role="alertdialog"]').waitForDisplayed();
    await saveScreenshot(browser, 'settings-hotkeys-conflict');
    await browser.keys('Escape');
    await dialog.$('[role="alertdialog"]').waitForDisplayed({ reverse: true });

    await closeSettings();
    await browser.keys(['Control', 'p']);
    const switcher = browser.$('[role="dialog"][aria-label="Быстрый переход"]');
    await switcher.waitForDisplayed();
    await browser.keys('Escape');
    await switcher.waitForDisplayed({ reverse: true });
  });
});
