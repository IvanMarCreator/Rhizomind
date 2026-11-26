import { appendFileSync, existsSync, statSync } from 'node:fs';
import path from 'node:path';
import { expect, it } from 'vitest';
import { startApp } from '../e2e/driver.ts';
import { indexReady, openFromManager } from './open-window.ts';
import { median, ms } from './stats.ts';
import { prepareVault, removeVault } from './vault.ts';

const words = ['проект', 'отчёт', 'план', 'заметка', 'работа'];
const count = Number(process.env.RHIZOMIND_PERF_NOTES ?? 10000);
const repeats = 5;

async function firstScreen(browser: WebdriverIO.Browser, text: string): Promise<number> {
  return browser.execute(async (word) => {
    const field = document.querySelector<HTMLInputElement>('.vault-search input[type="search"]');
    if (!field) throw new Error('Нет окна поиска');
    const enter = (value: string) => {
      field.value = value;
      field.dispatchEvent(new Event('input', { bubbles: true }));
    };
    enter('');
    await new Promise((resolve) => requestAnimationFrame(resolve));
    if (document.querySelector('.vault-search .result')) throw new Error('Остался результат предыдущего запроса');
    const start = performance.now();
    enter(word);
    while (performance.now() - start < 15000) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      if (document.querySelector('.vault-search .result')) return performance.now() - start;
    }
    throw new Error('Нет видимых результатов для ' + word);
  }, text);
}

async function switcher(browser: WebdriverIO.Browser): Promise<number> {
  return browser.execute(async () => {
    const start = performance.now();
    const target = document.activeElement ?? document.body;
    target.dispatchEvent(new KeyboardEvent('keydown', { key: 'o', code: 'KeyO', ctrlKey: true, bubbles: true, cancelable: true }));
    while (performance.now() - start < 15000) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      if (document.querySelector('.switcher [role="option"]')) return performance.now() - start;
    }
    throw new Error('Ctrl+O не открыл список');
  });
}

it(`полнотекстовый поиск в окне на ${count} заметках`, async () => {
  const vault = prepareVault(count);
  const report = [`Поиск в окне, ${process.platform}, ${count} заметок`];
  const misses: string[] = [];
  const index = path.join(vault.root, '.rhizomind', 'index.db');
  try {
    const cold = await startApp({ pickFolder: vault.root });
    try {
      const tree = await openFromManager(cold.browser, vault);
      const ready = await indexReady(cold.browser);
      await cold.browser.waitUntil(() => existsSync(index), { timeout: 180000 });
      report.push(`Дерево при холодном открытии ${ms(tree.total)} мс; готовый индекс от навигации ${ms(ready)} мс; база ${statSync(index).size} Б.`);
      // Критерий 0.2 для дерева проверяет links.test.ts по медиане трёх холодных запусков.
      // Здесь фиксируем открытие именно той же сессии, в которой подготовлен поиск.
    } finally { await cold.stop(); }

    const warm = await startApp({ pickFolder: vault.root });
    try {
      const browser = warm.browser;
      await openFromManager(browser, vault);
      report.push(`Готовый индекс при повторном открытии ${ms(await indexReady(browser))} мс.`);
      await browser.keys('F12');
      const filter = browser.$('input[aria-label="Найти команду…"]');
      await filter.waitForDisplayed();
      await filter.setValue('Поиск по хранилищу');
      await browser.keys('Enter');
      await browser.$('.vault-search input[type="search"]').waitForDisplayed();
      for (const word of words) {
        const times: number[] = [];
        for (let index = 0; index < repeats; index++) times.push(await firstScreen(browser, word));
        const value = median(times);
        const verdict = count === 10000 ? value <= 200 ? '≤ 200 мс' : 'выше 200 мс' : 'справочно';
        report.push(`${word}: ${times.map(ms).join(' / ')} мс, медиана ${ms(value)} мс, ${verdict}.`);
        if (count === 10000 && value > 200) misses.push(word);
      }
      await browser.keys('Escape');
      const times: number[] = [];
      for (let index = 0; index < repeats; index++) {
        times.push(await switcher(browser));
        await browser.keys('Escape');
      }
      report.push(`Ctrl+O: ${times.map(ms).join(' / ')} мс, медиана ${ms(median(times))} мс.`);
    } finally { await warm.stop(); }
  } finally {
    const output = report.join('\n');
    console.log(output);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, output + '\n\n');
    removeVault(vault);
  }
  expect(misses).toEqual([]);
});
