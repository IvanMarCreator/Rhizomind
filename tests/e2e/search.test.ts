import { readFileSync } from 'node:fs';
import path from 'node:path';
import { expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp } from './driver.ts';
import { themeShots } from './kt1.ts';

it('поиск из F12 находит русскую форму и открывает строку исходной заметки', async () => {
  const body = 'Начало\n\nПосле проверки отчёт готов';
  const vault = createTestVault('Поиск', { 'Квартал.md': body, 'Архив.md': 'Перепроверка' });
  const session = await startApp({ vault });
  try {
    const { browser } = session;
    await browser.$('[role="treeitem"][aria-label="Квартал"]').waitForDisplayed();
    await browser.keys('F12');
    const command = browser.$('input[aria-label="Найти команду…"]');
    await command.waitForDisplayed();
    await command.setValue('Поиск по хранилищу');
    await browser.keys('Enter');
    const input = browser.$('.vault-search input[type="search"]');
    await input.waitForDisplayed();
    await input.setValue('проверка');
    const row = browser.$('.vault-search .result');
    await row.waitForDisplayed();
    expect(await row.getText()).toContain('После проверки отчёт готов');
    expect(await browser.$$('.vault-search .result').length).toBe(1);
    await themeShots('search')(browser, 'результаты');
    await row.click();
    await browser.waitUntil(() => browser.execute(() => {
      const sel = window.getSelection();
      const element = sel?.anchorNode?.parentElement;
      return element?.closest('.cm-line')?.textContent === 'После проверки отчёт готов' && sel?.anchorOffset === 6;
    }));
    expect(readFileSync(path.join(vault, 'Квартал.md'), 'utf8')).toBe(body);
  } finally {
    await session.stop();
    removeTestVault(vault);
  }
});
