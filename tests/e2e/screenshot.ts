// Снимки экрана сценарных тестов: сохранение и проверка, что снимок не пустой
// (обзор задачи 0.2: одноцветный PNG означает, что окно не отрисовалось).
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { inflateSync } from 'node:zlib';
import { SCREENSHOT_DIR } from './driver.ts';

/** Разных значений байт в распакованных строках изображения. */
export function distinctBytes(png: Buffer): number {
  const chunks: Buffer[] = [];
  let offset = 8; // подпись PNG
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString('ascii', offset + 4, offset + 8);
    if (type === 'IDAT') chunks.push(png.subarray(offset + 8, offset + 8 + length));
    offset += 12 + length;
  }
  const pixels = inflateSync(Buffer.concat(chunks));
  return new Set(pixels).size;
}

/** Сохраняет снимок окна и возвращает путь; пустой или одноцветный снимок — ошибка. */
export async function saveScreenshot(browser: WebdriverIO.Browser, name: string): Promise<string> {
  const file = path.join(SCREENSHOT_DIR, `${name}-${process.platform}.png`);
  await browser.saveScreenshot(file);
  const variety = distinctBytes(readFileSync(file));
  if (variety < 16) throw new Error(`Снимок ${file} почти одноцветный (${variety} значений байт)`);
  return file;
}
