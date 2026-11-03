// Обратные ссылки на настоящем ядре (задача 0.2-1.4, С2-3.2): в хранилище
// есть обратная ссылка — после открытия правая панель «Обратные ссылки»
// показывает её, строка с HTML — текстом. Снимки — в светлой и тёмной теме.
import { rmSync } from 'node:fs';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const TEXT = '[role="textbox"][aria-label="Текст заметки"]';
const LINE = 'Сегодня обсудили [[Узел]] и <b>планы</b>';

describe('обратные ссылки', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    vault = createTestVault('Связи', {
      'Узел.md': '# Узел\n',
      'Дневник.md': `# Дневник\n\n${LINE}\n`,
    });
    profile = createTestProfile();
    session = await startApp({ vault, profile });
    const { browser } = session;
    const row = browser.$('[role="treeitem"][aria-label="Узел"]');
    await row.waitForDisplayed({ timeout: 20_000 });
    await row.click();
    await browser.$(TEXT).waitForDisplayed({ timeout: 10_000 });
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (vault) removeTestVault(vault);
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  it('правая панель показывает ссылающуюся заметку и строку со ссылкой', async () => {
    if (!session) throw new Error('нет сессии');
    const b = session.browser;
    await b.$('button.panel-toggle').click();
    await b.$('[role="tab"][aria-label="Обратные ссылки"]').click();
    // Текст — через textContent (как в других сценариях): getText в WebKitWebDriver
    // отдаёт пустую строку для элементов внутри кнопки.
    const nameText = () => b.execute(() => document.querySelector('.backlinks .groups .note .name')?.textContent ?? '');
    await b.waitUntil(async () => (await nameText()) === 'Дневник', {
      timeout: 15_000,
      timeoutMsg: 'нет ссылающейся заметки в панели',
    });
    const line = await b.execute(() => {
      const button = document.querySelector('.backlinks .lines .line');
      return {
        text: button?.textContent ?? '',
        link: button?.querySelector('mark.link')?.textContent ?? '',
        // Строка показана текстом: разметки из заметки в панели нет.
        injected: button?.querySelector('b') !== null,
      };
    });
    expect(line).toEqual({ text: LINE, link: '[[Узел]]', injected: false });
    for (const theme of ['light', 'dark'] as const) {
      await b.execute((value) => {
        document.documentElement.dataset.theme = value;
      }, theme);
      await saveScreenshot(b, `backlinks-01-real-core-${theme}`);
    }
    await b.execute(() => {
      delete document.documentElement.dataset.theme;
    });
  });
});
