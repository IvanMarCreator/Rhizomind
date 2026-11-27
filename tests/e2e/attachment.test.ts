import { copiedPath, setClipboard } from './attachment-clipboard.ts';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { openFromTree, TEXT, themeShots, waitForFileText } from './kt1.ts';

const shot = themeShots('attachment');
const dialog = '[role="dialog"][aria-label="Вставить вложение"]';

describe('вставка вложений', () => {
  let vault: string;
  let session: Session;
  beforeAll(async () => {
    vault = createTestVault('Вставка', { 'План.md': 'Начало' });
    mkdirSync(path.join(vault, 'Исходники'));
    writeFileSync(path.join(vault, 'Исходники', 'Таблица.csv'), 'яблоко;груша\r\n');
    writeFileSync(path.join(vault, 'Исходники', 'Отчёт +ёж.bin'), Buffer.from([0, 255, 12, 99]));
    writeFileSync(path.join(vault, 'Исходники', 'Фото#1.bin'), Buffer.from([1, 128, 7]));
    session = await startApp({ vault });
  });
  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('выбирает внутренний файл, сохраняет его байты и вставляет различающую ссылку', async () => {
    const { browser } = session;
    await openFromTree(browser, 'План');
    await browser.$(TEXT).click();
    await browser.keys(['Control','End']);
    await browser.keys('F12');
    const filter = browser.$('[role="dialog"] input[role="combobox"]');
    await filter.waitForDisplayed();
    await filter.setValue('Вставить вложение');
    await browser.keys('Enter');
    const picker=browser.$(dialog);
    const row=picker.$('button=Исходники/Таблица.csv');
    await row.waitForDisplayed();
    await shot(browser,'files');
    await row.click();
    await picker.$('input').waitForDisplayed();
    await shot(browser,'name');
    await picker.$('button=Вставить вложение').click();
    await waitForFileText(path.join(vault,'План.md'), text => text.includes('![[/Таблица.csv]]'), 'ссылка на копию');
    expect(readFileSync(path.join(vault,'Таблица.csv'))).toEqual(readFileSync(path.join(vault,'Исходники','Таблица.csv')));
    await shot(browser,'inserted');
  });

  it('Ctrl+V записывает изображение из настоящего системного буфера', async () => {
    const { browser } = session;
    const helper = await setClipboard('image');
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','End']);
      await browser.keys(['Control','v']);
      await waitForFileText(path.join(vault,'План.md'), text => text.includes('![[Скриншот.png]]'), 'ссылка на скриншот');
      const image = readFileSync(path.join(vault,'Скриншот.png'));
      expect(image.subarray(0,8).toString('hex')).toBe('89504e470d0a1a0a');
      expect([image.readUInt32BE(16),image.readUInt32BE(20)]).toEqual([2,2]);
      expect(readdirSync(vault).filter((file) => file.endsWith('.png'))).toEqual(['Скриншот.png']);
      await shot(browser,'screenshot');
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it.each(['path','files'] as const)('Ctrl+V отклоняет внешний путь из формата %s и сохраняет заметку побайтно', async (format) => {
    const { browser } = session;
    const note = path.join(vault, 'План.md');
    const before = readFileSync(note);
    const previous = browser.$('//div[@class="notice"][p[contains(.,"Сначала скопируйте файл")]]/button');
    if (await previous.isExisting()) await previous.click();
    await browser.waitUntil(async () => !(await browser.$('body').getText()).includes('Сначала скопируйте файл'));
    const helper = await setClipboard(format);
    let requestedFileList = false;
    helper.stdout?.on('data', (chunk: Buffer) => { if (chunk.toString().includes('requested-file-list')) requestedFileList = true; });
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','v']);

      await browser.waitUntil(async () => (await browser.$('body').getText()).includes('Сначала скопируйте файл'), { timeout: 7000 });
      expect(await browser.$(TEXT).getText()).not.toContain(copiedPath);
      expect(readFileSync(note)).toEqual(before);
      expect(readdirSync(vault)).not.toContain('rhizomind-fixture.png');
      if (process.platform === 'linux' && format === 'files') expect(requestedFileList).toBe(true);

      await shot(browser,format === 'path' ? 'external-path' : 'external-file-list');
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it.each(['files','path'] as const)('копирует внутренний Unicode-файл из системного буфера формата %s', async (format) => {
    const { browser } = session;
    const source = path.join(vault, 'Исходники', 'Отчёт +ёж.bin');
    const helper = await setClipboard(format, source);
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','End']);
      await browser.keys(['Control','v']);
      const first = format === 'files';
      const name = first ? 'Отчёт +ёж.bin' : 'Отчёт +ёж 2.bin';
      const link = first ? '![[/Отчёт +ёж.bin]]' : '![[Отчёт +ёж 2.bin]]';
      await waitForFileText(path.join(vault, 'План.md'), (text) => text.includes(link), link);
      expect(readFileSync(path.join(vault,name))).toEqual(readFileSync(source));
      await shot(browser, first ? 'internal-file-list' : 'internal-text-path');
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it('просит допустимое имя внутреннего файла из буфера до создания копии', async () => {
    const { browser } = session;
    const source = path.join(vault, 'Исходники', 'Фото#1.bin');
    const helper = await setClipboard('files',source);
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','End']);
      await browser.keys(['Control','v']);
      const picker = browser.$(dialog);
      await picker.waitForDisplayed({ timeout: 7000 });
      expect(readdirSync(vault)).not.toContain('Фото#1.bin');
      const name = picker.$('input');
      expect(await name.getValue()).toBe('Фото#1.bin');
      await name.setValue('Фото 1.bin');
      await browser.keys('Enter');
      await waitForFileText(path.join(vault, 'План.md'), (text) => text.includes('![[Фото 1.bin]]'), 'ссылка после исправления имени');
      expect(readFileSync(path.join(vault,'Фото 1.bin'))).toEqual(readFileSync(source));
    } finally { if (helper.exitCode === null) helper.kill(); }
  });
});
