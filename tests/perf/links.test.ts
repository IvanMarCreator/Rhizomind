// Оконные критерии С2-8.3. Три независимых хранилища дают три холодных и три
// тёплых открытия, а также независимые переименования и переносы. Для каждого
// повтора берём новый root: прежний индекс и результаты правок не переиспользуются.
import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { startApp } from '../e2e/driver.ts';
import { openFromTree, showView } from '../e2e/kt1.ts';
import { dragRow } from '../e2e/drag-row.ts';
import { enterFromManager, openFromManager, type TreeOpen } from './open-window.ts';
import { ms, windowRun, type Measured } from './stats.ts';
import { LONG_TITLE, MOVE_TARGET, prepareVault, removeVault, type PerfVault } from './vault.ts';

const count = Number(process.env.RHIZOMIND_PERF_NOTES ?? 10_000);
if (count !== 10_000 && count !== 50_000) throw new Error('RHIZOMIND_PERF_NOTES: ожидается 10000 или 50000');
const REPEATS = 3;
const TAG = 'новыйтегзамера';
const GROUPS = '.backlinks .groups > .group';
const TAG_ROWS = '.tags .list > li[data-key]';
const enforce = process.platform === 'win32' && count === 10_000;
const criteria: [string, string, number, number][] = [
  ['tree.cold', 'Дерево при сборке индекса с нуля', 3000, 6000],
  ['index.warm', 'Готовый индекс после открытия окна с сохранённым индексом', 1000, 4000],
  ['backlinks', 'Открытие узла → 100 групп обратных ссылок', 200, 300],
  ['tags', 'Выбор вида «Теги» → полный список', 100, 200],
  ['save.links', 'Сохранение → новая обратная ссылка', 2000, 2000],
  ['save.tags', 'Сохранение → новый тег', 2000, 2000],
  ['rename', 'Ответ при переименовании узла → уведомление', 2000, 2000],
  ['move', 'Ответ при переносе папки → уведомление (ход выполнения виден)', 15000, 20000],
];
const metrics: Measured[] = criteria.map(([id, title, ms10k, ms50k]) => ({
  id, title, limitMs: count === 50_000 ? ms50k : ms10k, repeatStat: 'max', repeats: [],
}));
const coldOpens: TreeOpen[] = [];
let completedRuns = 0;
function record(id: string, value: number) {
  const metric = metrics.find((m) => m.id === id);
  if (!metric || !Number.isFinite(value) || value < 0) throw new Error(`Некорректный замер ${id}: ${value}`);
  metric.repeats.push([value]);
  console.log(`[perf] ${count} заметок, ${metric.title}: ${Math.round(value)} мс`);
}

async function indexReady(browser: WebdriverIO.Browser): Promise<number> {
  let elapsed = 0;
  await browser.waitUntil(async () => {
    const result = await browser.execute(async () => {
      const api = (window as unknown as { __TAURI_INTERNALS__?: { invoke(cmd: string): Promise<{ state: string }> } }).__TAURI_INTERNALS__;
      const status = await api?.invoke('index_status');
      return { state: status?.state, elapsed: performance.now() };
    });
    elapsed = result.elapsed;
    return result.state === 'ready';
  }, { timeout: 180_000, interval: 25, timeoutMsg: 'индекс не готов' });
  // performance.now() — от начала навигации окна. Опрос даёт верхнюю оценку.
  return elapsed;
}

/** Щелчок и ожидание кадра делаются внутри движка, без задержки опроса WebDriver. */
async function clickToRows(browser: WebdriverIO.Browser, click: string, rows: string, expected: number) {
  return browser.execute(async (click, rows, expected) => {
    const button = document.querySelector<HTMLElement>(click);
    if (!button || document.querySelectorAll(rows).length === expected) throw new Error(`Нет исходного состояния для ${click}`);
    const start = performance.now();
    button.click();
    while (performance.now() - start < 15_000) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      if (document.querySelectorAll(rows).length === expected) return performance.now() - start;
    }
    throw new Error(`Ожидалось ${expected} строк ${rows}, в панели ${document.querySelectorAll(rows).length}`);
  }, click, rows, expected);
}

/** От времени записи штатным автосохранением до первого наблюдения результата.
 * Момент записи берём с диска, на тех же часах, что Date.now() в тесте. */
async function saved(browser: WebdriverIO.Browser, file: string, addition: string, selector: string, targetRow?: string) {
  expect(await browser.$(selector).isExisting()).toBe(false);
  expect(readFileSync(file, 'utf8')).not.toContain(addition.trim());
  await browser.$('.cm-content').click();
  await browser.keys(['Control', 'End']);
  const beforeEdit = Date.now();
  await browser.execute((text) => {
    const data = new DataTransfer();
    data.setData('text/plain', text);
    const editor = document.querySelector('.cm-content');
    if (!editor) throw new Error('Нет редактора заметки');
    editor.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, addition);
  // Вид обратных ссылок показывает входящие ссылки текущей заметки. Переход
  // с отредактированного источника на цель также запускает штатное сохранение.
  if (targetRow) await browser.$(targetRow).click();
  await browser.waitUntil(async () => await browser.$(selector).isExisting() && readFileSync(file, 'utf8').includes(addition.trim()), {
    timeout: 30_000, interval: 25, timeoutMsg: selector,
  });
  const seen = Date.now();
  const savedAt = statSync(file).mtimeMs;
  // NTFS хранит доли миллисекунды; Date.now() округлён вниз до целых.
  expect(savedAt).toBeGreaterThanOrEqual(beforeEdit - 1);
  expect(savedAt).toBeLessThan(seen + 1);
  return Math.max(0, seen - savedAt);
}

async function answer(browser: WebdriverIO.Browser, requireProgress: boolean) {
  const update = browser.$('[data-testid="rename-dialog"]').$('button=Обновить ссылки');
  await update.waitForDisplayed({ timeout: 180_000 });
  // Замер внутри окна видит короткоживущее уведомление о ходе между опросами
  // WebDriver. Старые уведомления исключены по DOM-элементу.
  return browser.execute(async (progressRequired) => {
    const buttons = document.querySelectorAll<HTMLButtonElement>('[data-testid="rename-dialog"] button');
    const update = [...buttons].find((b) => b.textContent?.trim() === 'Обновить ссылки');
    if (!update) throw new Error('Нет кнопки обновления');
    const question = document.querySelector('[data-testid="rename-dialog"] h2')?.textContent ?? '';
    const expected = Number(/ссыла(?:ется|ются) (\d+) замет/u.exec(question)?.[1]);
    if (!Number.isInteger(expected) || (progressRequired ? expected < 2 : expected !== 100)) throw new Error(question);
    const prior = new Set(document.querySelectorAll('.notice'));
    const started = performance.now();
    let progress = false;
    update.click();
    while (performance.now() - started < 120_000) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const notices = [...document.querySelectorAll<HTMLElement>('.notice[data-tone]')].filter((node) => !prior.has(node));
      const warning = notices.find((node) => node.dataset.tone === 'warning');
      if (warning) throw new Error(warning.textContent ?? 'Ошибка обновления');
      for (const current of notices) {
        if (current.dataset.tone === 'progress' && current.getBoundingClientRect().height > 0) progress = true;
        if (current.dataset.tone === 'success' && current.textContent?.includes('Ссылки обновлены в')) {
          if (progressRequired && !progress) throw new Error('При переносе не показан ход обновления ссылок');
          const written = Number(/Ссылки обновлены в (\d+) замет/u.exec(current.textContent)?.[1]);
          if (written !== expected) throw new Error(`${question}; ${current.textContent}`);
          return performance.now() - started;
        }
      }
    }
    throw new Error('Нет уведомления об обновлении ссылок');
  }, requireProgress);
}

async function measureChanges(browser: WebdriverIO.Browser, vault: PerfVault) {
  const { hub, folder } = vault.fixture;
  const hubRow = `[role="treeitem"][aria-label="${hub}"]`;
  const tagTab = '[role="tab"][aria-label="Теги"]';
  await openFromTree(browser, LONG_TITLE);
  await showView(browser, 'Обратные ссылки');
  record('backlinks', await clickToRows(browser, hubRow, GROUPS, 100));
  const tagsMs = await clickToRows(browser, tagTab, TAG_ROWS, vault.tagKeys.length);
  const keys = await browser.execute((s) => [...document.querySelectorAll<HTMLElement>(s)].map((row) => row.dataset.key).sort(), TAG_ROWS);
  expect(keys).toEqual(vault.tagKeys);
  record('tags', tagsMs);

  const file = path.join(vault.root, `${LONG_TITLE}.md`);
  await openFromTree(browser, LONG_TITLE);
  const original = readFileSync(file);
  record('save.tags', await saved(browser, file, `\n#${TAG}\n`, `[data-key="${TAG}"]`));
  await showView(browser, 'Обратные ссылки');
  const link = `.backlinks button.note[aria-label="Открыть «${LONG_TITLE}»"]`;
  record('save.links', await saved(browser, file, `\nКонтрольная ссылка после сохранения [[${hub}]]\n`, link, hubRow));
  // Убираем добавленную ссылку по окончании замера, чтобы в переименовании
  // по-прежнему участвовали ровно 100 заметок манифеста.
  writeFileSync(file, original);
  await browser.waitUntil(() => browser.execute((s) => document.querySelectorAll(s).length === 100, GROUPS), { timeout: 30_000 });
  await browser.$(hubRow).click();
  await browser.keys('F2');
  await browser.$('input[aria-label="Новое имя"]').waitForDisplayed();
  await browser.keys(['Control', 'a']);
  await browser.keys('Переименованный узел');
  await browser.keys('Enter');
  const renameMs = await answer(browser, false);
  expect(existsSync(path.join(vault.root, 'Переименованный узел.md'))).toBe(true);
  expect(existsSync(path.join(vault.root, `${hub}.md`))).toBe(false);
  record('rename', renameMs);

  await dragRow(browser, folder, MOVE_TARGET);
  const moveMs = await answer(browser, true);
  expect(existsSync(path.join(vault.root, MOVE_TARGET, folder))).toBe(true);
  expect(existsSync(path.join(vault.root, folder))).toBe(false);
  for (const sample of vault.movedLinks) {
    const text = readFileSync(path.join(vault.root, sample.source), 'utf8');
    expect(text, sample.source).not.toContain(sample.oldLink);
    expect(text.split(sample.newLink).length - 1, sample.source).toBe(sample.expectedCount);
    expect(existsSync(path.join(vault.root, MOVE_TARGET, sample.target))).toBe(true);
  }
  record('move', moveMs);
}

describe(`С2-8.3: окно на ${count} заметок`, () => {
  for (let run = 1; run <= REPEATS; run++) {
    it(`холодное открытие, открытие с индексом, панели и изменения — повтор ${run}`, async () => {
      const vault = prepareVault(count);
      try {
        const index = path.join(vault.root, '.rhizomind', 'index.json');
        expect(existsSync(index)).toBe(false);
        const cold = await startApp({ pickFolder: vault.root });
        try {
          const tree = await openFromManager(cold.browser, vault);
          coldOpens.push(tree);
          record('tree.cold', tree.total);
          await indexReady(cold.browser);
          await cold.browser.waitUntil(() => existsSync(index), { timeout: 120_000, timeoutMsg: 'не записан index.json' });
        } finally {
          await cold.stop();
        }
        const warm = await startApp({ pickFolder: vault.root });
        try {
          await warm.browser.setTimeout({ script: 150_000 });
          await enterFromManager(warm.browser);
          record('index.warm', await indexReady(warm.browser));
          await measureChanges(warm.browser, vault);
        } finally {
          await warm.stop();
        }
      } finally {
        removeVault(vault);
      }
      completedRuns++;
    });
  }

  afterAll(() => {
    const { complete, progress, summaries, markdown } = windowRun(metrics, REPEATS, completedRuns);
    const directory = path.join(import.meta.dirname, '../../test-results/perf');
    const file = path.join(directory, `links-${process.platform}-${count}`);
    mkdirSync(directory, { recursive: true });
    const purpose = enforce ? 'Приёмочные пределы Windows.' : 'Справочное сравнение с пределами С2-8.3.';
    const stages = coldOpens.map((t, i) =>
      `| ${i + 1} | ${ms(t.navigation)} | ${t.domReady === null ? '—' : ms(t.domReady - t.navigation)} | ` +
      `${t.domReady === null ? '—' : ms(t.rendered - t.domReady)} | ${ms(t.total)} |`);
    const breakdown = ['Фазы холодного открытия, мс.', '', '| Повтор | Щелчок → окно | Окно → DOMContentLoaded | DOMContentLoaded → строки | Полное время |',
      '|---|---|---|---|---|', ...stages].join('\n');
    const report = `## Окно С2-8.3, ${process.platform}, ${count} заметок\n\n${purpose}\n\n${markdown}\n\n${breakdown}\n`;
    writeFileSync(`${file}.json`, JSON.stringify({ count, completedRuns, complete, progress, metrics, summaries, coldOpens }, null, 2));
    writeFileSync(`${file}.md`, report);
    console.log(report);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, report);
    expect(complete, report).toBe(true);
    if (enforce) expect(summaries.filter((s) => s.pass === false).map((s) => s.title), report).toEqual([]);
  });
});
