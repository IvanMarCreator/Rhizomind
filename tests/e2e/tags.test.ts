// Теги на настоящем ядре (задача 0.2-8.3, С2-3.3; КТ1, п. 4): `#проект`,
// `#Проект` и `#работа/отчёт` в тексте и в блоке свойств; вид «Теги» —
// плоский список с числом заметок (одно написание на тег — самое частое,
// `#работа/отчёт` — самостоятельный тег, `#работа` не появляется); щелчок по
// тегу раскрывает список заметок; щелчок по плашке тега в тексте и по тегу в
// строке «Свойства» открывает вид «Теги» с раскрытым тегом. Не теги (код,
// ссылка на заголовок, адрес, число, `\#`) в список не попадают.
// Снимки — на каждом шаге в светлой и тёмной теме (`tags-NN-…`).
import { rmSync } from 'node:fs';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { clickTag, openFromTree, panelHeading, panelOpen, showView, tagRows, TEXT, themeShots } from './kt1.ts';

const shot = themeShots('tags');

const PROJECT = '#проект, заметок: 5';
const REPORT = '#работа/отчёт, заметок: 2';
const IDEA = '#идея, заметок: 1';

/** Значения плашек тегов в тексте открытой заметки. */
function plaques(browser: WebdriverIO.Browser): Promise<string[]> {
  return browser.execute(
    (selector) => [...document.querySelectorAll(`${selector} .cm-rz-tag`)].map((el) => el.getAttribute('data-rz-tag') ?? ''),
    TEXT,
  );
}

/** Закрыть правую панель (кнопка справа в строке над заметкой). */
async function closePanel(browser: WebdriverIO.Browser): Promise<void> {
  if (await panelOpen(browser)) await browser.$('button.panel-toggle').click();
  await browser.waitUntil(async () => !(await panelOpen(browser)), { timeout: 5_000, timeoutMsg: 'панель не закрылась' });
}

describe('теги', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    vault = createTestVault('Теги', {
      'А.md': 'Первая.\n\nПро #проект и #работа/отчёт.\n',
      'Б.md': 'Текст без тегов в начале.\n\nЗаметка про #Проект\n',
      'В.md': '---\ntags: [проект, идея]\n---\nТекст с #работа/отчёт\n',
      'Г.md': '---\ntags:\n  - "#Проект"\n---\nТолько свойства.\n',
      'Д.md': '#проект в начале строки.\n',
      'Не теги.md': '`#код` и [[А#Первая]] и https://site/#x и #2026 и \\#экран\n\n# Заголовок\n',
    });
    profile = createTestProfile();
    session = await startApp({ vault, profile });
    await openFromTree(session.browser, 'А');
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (vault) removeTestVault(vault);
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  const browser = () => {
    if (!session) throw new Error('нет сессии');
    return session.browser;
  };

  it('плашки тегов в тексте; не теги без плашек', async () => {
    const b = browser();
    await b.waitUntil(async () => (await plaques(b)).length === 2, { timeout: 5_000, timeoutMsg: 'нет плашек в «А»' });
    expect(await plaques(b)).toEqual(['проект', 'работа/отчёт']);
    await shot(b, 'plaques-in-text');
    await openFromTree(b, 'Не теги');
    expect(await plaques(b)).toEqual([]);
    await shot(b, 'not-tags');
  });

  it('вид «Теги»: плоский список с числом заметок, одно написание на тег', async () => {
    const b = browser();
    await openFromTree(b, 'А');
    await showView(b, 'Теги');
    await b.waitUntil(async () => (await tagRows(b)).length > 0, { timeout: 15_000, timeoutMsg: 'список тегов пуст' });
    const rows = await tagRows(b);
    // Естественный алфавитный порядок; `#работа` отдельно не появляется (решение 113).
    expect(rows.map((r) => r.label)).toEqual([IDEA, PROJECT, REPORT]);
    expect(rows.every((r) => !r.expanded)).toBe(true);
    expect(await panelHeading(b)).toBe('Теги 3');
    await shot(b, 'tags-view');
  });

  it('щелчок по тегу раскрывает список заметок; повторный — сворачивает', async () => {
    const b = browser();
    await clickTag(b, PROJECT);
    await b.waitUntil(async () => ((await tagRows(b)).find((r) => r.label === PROJECT)?.notes.length ?? 0) === 5, {
      timeout: 5_000,
      timeoutMsg: 'не раскрылся список заметок #проект',
    });
    const project = (await tagRows(b)).find((r) => r.label === PROJECT);
    expect(project?.expanded).toBe(true);
    expect(project?.notes).toEqual(['А', 'Б', 'В', 'Г', 'Д']);
    await shot(b, 'tag-expanded');
    await clickTag(b, REPORT);
    await b.waitUntil(async () => ((await tagRows(b)).find((r) => r.label === REPORT)?.notes.length ?? 0) === 2, {
      timeout: 5_000,
      timeoutMsg: 'не раскрылся #работа/отчёт',
    });
    expect((await tagRows(b)).find((r) => r.label === REPORT)?.notes).toEqual(['А', 'В']);
    await shot(b, 'two-tags-expanded');
    await clickTag(b, PROJECT);
    await clickTag(b, REPORT);
    await b.waitUntil(async () => (await tagRows(b)).every((r) => !r.expanded), { timeout: 5_000, timeoutMsg: 'теги не свернулись' });
  });

  it('щелчок по заметке в списке тега открывает её', async () => {
    const b = browser();
    await clickTag(b, IDEA);
    await b.$('.tags .notes .note-row').waitForDisplayed({ timeout: 5_000 });
    await b.$('.tags .notes .note-row').click();
    await b.waitUntil(() => b.execute(() => document.querySelector('nav[aria-label="Путь к заметке"]')?.textContent?.trim() === 'В'), {
      timeout: 5_000,
      timeoutMsg: 'заметка «В» не открылась',
    });
    await shot(b, 'note-from-tag');
    await clickTag(b, IDEA);
  });

  it('щелчок по плашке тега в тексте открывает вид «Теги» с раскрытым тегом', async () => {
    const b = browser();
    await showView(b, 'Обратные ссылки');
    await closePanel(b);
    await openFromTree(b, 'Б');
    await shot(b, 'before-plaque-click');
    // Курсор в начале заметки — плашка далеко от него, обычный щелчок открывает тег.
    await b.$(`${TEXT} [data-rz-tag="Проект"]`).click();
    await b.waitUntil(async () => (await panelOpen(b)) && (await panelHeading(b)).startsWith('Теги'), {
      timeout: 5_000,
      timeoutMsg: 'вид «Теги» не открылся',
    });
    await b.waitUntil(async () => (await tagRows(b)).find((r) => r.label === PROJECT)?.expanded === true, {
      timeout: 5_000,
      timeoutMsg: 'тег #проект не раскрыт',
    });
    expect((await tagRows(b)).filter((r) => r.expanded).map((r) => r.label)).toEqual([PROJECT]);
    await shot(b, 'plaque-click-opens-tag');
  });

  it('щелчок по тегу в строке «Свойства» открывает вид «Теги» с раскрытым тегом', async () => {
    const b = browser();
    await clickTag(b, PROJECT);
    await showView(b, 'Обратные ссылки');
    await closePanel(b);
    await openFromTree(b, 'В');
    await b.$(`${TEXT} [data-rz-properties-tag="идея"]`).click();
    await b.waitUntil(async () => (await tagRows(b)).find((r) => r.label === IDEA)?.expanded === true, {
      timeout: 5_000,
      timeoutMsg: 'тег #идея не раскрыт из строки «Свойства»',
    });
    expect(await panelHeading(b)).toBe('Теги 3');
    await shot(b, 'properties-tag-click');
  });
});
