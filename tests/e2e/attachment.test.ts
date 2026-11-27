import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { openFromTree, TEXT, themeShots, waitForFileText } from './kt1.ts';

const shot = themeShots('attachment');
const dialog = '[role="dialog"][aria-label="Вставить вложение"]';

// Независимый владелец системного буфера передаёт нарисованный пиксель.
// В X11 он обслуживает запрос получения байтов до окончания вставки.
const copiedPath = process.platform === 'win32' ? 'C:/external/rhizomind-fixture.png' : '/external/rhizomind-fixture.png';

async function setClipboard(kind: 'image' | 'path'): Promise<ChildProcess> {
  const windows = process.platform === 'win32';
  const win = [
    'Add-Type -AssemblyName System.Windows.Forms', 'Add-Type -AssemblyName System.Drawing',
    ...(kind === 'image' ? [
      '$image = New-Object System.Drawing.Bitmap 2,2',
      '$image.SetPixel(0,0,[System.Drawing.Color]::Red)',
      '[System.Windows.Forms.Clipboard]::SetImage($image)',
    ] : [`[System.Windows.Forms.Clipboard]::SetText('${copiedPath}')`]),
    "Write-Output 'ready'",
  ].join('; ');
  const linux = [
    'import ctypes as c', "gtk=c.CDLL('libgtk-3.so.0')", "gdk=c.CDLL('libgdk-3.so.0')", "pix=c.CDLL('libgdk_pixbuf-2.0.so.0')",
    'gtk.gtk_init(None,None)', 'gdk.gdk_atom_intern_static_string.restype=c.c_void_p',
    'gtk.gtk_clipboard_get.argtypes=[c.c_void_p]', 'gtk.gtk_clipboard_get.restype=c.c_void_p',
    "cb=gtk.gtk_clipboard_get(gdk.gdk_atom_intern_static_string(b'CLIPBOARD'))",
    ...(kind === 'image' ? [
      'pix.gdk_pixbuf_new.restype=c.c_void_p', 'p=pix.gdk_pixbuf_new(0,0,8,2,2)',
      'pix.gdk_pixbuf_fill.argtypes=[c.c_void_p,c.c_uint32]', 'pix.gdk_pixbuf_fill(p,0xff0000ff)',
      'gtk.gtk_clipboard_set_image.argtypes=[c.c_void_p,c.c_void_p]', 'gtk.gtk_clipboard_set_image(cb,p)',
    ] : [
      'gtk.gtk_clipboard_set_text.argtypes=[c.c_void_p,c.c_char_p,c.c_int]',
      `gtk.gtk_clipboard_set_text(cb,b'${copiedPath}',-1)`,
    ]),
    "print('ready',flush=True)", 'gtk.gtk_main()',
  ].join('\n');
  const helper = windows
    ? spawn('powershell.exe', ['-NoProfile','-STA','-Command',win], { windowsHide: true })
    : spawn('python3', ['-c',linux]);
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => { helper.kill(); reject(new Error('буфер не готов')); }, 20_000);
    const fail = (error: unknown) => { clearTimeout(timer); reject(error); };
    helper.once('error', fail);
    helper.once('exit', (code) => { clearTimeout(timer); if (code) reject(new Error('буфер: ' + String(code))); });
    helper.stdout?.on('data', (chunk: Buffer) => { if (chunk.toString().includes('ready')) { clearTimeout(timer); resolve(); } });
  });
  return helper;
}

describe('вставка вложений', () => {
  let vault: string;
  let session: Session;
  beforeAll(async () => {
    vault = createTestVault('Вставка', { 'План.md': 'Начало' });
    mkdirSync(path.join(vault, 'Исходники'));
    writeFileSync(path.join(vault, 'Исходники', 'Таблица.csv'), 'яблоко;груша\r\n');
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

  it('Ctrl+V после «Копировать как путь» показывает подсказку и сохраняет заметку побайтно', async () => {
    const { browser } = session;
    const note = path.join(vault, 'План.md');
    const before = readFileSync(note);
    const helper = await setClipboard('path');
    try {
      await browser.$(TEXT).click();
      await browser.keys(['Control','v']);

      await browser.waitUntil(async () => (await browser.$('body').getText()).includes('Сначала скопируйте файл'), { timeout: 7000 });
      expect(await browser.$(TEXT).getText()).not.toContain(copiedPath);
      expect(readFileSync(note)).toEqual(before);
      expect(readdirSync(vault)).not.toContain('rhizomind-fixture.png');

      await shot(browser,'external-path');
    } finally { if (helper.exitCode === null) helper.kill(); }
  });
});
