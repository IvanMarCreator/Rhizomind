// Индекс хранилища на настоящем ядре (задача 0.2-8.3, С2-3.7, С2-4.1; КТ1,
// пп. 1 и 6): хранилище из генератора (tools/gen-vault) открывается через
// «Открыть папку как хранилище» в работающей программе — дерево сразу, в
// правой панели «Индекс строится… N %», потом список обратных ссылок и
// тегов. Затем программа закрывается, `.rhizomind/index.db` удаляется, и
// при новом открытии индекс собирается заново — обратные ссылки и теги те же.
//
// Сборка индекса начинается в ядре при открытии хранилища, поэтому окно
// открывается из окна управления уже после создания сеанса WebDriver (иначе
// запуск сеанса — до 30 с на Linux — перекрывает всю сборку), а раскладка
// `.rhizomind/workspace.json` заранее открывает заметку и правую панель.
// Состояние «строится» короткое (секунды); что успели увидеть, печатается в
// журнал и аннотацией GitHub Actions (`::notice`).
// Снимки — на каждом шаге в светлой и тёмной теме (`index-NN-…`).
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, SCREENSHOT_DIR, startApp, waitForAppExit, type Session } from './driver.ts';
import { backlinkGroups, generateVault, panelBody, showView, tagRows } from './kt1.ts';
import { distinctBytes, saveScreenshot } from './screenshot.ts';

type Theme = 'light' | 'dark';
type Opening = 'first-open' | 'reopen';
const steps = {
  'first-open': { building: '01', backlinks: '02', tags: '03' },
  reopen: { building: '04', backlinks: '05', tags: '06' },
} as const;

function frameName(opening: Opening, state: 'building' | 'backlinks' | 'tags', theme: Theme): string {
  return `index-${steps[opening][state]}-${opening}-${state}-${theme}`;
}

const SHELL = '[data-testid="shell"]';
const OPEN_FOLDER = 'button[aria-label="Открыть папку как хранилище"]';
/**
 * Заметок генератора — объём критерия «дерево ≤ 3 с» (С2-3.7). На Linux окно
 * в Xvfb появляется через 5–8 с, сборка к этому времени уже закончена (и на
 * 50 000 заметок), поэтому «строится» там не видно; на Windows — видно.
 */
const NOTES = Number(process.env.RHIZOMIND_E2E_KT1_NOTES ?? 10_000);
const TARGET = 'КТ1 Узел';
const SOURCE = 'КТ1 Ссылка';
const OWN_TAG = '#кт1-проверка, заметок: 1';
/** Индекс пишется не чаще раза в 30 с (С2-4.1) — ждём с запасом. */
const INDEX_WRITE_MS = 45_000;

/** Аннотация GitHub Actions (видна в сводке прогона без входа) и строка журнала. */
function notice(message: string): void {
  console.log(`[kt1] ${message}`);
  if (process.env.GITHUB_ACTIONS) console.log(`::notice title=КТ1 ${process.platform}::${message}`);
}

interface Observed {
  /** Тексты панели «строится», по порядку, без повторов. */
  building: string[];
  /** Все тексты панели до списка, по порядку, без повторов (для отчёта). */
  states: string[];
  /** Строк дерева, когда панель показывала «строится» (последний такой опрос); -1 — «строится» не видели. */
  treeWhileBuilding: number;
  /** Через сколько мс после «Открыть папку…» появилось окно хранилища и список. */
  shellMs: number;
  listMs: number;
}

/**
 * Окно управления → «Открыть папку как хранилище» (ответ окна выбора папки —
 * `pickFolder`) → окно хранилища. Пока нет списка обратных ссылок, панель
 * опрашивается без пауз. Тема уже выбрана в настройках хранилища.
 */
async function openFromManager(browser: WebdriverIO.Browser, step: Opening, theme: Theme): Promise<Observed> {
  await browser.$(OPEN_FOLDER).waitForDisplayed({ timeout: 30_000 });
  const started = Date.now();
  await browser.$(OPEN_FOLDER).click();
  const observed: Observed = { building: [], states: [], treeWhileBuilding: -1, shellMs: -1, listMs: -1 };
  const deadline = started + 120_000;
  // Окно хранилища.
  while (observed.shellMs < 0) {
    if (Date.now() > deadline) throw new Error('окно хранилища не появилось');
    for (const handle of await browser.getWindowHandles()) {
      try {
        await browser.switchToWindow(handle);
        if (await browser.execute((s) => document.querySelector(s) !== null, SHELL)) {
          observed.shellMs = Date.now() - started;
          break;
        }
      } catch {
        // Окно управления закрывается — его описатель пропадает.
      }
    }
  }
  let captured = false;
  for (;;) {
    if (Date.now() > deadline) throw new Error(`список не появился; панель: «${await panelBody(browser)}»`);
    const state = await browser.execute(() => ({
      body: (document.querySelector('aside.right-panel .body')?.textContent ?? '').replace(/\s+/g, ' ').trim(),
      theme: document.documentElement.dataset.theme,
      rows: document.querySelectorAll('[role="treeitem"]').length,
      ready: document.querySelector('.backlinks .groups > .group') !== null,
    }));
    const body = state.body;
    if (observed.states.at(-1) !== body) observed.states.push(body);
    if (body.startsWith('Индекс строится')) {
      if (observed.building.at(-1) !== body) observed.building.push(body);
      observed.treeWhileBuilding = state.rows;
      if (!captured && state.theme === theme && state.rows > 0) {
        // Индекс может закончиться прямо во время снимка. Сохраняем PNG как
        // «строится» только если после захвата кадра окно ещё на этом этапе.
        const png = Buffer.from(await browser.takeScreenshot(), 'base64');
        const stillBuilding = await browser.execute((expected) =>
          document.documentElement.dataset.theme === expected &&
          (document.querySelector('aside.right-panel .body')?.textContent ?? '').trimStart().startsWith('Индекс строится'), theme);
        if (stillBuilding) {
          expect(distinctBytes(png)).toBeGreaterThanOrEqual(16);
          writeFileSync(path.join(SCREENSHOT_DIR, `${frameName(step, 'building', theme)}-${process.platform}.png`), png);
          captured = true;
        }
      }
    }
    if (state.ready) {
      observed.listMs = Date.now() - started;
      break;
    }
  }
  notice(
    `${step} (${theme}): ${NOTES} заметок генератора; окно хранилища через ${observed.shellMs} мс, список через ${observed.listMs} мс; ` +
      `«строится» видно: ${observed.building.length > 0 ? observed.building.join(' → ') : 'нет (сборка закончилась раньше первого опроса)'}; строк дерева во время «строится»: ${observed.treeWhileBuilding}; панель до списка: ${observed.states.map((t) => `«${t.slice(0, 60)}»`).join(' → ')}`,
  );
  if (process.platform === 'win32') expect(captured, `нет снимка построения для ${step} (${theme})`).toBe(true);
  return observed;
}

function treeRows(browser: WebdriverIO.Browser): Promise<number> {
  return browser.execute(() => document.querySelectorAll('[role="treeitem"]').length);
}

interface Snapshot {
  backlinks: string[];
  tags: string[];
}

/** Что показывают оба вида после сборки: группы обратных ссылок и все строки тегов. */
async function snapshot(browser: WebdriverIO.Browser, step: Opening, theme: Theme): Promise<Snapshot> {
  expect(await browser.execute(() => document.documentElement.dataset.theme)).toBe(theme);
  const groups = await backlinkGroups(browser);
  await saveScreenshot(browser, frameName(step, 'backlinks', theme));
  await showView(browser, 'Теги');
  await browser.waitUntil(async () => (await tagRows(browser)).length > 0, { timeout: 30_000, timeoutMsg: 'список тегов пуст' });
  const tags = (await tagRows(browser)).map((r) => r.label);
  await saveScreenshot(browser, frameName(step, 'tags', theme));
  await showView(browser, 'Обратные ссылки');
  return { backlinks: groups.map((g) => `${g.name}: ${g.lines.join(' | ')}`), tags };
}

let parent = '';
let vault = '';

beforeAll(() => {
  parent = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-e2e-kt1-'));
  vault = path.join(parent, 'Генератор');
  const ms = generateVault(vault, NOTES, 7);
  notice(`генератор: ${NOTES} заметок за ${ms} мс`);
  writeFileSync(path.join(vault, `${TARGET}.md`), `# ${TARGET}\n\nЦель для проверки индекса.\n`);
  writeFileSync(path.join(vault, `${SOURCE}.md`), `Ссылка на [[${TARGET}]] и тег #кт1-проверка\n`);
}, 180_000);

afterAll(() => {
  if (parent) rmSync(parent, { recursive: true, force: true });
});

describe.each(['light', 'dark'] as const)('индекс: первое открытие и пересборка без index.db (%s)', (theme) => {
  let session: Session | undefined;
  let profile = '';
  let first: Snapshot | undefined;

  beforeAll(() => {
    // Раскладка: открыта заметка-цель, правая панель — «Обратные ссылки» (модель раскладки v1, src/core/layout/layout.ts).
    mkdirSync(path.join(vault, '.rhizomind'), { recursive: true });
    rmSync(path.join(vault, '.rhizomind', 'index.db'), { force: true });
    writeFileSync(path.join(vault, '.rhizomind', 'settings.json'), JSON.stringify({ format_version: 1, theme }));
    writeFileSync(
      path.join(vault, '.rhizomind', 'workspace.json'),
      JSON.stringify({
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path: [`${TARGET}.md`] }], active: 0 },
        active_pane: 'pane-1',
        side_panel: { open: true, view: 'backlinks', width: 300, tag_order: 'name' },
      }),
    );
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  it('первое открытие: дерево сразу, «Индекс строится… N %», потом список', async () => {
    profile = createTestProfile();
    session = await startApp({ pickFolder: vault, profile });
    const b = session.browser;
    const seen = await openFromManager(b, 'first-open', theme);
    for (const text of seen.building) expect(text).toMatch(/^Индекс строится…( \d{1,3} %)?$/);
    // Дерево не ждёт индекса (С2-3.7): пока панель показывала «строится», дерево уже было.
    if (seen.building.length > 0) expect(seen.treeWhileBuilding, 'строк дерева во время сборки').toBeGreaterThan(0);
    await b.waitUntil(async () => (await treeRows(b)) > 0, { timeout: 20_000, timeoutMsg: 'дерево пусто' });
    expect((await backlinkGroups(b)).map((g) => g.name)).toEqual([SOURCE]);
    first = await snapshot(b, 'first-open', theme);
    expect(first.tags).toContain(OWN_TAG);
    notice(`первое открытие: тегов ${first.tags.length}`);
  }, 300_000);

  it('индекс записан в .rhizomind/index.db', async () => {
    const file = path.join(vault, '.rhizomind', 'index.db');
    const deadline = Date.now() + INDEX_WRITE_MS;
    while (!existsSync(file) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 500));
    expect(existsSync(file), 'index.db не записан').toBe(true);
  }, INDEX_WRITE_MS + 10_000);

  it('закрыть, удалить index.db, открыть — обратные ссылки и теги те же', async () => {
    if (!first) throw new Error('нет результата первого открытия');
    await session?.stop();
    session = undefined;
    expect(await waitForAppExit(15_000)).toBeGreaterThanOrEqual(0);
    rmSync(profile, { recursive: true, force: true });
    const file = path.join(vault, '.rhizomind', 'index.db');
    rmSync(file);
    expect(existsSync(file)).toBe(false);

    profile = createTestProfile();
    session = await startApp({ pickFolder: vault, profile });
    const b = session.browser;
    const seen = await openFromManager(b, 'reopen', theme);
    for (const text of seen.building) expect(text).toMatch(/^Индекс строится…( \d{1,3} %)?$/);
    const again = await snapshot(b, 'reopen', theme);
    expect(again).toEqual(first);
    // Индекс собран с нуля и записан снова.
    const deadline = Date.now() + INDEX_WRITE_MS;
    while (!existsSync(file) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 500));
    expect(existsSync(file), 'index.db не записан заново').toBe(true);
  }, 400_000);
});
