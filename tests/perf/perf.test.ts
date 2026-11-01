// Критерий С9.5 (задача 3.2): хранилище из 10 000 заметок — дерево показано
// не позже 3 секунд после открытия; быстрый переход отвечает не дольше 100 мс;
// набор текста в заметке на 5 000 строк без заметных задержек.
//
// Настоящее окно тестовой сборки (`npm run build:e2e`: выпускной профиль
// Cargo, интерфейс встроен в исполняемый файл, плюс порт отладки для
// WebDriver) под tauri-driver, как сценарные тесты tests/e2e.
//
// Что и как меряется (подробно — docs/reviews/2026-11-01-task-3.2-perf.md):
// 1. Дерево: окно «Управление хранилищами» → «Открыть папку как хранилище»
//    (ответ окна выбора папки подставлен) → в новом окне все записи корня в
//    дереве. От щелчка до момента, когда тест увидел дерево (часы теста —
//    оценка сверху: опрос окна добавляет десятки мс). Пять запусков программы.
// 2. Быстрый переход: Ctrl+O → первые пункты списка; затем каждая буква
//    запроса → обновлённый список на экране. Пять запросов (имена настоящих
//    заметок); повтор — худшее нажатие.
// 3. Ввод: открыть «Длинную заметку 5000 строк», печатать в конце, в начале
//    и в середине — по 20 одиночных нажатий (следующее — после кадра с
//    предыдущим, темп ~10–30 символов в секунду, как быстрый набор). Повтор —
//    95-й процентиль задержки «нажатие → кадр» по 60 нажатиям. Предел 100 мс —
//    порог, до которого отклик воспринимается мгновенным (Nielsen, 0,1 с;
//    модель RAIL — ответ на ввод до 100 мс); 50 мс — цель, отмечается в отчёте.
//    Справочно — серия из 30 символов одним вызовом WebDriver: нажатия
//    приходят быстрее, чем человек печатает, и задержка включает очередь.
// Нажатия — временем движка окна (probe.ts), без задержек WebDriver.
// Вывод — по медиане повторов (stats.ts: почему не одна попытка).
import { mkdirSync, appendFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { startApp, type Session } from '../e2e/driver.ts';
import { saveScreenshot } from '../e2e/screenshot.ts';
import { armProbe, collectProbe, finishProbe } from './probe.ts';
import { median, ms, nth, renderTable, summarize, type Measured } from './stats.ts';
import { LONG_LINES, LONG_MARKER, LONG_TITLE, NOTES, SEED, prepareVault, removeVault, type PerfVault } from './vault.ts';

const ROOT = path.join(import.meta.dirname, '..', '..');
const REPORT_DIR = path.join(ROOT, 'test-results', 'perf');
const MANAGER = '[data-testid="vault-manager"]';
const SWITCHER = 'input[role="combobox"][aria-label="Быстрый переход"]';
const OPTION_SELECTED = '[role="option"][aria-selected="true"] .name';

const TREE_REPEATS = 5;
const TYPING_REPEATS = 3;
const SINGLE_KEYS = 20;
/** Серия подряд одним вызовом WebDriver: 29 символов и пробел. */
const BURST = 'fast typing in a long note ok';
const TYPED = 'quick brown fox jumps over ';

const measured: Measured[] = [];
const extra: string[] = [];
let environment: Record<string, string | number> = {};

function log(text: string): void {
  console.log(`[perf] ${text}`);
}

interface TreeOpen {
  /** Щелчок → дерево увидел тест (часы теста), мс. */
  total: number;
  /** Щелчок → начало загрузки страницы нового окна (часы окна). */
  navigation: number;
  /** Щелчок → DOMContentLoaded нового окна: скрипт интерфейса загружен и выполнен; `null` — нет данных. */
  domReady: number | null;
  /** Начало и конец запроса `tree_list` от щелчка; `null` — движок не даёт Resource Timing для ipc. */
  treeList: { start: number; end: number } | null;
  /** Щелчок → дерево в DOM (часы окна). */
  rendered: number;
}

/** «Открыть папку как хранилище» в окне управления; ждёт все записи корня в дереве нового окна. */
async function openFromManager(browser: WebdriverIO.Browser, vault: PerfVault): Promise<TreeOpen> {
  await browser.$(MANAGER).waitForDisplayed({ timeout: 60_000 });
  const manager = await browser.getWindowHandle();
  const button = browser.$('button[aria-label="Открыть папку как хранилище"]');
  await button.waitForClickable({ timeout: 10_000 });
  const clickAt = Date.now();
  await button.click();
  const deadline = clickAt + 60_000;
  let current = manager;
  for (;;) {
    if (Date.now() > deadline) throw new Error(`дерево не показано за 60 с (записей корня ${vault.rootEntries})`);
    const other = (await browser.getWindowHandles()).find((handle) => handle !== manager);
    if (!other) continue;
    try {
      if (current !== other) {
        await browser.switchToWindow(other);
        current = other;
      }
      const state = await browser.execute(() => {
        const nav = performance.timeOrigin;
        const ipc = performance
          .getEntriesByType('resource')
          .filter((e) => /tree_list/.test(e.name))
          .map((e) => ({ start: nav + e.startTime, end: nav + (e as PerformanceResourceTiming).responseEnd }));
        const page = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
        return {
          items: document.querySelectorAll('[role="treeitem"]').length,
          nav,
          domReady: page && page.domContentLoadedEventEnd > 0 ? nav + page.domContentLoadedEventEnd : null,
          now: nav + performance.now(),
          treeList: ipc[0] ?? null,
        };
      });
      if (state.items >= vault.rootEntries) {
        const seen = Date.now();
        return {
          total: seen - clickAt,
          navigation: state.nav - clickAt,
          domReady: state.domReady === null ? null : state.domReady - clickAt,
          treeList: state.treeList ? { start: state.treeList.start - clickAt, end: state.treeList.end - clickAt } : null,
          rendered: state.now - clickAt,
        };
      }
    } catch {
      // Окно ещё создаётся (WebDriver видит его раньше, чем страницу).
    }
  }
}

/** Быстрый переход закрыт (остался открытым после сбоя прошлой проверки — Esc). */
async function switcherClosed(browser: WebdriverIO.Browser): Promise<void> {
  if (await browser.$(SWITCHER).isExisting()) await browser.keys('Escape');
  await browser.$(SWITCHER).waitForExist({ reverse: true, timeout: 5_000 });
}

/** Ctrl+O, запрос, Enter на пункте `title`. */
async function openNote(browser: WebdriverIO.Browser, query: string, title: string | null): Promise<void> {
  await switcherClosed(browser);
  await browser.keys(['Control', 'o']);
  await browser.$(SWITCHER).waitForDisplayed({ timeout: 5_000 });
  await browser.keys(query);
  await browser.waitUntil(
    async () => {
      const name = await browser.execute((sel) => document.querySelector(sel)?.textContent ?? null, OPTION_SELECTED);
      return title === null ? name !== null && name !== LONG_TITLE : name === title;
    },
    { timeout: 10_000, timeoutMsg: `быстрый переход: нет пункта ${title ?? 'для ' + query}` },
  );
}

/** Каретка: конец, начало или середина длинной заметки (строка без ссылок, в конце строки). */
async function placeCaret(browser: WebdriverIO.Browser, where: 'конец' | 'начало' | 'середина'): Promise<void> {
  await browser.$('.cm-content').click();
  if (where === 'конец') await browser.keys(['Control', 'End']);
  else if (where === 'начало') await browser.keys(['Control', 'Home']);
  else {
    // Строку в середине выбирает окно, щелчок — по координатам: CodeMirror
    // перерисовывает строки после прокрутки, пометка атрибутом не доживает до щелчка.
    const point = await browser.execute(async () => {
      // Прокручивается ближайший предок текста с прокруткой (не обязательно .cm-scroller).
      let scroller = document.querySelector<HTMLElement>('.cm-content');
      while (scroller && !(scroller.scrollHeight > scroller.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(scroller).overflowY))) {
        scroller = scroller.parentElement;
      }
      if (!scroller) throw new Error('нет прокручиваемого предка у текста заметки');
      scroller.scrollTop = (scroller.scrollHeight - scroller.clientHeight) / 2;
      for (let i = 0; i < 4; i++) await new Promise((r) => requestAnimationFrame(r));
      const rect = scroller.getBoundingClientRect();
      const box = { top: Math.max(rect.top, 0), bottom: Math.min(rect.bottom, window.innerHeight) };
      const centre = (box.top + box.bottom) / 2;
      const lines = [...document.querySelectorAll<HTMLElement>('.cm-line')]
        .map((line) => ({ rect: line.getBoundingClientRect(), text: line.textContent ?? '', links: line.querySelector('a, [class*="link"]') }))
        .filter(({ rect, text, links }) => rect.top > box.top && rect.bottom < box.bottom && text.length > 10 && !links && !/[[\]()<>`#*_|]|http/.test(text))
        .sort((a, b) => Math.abs(a.rect.top - centre) - Math.abs(b.rect.top - centre));
      const line = lines[0];
      if (!line) throw new Error('нет подходящей строки в середине заметки');
      return { x: Math.round(line.rect.left + 4), y: Math.round(line.rect.top + line.rect.height / 2) };
    });
    await browser.action('pointer').move({ x: point.x, y: point.y, origin: 'viewport' }).down().up().perform();
    await browser.keys('End');
  }
}

describe(`С9.5: хранилище из ${NOTES} заметок`, () => {
  let vault: PerfVault;
  let session: Session | undefined;

  beforeAll(() => {
    vault = prepareVault();
    log(`хранилище: ${vault.root}; ${NOTES} заметок (зерно ${SEED}), записей в корне ${vault.rootEntries}; генерация ${vault.generateMs} мс`);
    log(`запросы быстрого перехода: ${vault.queries.join(', ')}`);
    environment = {
      'ОС': `${process.platform} ${os.release()}`,
      'Процессор': `${os.cpus()[0]?.model ?? '?'} × ${os.cpus().length}`,
      'Память': `${Math.round(os.totalmem() / 2 ** 30)} ГБ`,
      'Хранилище': `${NOTES} заметок, gen-vault --seed ${SEED} (+ длинная заметка ${LONG_LINES} строк), записей в корне ${vault.rootEntries}`,
      'Генерация хранилища': `${vault.generateMs} мс`,
    };
  });

  afterAll(async () => {
    await session?.stop();
    if (vault) removeVault(vault);
    writeReport();
  });

  /** Сессия в окне хранилища: последняя из замера дерева или новая. */
  async function vaultSession(): Promise<WebdriverIO.Browser> {
    if (!session) {
      session = await startApp({ vault: vault.root });
      await session.browser.$('[role="treeitem"]').waitForDisplayed({ timeout: 60_000 });
    }
    return session.browser;
  }

  it('дерево показано не позже 3 с после открытия хранилища', async () => {
    const runs: TreeOpen[] = [];
    for (let i = 1; i <= TREE_REPEATS; i++) {
      const s = await startApp({ pickFolder: vault.root });
      try {
        const run = await openFromManager(s.browser, vault);
        runs.push(run);
        log(
          `дерево, запуск ${i}: ${ms(run.total)} мс (окно начало загрузку через ${ms(run.navigation)} мс; ` +
            `tree_list ${run.treeList === null ? 'нет данных' : `${ms(run.treeList.start)}–${ms(run.treeList.end)} мс`}; ` +
            `дерево в DOM ${ms(run.rendered)} мс)`,
        );
        if (i === 1) {
          environment['Движок окна'] = await s.browser.execute(() => navigator.userAgent);
          await saveScreenshot(s.browser, 'perf-tree');
        }
      } finally {
        if (i < TREE_REPEATS) await s.stop();
        else session = s;
      }
    }
    measured.push({
      id: 'tree',
      title: 'Дерево показано после открытия хранилища',
      limitMs: 3000,
      repeatStat: 'max',
      repeats: runs.map((r) => [r.total]),
    });
    const phase = (id: string, title: string, values: (number | null)[]) => {
      const known = values.filter((v): v is number => v !== null);
      if (known.length > 0) measured.push({ id, title, limitMs: null, repeatStat: 'max', repeats: known.map((v) => [v]) });
    };
    phase('tree.core', 'Дерево: щелчок → окно начало загрузку (ядро: обход папки, индекс; создание окна)', runs.map((r) => r.navigation));
    phase('tree.dom', 'Дерево: начало загрузки окна → DOMContentLoaded (скрипт интерфейса)', runs.map((r) => (r.domReady === null ? null : r.domReady - r.navigation)));
    phase('tree.ui', 'Дерево: DOMContentLoaded → строки в DOM (запуск интерфейса, tree_list, отрисовка)', runs.map((r) => (r.domReady === null ? null : r.rendered - r.domReady)));
    phase('tree.boot', 'Дерево: загрузка окна → запрос tree_list', runs.map((r) => (r.treeList ? r.treeList.start - r.navigation : null)));
    phase('tree.list', 'Дерево: запрос tree_list (ядро + IPC)', runs.map((r) => (r.treeList ? r.treeList.end - r.treeList.start : null)));
    phase('tree.render', 'Дерево: ответ tree_list → строки в DOM', runs.map((r) => (r.treeList ? r.rendered - r.treeList.end : null)));
  });

  it('быстрый переход отвечает не дольше 100 мс', async () => {
    const browser = await vaultSession();
    const repeats: number[][] = [];
    const opens: number[] = [];
    const ipc: number[] = [];
    for (const query of vault.queries) {
      await switcherClosed(browser);
      await armProbe(browser, 'options');
      await browser.keys(['Control', 'o']);
      const open = nth(await collectProbe(browser, 1), 0);
      await finishProbe(browser);
      await armProbe(browser, 'listbox');
      let keys: number[] = [];
      try {
        for (const letter of query) {
          await browser.keys(letter);
          keys = await collectProbe(browser, keys.length + 1);
        }
      } finally {
        await saveScreenshot(browser, 'perf-switcher');
      }
      const extras = await finishProbe(browser);
      ipc.push(...extras.ipc.filter((e) => e.command.startsWith('switcher_search')).map((e) => e.ms));
      expect(await browser.$(SWITCHER).getValue()).toBe(query);
      opens.push(open);
      repeats.push([open, ...keys]);
      log(`быстрый переход «${query}»: открытие ${ms(open)} мс, буквы ${keys.map(ms).join(' / ')} мс`);
      await browser.keys('Escape');
    }
    measured.push({ id: 'switcher', title: 'Быстрый переход: отклик (открытие и каждая буква)', limitMs: 100, repeatStat: 'max', repeats });
    measured.push({ id: 'switcher.open', title: 'Быстрый переход: Ctrl+O → список', limitMs: null, repeatStat: 'max', repeats: opens.map((v) => [v]) });
    if (ipc.length > 0) {
      measured.push({ id: 'switcher.ipc', title: 'Быстрый переход: запрос switcher_search (ядро + IPC)', limitMs: null, repeatStat: 'max', repeats: [ipc] });
    }
  });

  it(`набор текста в заметке на ${LONG_LINES} строк без заметных задержек`, async () => {
    const browser = await vaultSession();
    const repeats: number[][] = [];
    const bursts: number[][] = [];
    const opens: number[] = [];
    let longTasks: number[] = [];
    let eventMax = 0;
    for (let r = 0; r < TYPING_REPEATS; r++) {
      await openNote(browser, 'длинная зам', LONG_TITLE);
      await armProbe(browser, 'editor-text', LONG_MARKER);
      await browser.keys('Enter');
      const open = nth(await collectProbe(browser, 1, 30_000), 0);
      await finishProbe(browser);
      opens.push(open);
      await browser.$('.cm-content').waitForDisplayed({ timeout: 10_000 });

      const latencies: number[] = [];
      const burstLatencies: number[] = [];
      for (const where of ['конец', 'начало', 'середина'] as const) {
        await placeCaret(browser, where);
        await armProbe(browser, 'editor');
        for (let i = 0; i < SINGLE_KEYS; i++) {
          await browser.keys(TYPED.charAt(i % TYPED.length));
          await collectProbe(browser, i + 1);
        }
        await browser.keys(`${BURST} `);
        const all = await collectProbe(browser, SINGLE_KEYS + BURST.length + 1, 20_000);
        const extras = await finishProbe(browser);
        longTasks = longTasks.concat(extras.longTasks);
        eventMax = Math.max(eventMax, extras.eventMax);
        const single = all.slice(0, SINGLE_KEYS);
        const burst = all.slice(SINGLE_KEYS);
        latencies.push(...single);
        burstLatencies.push(...burst);
        log(
          `ввод, повтор ${r + 1}, ${where}: одиночные — медиана ${ms(median(single))}, ` +
            `худшее ${ms(Math.max(...single))} мс; серия — худшее ${ms(Math.max(...burst))} мс`,
        );
      }
      repeats.push(latencies);
      bursts.push(burstLatencies);
      if (r === 0) await saveScreenshot(browser, 'perf-typing');
      // Уйти на другую заметку: следующий повтор снова открывает длинную.
      await openNote(browser, vault.queries[r % vault.queries.length] ?? 'а', null);
      await browser.keys('Enter');
      await browser.waitUntil(
        async () => !(await browser.execute((m) => (document.querySelector('.cm-content')?.textContent ?? '').includes(m), LONG_MARKER)),
        { timeout: 10_000, timeoutMsg: 'длинная заметка не закрылась' },
      );
    }
    const typing: Measured = {
      id: 'typing',
      title: `Ввод в заметке на ${LONG_LINES} строк: нажатие → кадр (95-й процентиль)`,
      limitMs: 100,
      repeatStat: 'p95',
      repeats,
    };
    measured.push(typing);
    measured.push({ id: 'typing.max', title: 'Ввод: худшее одиночное нажатие в повторе', limitMs: null, repeatStat: 'max', repeats });
    measured.push({ id: 'typing.burst', title: `Ввод: серия ${BURST.length + 1} символов без пауз (95-й процентиль, с очередью)`, limitMs: null, repeatStat: 'p95', repeats: bursts });
    measured.push({ id: 'note.open', title: `Открытие заметки на ${LONG_LINES} строк (Enter → текст на экране)`, limitMs: null, repeatStat: 'max', repeats: opens.map((v) => [v]) });
    const p95 = summarize(typing).median;
    extra.push(
      `Ввод: цель 50 мс по 95-му процентилю ${p95 <= 50 ? 'достигнута' : 'НЕ достигнута'} (${ms(p95)} мс).`,
      longTasks.length > 0
        ? `Длинные задачи (> 50 мс) за время ввода: ${longTasks.length}, самая долгая ${ms(Math.max(...longTasks))} мс.`
        : 'Длинных задач (> 50 мс) за время ввода нет или движок не даёт Long Tasks API.',
      eventMax > 0 ? `Event Timing: самое долгое событие ввода ${ms(eventMax)} мс.` : 'Event Timing: событий дольше 16 мс нет или движок не даёт API.',
    );
  });

  it('сводка: критерии С9.5 укладываются в пределы', () => {
    const summaries = measured.map(summarize);
    const failed = summaries.filter((s) => s.pass === false);
    expect(
      failed.map((s) => s.title),
      `критерий С9.5 не выполнен:\n${renderTable(summaries)}`,
    ).toEqual([]);
    expect(summaries.filter((s) => s.pass !== null).map((s) => s.id)).toEqual(['tree', 'switcher', 'typing']);
  });
});

function writeReport(): void {
  mkdirSync(REPORT_DIR, { recursive: true });
  const summaries = measured.map(summarize);
  const name = `perf-${process.platform}`;
  writeFileSync(
    path.join(REPORT_DIR, `${name}.json`),
    `${JSON.stringify({ environment, summaries, measured, notes: extra }, null, 2)}\n`,
  );
  const markdown = [
    `## Производительность (С9.5), ${process.platform}`,
    '',
    ...Object.entries(environment).map(([key, value]) => `- ${key}: ${value}`),
    '',
    renderTable(summaries),
    '',
    ...extra.map((line) => `- ${line}`),
    '',
  ].join('\n');
  writeFileSync(path.join(REPORT_DIR, `${name}.md`), markdown);
  console.log(`\n${markdown}`);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${markdown}\n`);
}
