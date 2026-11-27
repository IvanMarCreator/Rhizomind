import { copiedPath, setClipboard } from './attachment-clipboard.ts';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { openFromTree, TEXT, themeShots, waitForFileText } from './kt1.ts';

const shot = themeShots('attachment');
const dialog = '[role="dialog"][aria-label="Вставить вложение"]';

describe('вставка вложений', () => {
  let vault: string;
  let session: Session;
  async function openNote(name: string): Promise<string> {
    await openFromTree(session.browser, name);
    await session.browser.$(TEXT).click();
    await session.browser.keys(['Control','End']);
    return path.join(vault, `${name}.md`);
  }
  beforeAll(async () => {
    const notes = ['План', 'Скриншот из буфера', 'Исправление имени',
      ...['path','files','mixed'].flatMap((format) => [`Внешний ${format}`, `Внутренний ${format}`])];
    vault = createTestVault('Вставка', Object.fromEntries(notes.map((name) => [`${name}.md`, 'Начало'])));
    mkdirSync(path.join(vault, 'Исходники'));
    writeFileSync(path.join(vault, 'Исходники', 'Таблица.csv'), 'яблоко;груша\r\n');
    for (const name of ['Отчёт +ёж.bin', 'Копия +ёж.bin', 'План +ёж.bin']) {
      writeFileSync(path.join(vault, 'Исходники', name), Buffer.from([0, 255, 12, 99]));
    }
    for (const name of ['Копия +ёж.bin', 'План +ёж.bin', 'План +ёж 2.bin']) {
      writeFileSync(path.join(vault, name), Buffer.from([22]));
    }
    writeFileSync(path.join(vault, 'Исходники', 'Фото#1.bin'), Buffer.from([1, 128, 7]));
    session = await startApp({ vault });
  });
  afterEach(async ({ task }) => {
    if (task.result?.state !== 'fail' || !session) return;
    const state = await session.browser.execute(() => ({
      focused: document.hasFocus(), active: document.activeElement?.getAttribute('class'),
      notices: [...document.querySelectorAll('.notice p, [role="alert"], [role="status"]')].map((node) => node.textContent),
      dialogs: [...document.querySelectorAll('[role="dialog"]')].map((node) => node.getAttribute('aria-label')),
    }));
    console.info('Видимое состояние окна после сбоя', state);
  });
  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
  });

  it('выбирает внутренний файл, сохраняет его байты и вставляет различающую ссылку', { timeout: 120_000 }, async () => {
    const { browser } = session;
    const note = await openNote('План');
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
    await waitForFileText(note, text => text.includes('![[/Таблица.csv]]'), 'ссылка на копию');
    expect(readFileSync(path.join(vault,'Таблица.csv'))).toEqual(readFileSync(path.join(vault,'Исходники','Таблица.csv')));
    await shot(browser,'inserted');
  });

  it('Ctrl+V записывает изображение из настоящего системного буфера', async () => {
    const { browser } = session;
    const note = await openNote('Скриншот из буфера');
    const helper = await setClipboard('image');
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','End']);
      await browser.keys(['Control','v']);
      await waitForFileText(note, text => text.includes('![[Скриншот.png]]'), 'ссылка на скриншот');
      const image = readFileSync(path.join(vault,'Скриншот.png'));
      expect(image.subarray(0,8).toString('hex')).toBe('89504e470d0a1a0a');
      expect([image.readUInt32BE(16),image.readUInt32BE(20)]).toEqual([2,2]);
      expect(readdirSync(vault).filter((file) => file.endsWith('.png'))).toEqual(['Скриншот.png']);
      await shot(browser,'screenshot');
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it.each(['path','files','mixed'] as const)('Ctrl+V отклоняет внешний путь из формата %s и сохраняет заметку побайтно', async (format) => {
    const { browser } = session;
    const note = await openNote(`Внешний ${format}`);
    const before = readFileSync(note);
    const images = readdirSync(vault).filter((name) => name.endsWith('.png'));
    const previous = browser.$('//section[@aria-live="polite"]//p[contains(.,"Сначала скопируйте файл")]/following-sibling::button');
    if (await previous.isExisting()) await previous.click();
    await browser.waitUntil(async () => !(await browser.$('body').getText()).includes('Сначала скопируйте файл'));
    const helper = await setClipboard(format);
    const requests: string[] = [];
    helper.stdout?.on('data', (chunk: Buffer) => requests.push(chunk.toString()));
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','v']);

      await browser.waitUntil(async () => (await browser.$('body').getText()).includes('Сначала скопируйте файл'), { timeout: 7000 });
      expect(await browser.$(TEXT).getText()).not.toContain(copiedPath);
      expect(readFileSync(note)).toEqual(before);
      expect(readdirSync(vault)).not.toContain('rhizomind-fixture.png');
      if (process.platform === 'linux' && format !== 'path') expect(requests.join('')).toContain('requested-file-list');
      expect(requests.join('')).not.toContain('requested-image');
      expect(readdirSync(vault).filter((file) => file.endsWith('.png'))).toEqual(images);
      await shot(browser, 'external-' + format);
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it.each([
    ['files', 'Отчёт +ёж.bin', 'Отчёт +ёж.bin', '/Отчёт +ёж.bin'],
    ['path', 'Копия +ёж.bin', 'Копия +ёж 2.bin', 'Копия +ёж 2.bin'],
    ['mixed', 'План +ёж.bin', 'План +ёж 3.bin', 'План +ёж 3.bin'],
  ] as const)('копирует внутренний Unicode-файл из системного буфера формата %s', async (format, original, name, target) => {
    const { browser } = session;
    const note = await openNote(`Внутренний ${format}`);
    const source = path.join(vault, 'Исходники', original);
    const images = readdirSync(vault).filter((name) => name.endsWith('.png'));
    const helper = await setClipboard(format, source);
    const requests: string[] = [];
    helper.stdout?.on('data', (chunk: Buffer) => requests.push(chunk.toString()));
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','End']);
      await browser.keys(['Control','v']);
      const link = `![[${target}]]`;
      await waitForFileText(note, (text) => text.includes(link), link);
      expect(readFileSync(path.join(vault,name))).toEqual(readFileSync(source));
      if (process.platform === 'linux' && format !== 'path') expect(requests.join('')).toContain('requested-file-list');
      expect(requests.join('')).not.toContain('requested-image');
      expect(readdirSync(vault).filter((file) => file.endsWith('.png'))).toEqual(images);
      await shot(browser, 'internal-' + format);
    } finally { if (helper.exitCode === null) helper.kill(); }
  });

  it('просит допустимое имя внутреннего файла из буфера до создания копии', async () => {
    const { browser } = session;
    const note = await openNote('Исправление имени');
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
      await waitForFileText(note, (text) => text.includes('![[Фото 1.bin]]'), 'ссылка после исправления имени');
      expect(readFileSync(path.join(vault,'Фото 1.bin'))).toEqual(readFileSync(source));
    } finally { if (helper.exitCode === null) helper.kill(); }
  });
});
