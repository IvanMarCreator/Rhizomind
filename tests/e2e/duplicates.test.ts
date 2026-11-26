// С2-3.6, решение 111: появление одноимённых файлов при открытом хранилище.
import { copyFileSync, mkdirSync, readdirSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { headerPath, openFromTree, panelHeading, showView, TEXT, themeShots } from './kt1.ts';

const shot = themeShots('duplicates');
const pending = '.notice[data-reason="duplicate"]';
const singleSources = Array.from({ length: 20 }, (_, i) => `Дневник ${i + 1}`);
const batchSources = Array.from({ length: 21 }, (_, i) => i + 1);

function diary(target: string): string {
  return `[[Начало]]\r\n[[${target}]] и ![[${target}#Решение|итоги]].  \r\n`;
}

function project(i: number, plan: string): string {
  return `[[Начало]]\n[[Отчёт ${i}]] и [[${plan}#Этап|план]].\n`;
}

/** Папки образца плоские. В Node 22 на Windows cpSync падает, если путь содержит кириллицу. */
function copyNotesFolder(vault: string, from: string, to: string): void {
  const source = path.join(vault, from);
  const destination = path.join(vault, to);
  mkdirSync(destination);
  for (const file of readdirSync(source)) copyFileSync(path.join(source, file), path.join(destination, file));
}

async function waitForSuccess(browser: WebdriverIO.Browser, text: string): Promise<void> {
  await browser.waitUntil(
    () => browser.execute(
      (message) => [...document.querySelectorAll('.notice[data-tone="success"] .text')].some((p) => p.textContent === message),
      text,
    ),
    { timeout: 30_000, timeoutMsg: `Нет сообщения «${text}»` },
  );
}

async function answerBatch(browser: WebdriverIO.Browser, vault: string, apply: boolean): Promise<void> {
  const question = browser.$(pending);
  await question.waitForDisplayed({ timeout: 30_000 });
  expect(await browser.$$(pending).length).toBe(1);
  expect(await question.$('.text').getText()).toBe('Появились повторы имён: уточнить ссылки в 21 заметке?');
  expect(await browser.$('[role="dialog"]').isExisting()).toBe(false);
  const buttons = await browser.execute(
    (selector) => [...document.querySelectorAll(`${selector} button`)].map((button) => button.textContent?.trim()),
    pending,
  );
  expect(buttons).toEqual(['Уточнить', 'Не трогать']);
  await shot(browser, apply ? 'before-clarify' : 'before-skip');
  assertProjects(vault, false);
  await question.$(apply ? 'button.primary' : 'button:not(.primary)').click();
  await question.waitForExist({ reverse: true, timeout: 15_000 });
}

function assertProjects(vault: string, clarified: boolean): void {
  for (const i of batchSources) {
    const file = `Проект ${i}.md`;
    const target = clarified ? `Планы/План ${i}` : `План ${i}`;
    expect(readFileSync(path.join(vault, file)), file).toEqual(Buffer.from(project(i, target)));
  }
}

function assertDiaries(vault: string, target: string): void {
  for (const name of singleSources) {
    expect(readFileSync(path.join(vault, `${name}.md`)), name).toEqual(Buffer.from(diary(target)));
  }
}

function diaryVersions(vault: string) {
  return singleSources.map((name) => {
    const { ino, mtimeNs } = statSync(path.join(vault, `${name}.md`), { bigint: true });
    return { name, ino, mtimeNs };
  });
}

describe('уточнение ссылок при повторе имени', () => {
  let session: Session;
  let vault = '';

  beforeAll(async () => {
    vault = createTestVault('Повторы имён', { 'Начало.md': '# Начало\n', 'Проверка.md': '[[Свидетель]]\n' });
    for (const dir of ['События', 'Копии', 'Отчёты', 'Планы']) mkdirSync(path.join(vault, dir));
    writeFileSync(path.join(vault, 'События', 'Встреча.md'), '## Решение\n');
    writeFileSync(path.join(vault, 'События', 'Свидетель.md'), 'Исходная заметка.\n');
    for (const name of singleSources) writeFileSync(path.join(vault, `${name}.md`), diary('Встреча'));
    for (const i of batchSources) {
      writeFileSync(path.join(vault, 'Планы', `План ${i}.md`), '## Этап\n');
      writeFileSync(path.join(vault, 'Отчёты', `Отчёт ${i}.md`), 'Отчёт.\n');
      writeFileSync(path.join(vault, `Проект ${i}.md`), project(i, `План ${i}`));
    }
    session = await startApp({ vault });
    const b = session.browser;
    await openFromTree(b, 'Начало');
    await showView(b, 'Обратные ссылки');
    // Каждая из 41 ссылающихся заметок уже в индексе до первой внешней операции.
    await b.waitUntil(async () => (await panelHeading(b)) === 'Обратные ссылки 41', { timeout: 20_000 });
    await shot(b, 'originals');
  });

  afterAll(async () => {
    try {
      await session?.stop();
    } finally {
      if (vault) removeTestVault(vault);
    }
  });

  it('один повтор, 20 ссылающихся заметок: автоматическое уточнение, после удаления копии пути сохраняются', async () => {
    const b = session.browser;
    await openFromTree(b, 'Дневник 1');
    const copy = path.join(vault, 'Копии', 'Встреча.md');
    copyFileSync(path.join(vault, 'События', 'Встреча.md'), copy);
    await waitForSuccess(b, 'Ссылки уточнены в 20 заметках');
    expect(await b.$(pending).isExisting()).toBe(false);
    assertDiaries(vault, 'События/Встреча');
    await shot(b, 'automatic');

    const copies = '[role="treeitem"][aria-label="Копии"]';
    await b.$(copies).click();
    // Дерево плоское: сразу за раскрытой папкой — её единственная дочерняя строка.
    const child = `${copies}[aria-expanded="true"] + [role="treeitem"][aria-level="2"]`;
    await b.$(`${child}[aria-label="Встреча"]`).waitForExist();
    const versions = diaryVersions(vault);
    unlinkSync(copy);
    await b.$(`${child}[aria-label="Встреча"]`).waitForExist({ reverse: true, timeout: 10_000 });
    // После удаления ждём ещё один полный цикл уточнения через независимую ссылку.
    copyFileSync(path.join(vault, 'События', 'Свидетель.md'), path.join(vault, 'Копии', 'Свидетель.md'));
    await waitForSuccess(b, 'Ссылки уточнены в 1 заметке');
    await b.$(`${child}[aria-label="Свидетель"]`).waitForExist();
    expect(readFileSync(path.join(vault, 'Проверка.md'), 'utf8')).toBe('[[События/Свидетель]]\n');
    assertDiaries(vault, 'События/Встреча');
    expect(diaryVersions(vault)).toEqual(versions);
    await shot(b, 'removed-copy');
  });

  it('копирование папки, 21 ссылающаяся заметка: «Не трогать» и «Уточнить»', async () => {
    const b = session.browser;
    await openFromTree(b, 'Проект 1');
    copyNotesFolder(vault, 'Отчёты', 'Копии отчётов');
    await answerBatch(b, vault, false);
    assertProjects(vault, false);
    await b.$(`${TEXT} [data-rz-wiki="Отчёт 1"]`).click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Отчёты / Отчёт 1', { timeout: 5_000 });
    await shot(b, 'skipped');

    await openFromTree(b, 'Проект 1');
    copyNotesFolder(vault, 'Планы', 'Копии планов');
    await answerBatch(b, vault, true);
    await waitForSuccess(b, 'Ссылки уточнены в 21 заметке');
    // После следующей обработанной пачки отклонённые ссылки на отчёты всё ещё короткие.
    assertProjects(vault, true);
    await b.$(`${TEXT} [data-rz-wiki="Планы/План 1#Этап|план"]`).click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Планы / План 1', { timeout: 5_000 });
    await shot(b, 'clarified');
  }, 120_000);
});
