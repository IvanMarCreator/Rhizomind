import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { headerPath, openFromTree, TEXT, themeShots, waitForFileText } from './kt1.ts';

const shot = themeShots('template');
const bytes = Buffer.from('\uFEFF---\r\ntags: [дело]\r\n---\r\n[[Встреча]] {{дата}}\r\n');

describe('создание из шаблона через палитру', () => {
  let vault: string;
  let session: Session;

  beforeAll(async () => {
    vault = createTestVault('Шаблоны выпуска', { 'Вход.md': 'Открыть палитру' });
    for (const name of ['А', 'Б']) mkdirSync(path.join(vault, 'Образцы', name), { recursive: true });
    writeFileSync(path.join(vault, 'Образцы', 'А', 'План.md'), 'Другой шаблон');
    writeFileSync(path.join(vault, 'Образцы', 'Б', 'План.md'), bytes);
    session = await startApp({ vault });
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('F12 → папка → вложенный шаблон → копия, которая редактируется отдельно', async () => {
    const { browser } = session;
    await openFromTree(browser, 'Вход');
    await browser.keys('F12');
    const command = browser.$('[role="dialog"] input[role="combobox"]');
    await command.waitForDisplayed({ timeout: 5_000 });
    await command.setValue('Создать из шаблона');
    await browser.keys('Enter');

    const picker = browser.$('[role="dialog"][aria-label="Папка шаблонов"]');
    await picker.waitForDisplayed({ timeout: 5_000 });
    await shot(browser, 'folder');
    const folder = picker.$('button=Образцы');
    await folder.waitForDisplayed({ timeout: 5_000 });
    await folder.click();

    const dialog = browser.$('[role="dialog"][aria-label="Создать из шаблона"]');
    const chosen = dialog.$('button=Б/План.md');
    await chosen.waitForDisplayed({ timeout: 5_000 });
    expect(await dialog.$('button=А/План.md').isDisplayed()).toBe(true);
    await shot(browser, 'templates');
    await chosen.click();
    const name = dialog.$('input[name="note-title"]');
    await name.setValue('Результат');
    await shot(browser, 'name');
    await browser.keys('Enter');
    await browser.waitUntil(async () => (await headerPath(browser)) === 'Результат', { timeout: 10_000 });
    const result = path.join(vault, 'Результат.md');
    expect(readFileSync(result)).toEqual(bytes);
    await shot(browser, 'created');
    await browser.$(TEXT).click();
    await browser.keys(['Control', 'End']);
    await browser.keys('Копия');
    await waitForFileText(result, (text) => text.includes('Копия'), 'правка в новой заметке');
    expect(readFileSync(path.join(vault, 'Образцы', 'Б', 'План.md'))).toEqual(bytes);
  });
});
