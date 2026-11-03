// Переименование в Проводнике при открытом приложении на настоящем ядре
// (задача 0.2-6.3; С2-3.5 «в Проводнике», «Открытые заметки»; решение 110;
// инвариант 4): в открытой заметке набран текст, другую заметку, на которую
// она ссылается, переименовывают снаружи (`rename` — то же, что `mv` или
// MoveFileExW в Проводнике). В режиме «Всегда спрашивать» — уведомление
// «нужен ответ» внизу окна, без окна поверх работы; «Обновить ссылки» —
// ссылки переписаны побайтно, набранный текст не потерян. Снимки — в светлой и
// тёмной теме (`relink-external-NN-…`).
//
// Внешние переименования в ядре — задача 0.2-6.1 (ветка
// `task/0.2-6.1-external-renames`); до её слияния ядро в main не присылает
// `links:pending`, поэтому сценарий выключен (`describe.skip`). Включить —
// убрать `.skip` после слияния 0.2-6.1.
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { cursorTo, editorLines, openFromTree, TEXT, themeShots } from './kt1.ts';
import { saveScreenshot } from './screenshot.ts';

const OLD = 'Встреча';
const NEW = 'Планёрка';
const OPEN_NOTE = 'Открытая';
// Без двойных букв подряд: WebKitWebDriver на Linux теряет повтор клавиши.
const TYPED = ' Новая мысль.';

const openText = (target: string, typed: string) => `${OPEN_NOTE} ссылается на [[${target}]].${typed}\n`;

/** Заметки, кроме переименованной: имя → текст (LF, CRLF, псевдоним, заголовок). */
function expected(target: string, typed: string): Record<string, string> {
  return {
    [`${OPEN_NOTE}.md`]: openText(target, typed),
    'Дневник.md': `Обсудили [[${target}]] и [[${target}#Итоги|итоги]].\n`,
    'Проекты/План.md': `Строка\r\nСм. [[${target}|встречу]]\r\n`,
    'Итоги.md': `![[${target}]]\n`,
    'Без ссылок.md': `Здесь нет ссылок на ${OLD}.\n`,
  };
}

const REFERRERS = 4;
const TARGET_TEXT = `# ${OLD}\n\n## Итоги\n\nТекст встречи.\n`;
const shot = themeShots('relink-external');

function mismatches(vault: string, files: Record<string, string>): string[] {
  const wrong: string[] = [];
  for (const [name, text] of Object.entries(files)) {
    const file = path.join(vault, ...name.split('/'));
    const actual = existsSync(file) ? readFileSync(file) : null;
    if (!actual) wrong.push(`${name}: нет файла`);
    else if (!actual.equals(Buffer.from(text, 'utf8'))) wrong.push(`${name}: ${JSON.stringify(actual.toString('utf8'))}`);
  }
  return wrong;
}

async function waitForFiles(vault: string, files: Record<string, string>, what: string, timeout = 15_000): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const wrong = mismatches(vault, files);
    if (wrong.length === 0) return;
    if (Date.now() > deadline) throw new Error(`${what}: ${wrong.length} файлов не совпали: ${wrong.slice(0, 3).join('; ')}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

const noticeTexts = (browser: WebdriverIO.Browser) =>
  browser.execute(() => [...document.querySelectorAll('.notice')].map((notice) => (notice.textContent ?? '').replace(/\s+/g, ' ').trim()));

describe.skip('переименование в Проводнике (mv снаружи) при открытом приложении — после 0.2-6.1', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    vault = createTestVault('Внешнее переименование', {});
    const notes: Record<string, string> = { [`${OLD}.md`]: TARGET_TEXT, ...expected(OLD, '') };
    for (const [name, text] of Object.entries(notes)) {
      const file = path.join(vault, ...name.split('/'));
      mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, text);
    }
    profile = createTestProfile();
    session = await startApp({ vault, profile });
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

  it('«Всегда спрашивать»: уведомление с кнопками, «Обновить ссылки» — ссылки обновлены побайтно, набранный текст сохранён', async () => {
    const b = browser();
    // Открытая заметка с набранным, ещё не записанным текстом (инвариант 4).
    await openFromTree(b, OPEN_NOTE);
    await cursorTo(b, `${OPEN_NOTE} ссылается`);
    await b.keys(TYPED);

    // Переименование снаружи — тот же файл под новым именем.
    renameSync(path.join(vault, `${OLD}.md`), path.join(vault, `${NEW}.md`));

    const question = `Заметка «${OLD}» переименована на диске в «${NEW}», на неё ссылаются ${REFERRERS} заметки`;
    await b
      .waitUntil(async () => (await noticeTexts(b)).some((text) => text.startsWith(question)), { timeout: 45_000, interval: 100 })
      .catch(async () => {
        await saveScreenshot(b, 'relink-external-no-question');
        throw new Error(`нет уведомления «${question}»: ${JSON.stringify(await noticeTexts(b))}`);
      });
    // Без окна поверх работы; до ответа ссылки не тронуты.
    expect(await b.execute(() => document.querySelector('[role="dialog"]') === null)).toBe(true);
    const untouched = Object.fromEntries(Object.entries(expected(OLD, '')).filter(([name]) => name !== `${OPEN_NOTE}.md`));
    expect(mismatches(vault, untouched)).toEqual([]);
    await shot(b, 'pending');

    const update = await b.$('.notice button.primary');
    expect(await update.getText()).toBe('Обновить ссылки');
    await update.click();

    const done = `Заметка «${OLD}» переименована на диске. Ссылки обновлены в ${REFERRERS} заметках`;
    await b.waitUntil(async () => (await noticeTexts(b)).some((text) => text.includes(done)), {
      timeout: 30_000,
      interval: 50,
      timeoutMsg: `нет уведомления «${done}»`,
    });
    expect((await noticeTexts(b)).some((text) => text.startsWith(question))).toBe(false);

    await waitForFiles(vault, expected(NEW, TYPED), 'после ответа');
    expect(readFileSync(path.join(vault, `${NEW}.md`)).equals(Buffer.from(TARGET_TEXT, 'utf8'))).toBe(true);
    // Открытая заметка осталась открытой, набранный текст на месте.
    await b.waitUntil(async () => (await editorLines(b)).some((line) => line.includes(TYPED.trim())), {
      timeout: 5_000,
      timeoutMsg: 'набранный текст пропал из редактора',
    });
    expect(await b.$(TEXT).isExisting()).toBe(true);
    await shot(b, 'updated');
  });
});
