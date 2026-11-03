// Индекс хранилища на настоящем ядре (задача 0.2-8.3, С2-3.7, С2-4.1; КТ1,
// пп. 1 и 6): хранилище из генератора (tools/gen-vault) открывается через
// «Открыть папку как хранилище» в работающей программе — дерево сразу, в
// правой панели «Индекс строится… N %», потом список обратных ссылок и
// тегов. Затем программа закрывается, `.rhizomind/index.json` удаляется, и
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
import { createTestProfile, startApp, waitForAppExit, type Session } from './driver.ts';
import { backlinkGroups, generateVault, panelBody, showView, tagRows, themeShots } from './kt1.ts';

const shot = themeShots('index');

const SHELL = '[data-testid="shell"]';
const OPEN_FOLDER = 'button[aria-label="Открыть папку как хранилище"]';
/** Заметок генератора: на Linux сборка быстрее — хранилище больше (предел требований — 50 000). */
const NOTES = Number(process.env.RHIZOMIND_E2E_KT1_NOTES ?? (process.platform === 'linux' ? 50_000 : 10_000));
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
  /** Через сколько мс после «Открыть папку…» появилось окно хранилища и список. */
  shellMs: number;
  listMs: number;
}

/**
 * Окно управления → «Открыть папку как хранилище» (ответ окна выбора папки —
 * `pickFolder`) → окно хранилища. Пока нет списка обратных ссылок, панель
 * опрашивается без пауз; первое «Индекс строится…» снимается в обеих темах.
 */
async function openFromManager(browser: WebdriverIO.Browser, step: string): Promise<Observed> {
  await browser.$(OPEN_FOLDER).waitForDisplayed({ timeout: 30_000 });
  const started = Date.now();
  await browser.$(OPEN_FOLDER).click();
  const observed: Observed = { building: [], states: [], shellMs: -1, listMs: -1 };
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
    const body = await panelBody(browser).catch(() => '');
    if (observed.states.at(-1) !== body) observed.states.push(body);
    if (body.startsWith('Индекс строится')) {
      if (observed.building.at(-1) !== body) observed.building.push(body);
      if (!captured) {
        captured = true;
        await saveBuilding(browser, step);
      }
    }
    if ((await backlinkGroups(browser)).length > 0) {
      observed.listMs = Date.now() - started;
      break;
    }
  }
  notice(
    `${step}: ${NOTES} заметок генератора; окно хранилища через ${observed.shellMs} мс, список через ${observed.listMs} мс; ` +
      `«строится» видно: ${observed.building.length > 0 ? observed.building.join(' → ') : 'нет (сборка закончилась раньше первого опроса)'}; панель до списка: ${observed.states.map((t) => `«${t.slice(0, 60)}»`).join(' → ')}`,
  );
  return observed;
}

async function saveBuilding(browser: WebdriverIO.Browser, step: string): Promise<void> {
  await shot(browser, `${step}-building`);
}

interface Snapshot {
  backlinks: string[];
  tags: string[];
}

/** Что показывают оба вида после сборки: группы обратных ссылок и все строки тегов. */
async function snapshot(browser: WebdriverIO.Browser, step: string): Promise<Snapshot> {
  const groups = await backlinkGroups(browser);
  await shot(browser, `${step}-backlinks`);
  await showView(browser, 'Теги');
  await browser.waitUntil(async () => (await tagRows(browser)).length > 0, { timeout: 30_000, timeoutMsg: 'список тегов пуст' });
  const tags = (await tagRows(browser)).map((r) => r.label);
  await shot(browser, `${step}-tags`);
  await showView(browser, 'Обратные ссылки');
  return { backlinks: groups.map((g) => `${g.name}: ${g.lines.join(' | ')}`), tags };
}

describe('индекс: первое открытие и пересборка без index.json', () => {
  let session: Session | undefined;
  let parent = '';
  let vault = '';
  let profile = '';
  let first: Snapshot | undefined;

  beforeAll(() => {
    parent = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-e2e-kt1-'));
    vault = path.join(parent, 'Генератор');
    const ms = generateVault(vault, NOTES, 7);
    notice(`генератор: ${NOTES} заметок за ${ms} мс`);
    writeFileSync(path.join(vault, `${TARGET}.md`), `# ${TARGET}\n\nЦель для проверки индекса.\n`);
    writeFileSync(path.join(vault, `${SOURCE}.md`), `Ссылка на [[${TARGET}]] и тег #кт1-проверка\n`);
    // Раскладка: открыта заметка-цель, правая панель — «Обратные ссылки» (модель раскладки v1, src/core/layout/layout.ts).
    mkdirSync(path.join(vault, '.rhizomind'), { recursive: true });
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
  }, 180_000);

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (profile) rmSync(profile, { recursive: true, force: true });
    if (parent) rmSync(parent, { recursive: true, force: true });
  });

  it('первое открытие: дерево сразу, «Индекс строится… N %», потом список', async () => {
    profile = createTestProfile();
    session = await startApp({ pickFolder: vault, profile });
    const b = session.browser;
    const seen = await openFromManager(b, 'first-open');
    for (const text of seen.building) expect(text).toMatch(/^Индекс строится…( \d{1,3} %)?$/);
    // Дерево показано, не дожидаясь индекса (С2-3.7).
    expect(await b.execute(() => document.querySelectorAll('[role="treeitem"]').length)).toBeGreaterThan(0);
    expect((await backlinkGroups(b)).map((g) => g.name)).toEqual([SOURCE]);
    first = await snapshot(b, 'first-open');
    expect(first.tags).toContain(OWN_TAG);
    notice(`первое открытие: тегов ${first.tags.length}`);
  }, 300_000);

  it('индекс записан в .rhizomind/index.json', async () => {
    const file = path.join(vault, '.rhizomind', 'index.json');
    const deadline = Date.now() + INDEX_WRITE_MS;
    while (!existsSync(file) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 500));
    expect(existsSync(file), 'index.json не записан').toBe(true);
  }, INDEX_WRITE_MS + 10_000);

  it('закрыть, удалить index.json, открыть — обратные ссылки и теги те же', async () => {
    if (!first) throw new Error('нет результата первого открытия');
    await session?.stop();
    session = undefined;
    expect(await waitForAppExit(15_000)).toBeGreaterThanOrEqual(0);
    rmSync(profile, { recursive: true, force: true });
    const file = path.join(vault, '.rhizomind', 'index.json');
    rmSync(file);
    expect(existsSync(file)).toBe(false);

    profile = createTestProfile();
    session = await startApp({ pickFolder: vault, profile });
    const b = session.browser;
    const seen = await openFromManager(b, 'reopen');
    for (const text of seen.building) expect(text).toMatch(/^Индекс строится…( \d{1,3} %)?$/);
    const again = await snapshot(b, 'reopen');
    expect(again).toEqual(first);
    // Индекс собран с нуля и записан снова.
    const deadline = Date.now() + INDEX_WRITE_MS;
    while (!existsSync(file) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 500));
    expect(existsSync(file), 'index.json не записан заново').toBe(true);
  }, 400_000);
});
