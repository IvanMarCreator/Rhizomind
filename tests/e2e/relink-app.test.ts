// Переименование с обновлением ссылок в приложении на настоящем ядре (задача
// 0.2-5.2; С2-3.5 «в приложении», «Открытые заметки», «Ctrl+Z»; Р9; КТ2):
// «узел» со 100 ссылающимися заметками (LF, CRLF, BOM, концевые пробелы,
// псевдоним, заголовок, встраивание; ссылки в коде и в блоке свойств — не
// ссылки) переименовывается из дерева в режиме «Всегда спрашивать» — окно
// вопроса, «Обновить ссылки», уведомление; все файлы проверены побайтно. В
// открытой заметке перед этим набран текст — он не теряется (инвариант 4).
// Ctrl+Z возвращает имя и ссылки без второго окна — файлы снова побайтно как
// были. Снимки — в светлой и тёмной теме (`relink-app-NN-…`).
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { cursorTo, editorLines, openFromTree, TEXT, themeShots } from './kt1.ts';

const OLD = 'Узел';
const NEW = 'Хаб';
const REFERRERS = 100;
const TYPED = ' Набранный текст.';
const OPEN_NOTE = 'Открытая';

/**
 * Заметка номер `i`, ссылающаяся на `target`. Ссылки в блоке кода и в блоке
 * свойств ведут на «Узел» всегда: это не ссылки, их правка запрещена.
 */
function referrer(i: number, target: string): string {
  switch (i % 6) {
    case 0:
      return `Ссылка на [[${target}]].\n`;
    case 1:
      return `Строка ${i}\r\nСм. [[${target}|узел]] и ещё раз [[${target}#Раздел]].\r\nКонец\r\n`;
    case 2:
      return `\uFEFF# Заметка ${i}\n\n![[${target}]]\n`;
    case 3:
      return `\`\`\`\n[[${OLD}]] в коде\n\`\`\`\nПосле кода [[${target}]]   \n\t\n`;
    case 4:
      return `---\nсвязь: "[[${OLD}]]"\ntags: [узел]\n---\nТекст [[${target}#Раздел|раздел]]\n`;
    default:
      return `[[${target}]] и снова [[${target}]] без перевода строки`;
  }
}

const referrerName = (i: number) => `Ссылки/Заметка ${String(i).padStart(3, '0')}.md`;
const HUB_TEXT = `# ${OLD}\n\n## Раздел\n\nТекст узла.\n`;
const openText = (target: string, typed: string) => `${OPEN_NOTE} ссылается на [[${target}]].${typed}\n`;

/** Ожидаемое содержимое всех заметок, кроме «узла»: имя → текст. */
function expected(target: string, typed: string): Record<string, string> {
  const files: Record<string, string> = { [`${OPEN_NOTE}.md`]: openText(target, typed) };
  for (let i = 1; i <= REFERRERS; i += 1) files[referrerName(i)] = referrer(i, target);
  files['Без ссылок.md'] = `Здесь нет ссылок на ${OLD}.\n`;
  return files;
}

const shot = themeShots('relink-app');

/** Файлы побайтно совпали с ожидаемыми; иначе — первые расхождения. */
function mismatches(vault: string, files: Record<string, string>): string[] {
  const wrong: string[] = [];
  for (const [name, text] of Object.entries(files)) {
    const file = path.join(vault, ...name.split('/'));
    let actual: Buffer;
    try {
      actual = readFileSync(file);
    } catch {
      wrong.push(`${name}: нет файла`);
      continue;
    }
    if (!actual.equals(Buffer.from(text, 'utf8'))) wrong.push(`${name}: ${JSON.stringify(actual.toString('utf8'))}`);
  }
  return wrong;
}

async function waitForFiles(vault: string, files: Record<string, string>, what: string, timeout = 10_000): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const wrong = mismatches(vault, files);
    if (wrong.length === 0) return;
    if (Date.now() > deadline) throw new Error(`${what}: ${wrong.length} файлов не совпали, например ${wrong.slice(0, 3).join('; ')}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

const dialogTitle = (browser: WebdriverIO.Browser) =>
  browser.execute(() => document.querySelector('[data-testid="rename-dialog"] h2')?.textContent?.trim() ?? null);

const noticeTexts = (browser: WebdriverIO.Browser) =>
  browser.execute(() => [...document.querySelectorAll('.notice')].map((notice) => (notice.textContent ?? '').replace(/\s+/g, ' ').trim()));

describe('переименование «узла» со 100 ссылками в приложении', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    const notes: Record<string, string> = { [`${OLD}.md`]: HUB_TEXT, ...expected(OLD, '') };
    vault = createTestVault('Переименование', {});
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

  it('«Всегда спрашивать»: окно до переименования, «Обновить ссылки» — все ссылки переписаны побайтно, набранный текст сохранён', async () => {
    const b = browser();
    // Открытая заметка с набранным, ещё не записанным текстом (инвариант 4).
    await openFromTree(b, OPEN_NOTE);
    await cursorTo(b, `${OPEN_NOTE} ссылается`);
    await b.keys(TYPED);

    // Переименование «узла» из меню дерева: правый щелчок только выбирает строку,
    // открытой остаётся заметка с набранным текстом.
    const hub = b.$(`[role="treeitem"][aria-label="${OLD}"]`);
    await hub.click({ button: 'right' });
    await b.$('[role="menuitem"]*=Переименовать').click();
    const field = b.$('input[aria-label="Новое имя"]');
    await field.waitForDisplayed();
    await b.keys(['Control', 'a']);
    await b.keys(NEW);
    await b.keys('Enter');

    // Пока индекс строится — окно ожидания; потом — вопрос (решение 114).
    const question = `На «${OLD}» ссылается ${REFERRERS + 1} заметка. Обновить ссылки?`;
    await b.waitUntil(async () => (await dialogTitle(b)) === question, {
      timeout: 60_000,
      interval: 100,
      timeoutMsg: 'нет окна вопроса',
    });
    // До ответа ничего не переименовано и не переписано.
    expect(existsSync(path.join(vault, `${OLD}.md`))).toBe(true);
    const untouched = Object.fromEntries(Object.entries(expected(OLD, '')).filter(([name]) => name !== `${OPEN_NOTE}.md`));
    expect(mismatches(vault, untouched)).toEqual([]);
    // «Обновить ссылки» — кнопка по умолчанию (фокус).
    expect(await b.execute(() => document.activeElement?.textContent?.trim())).toBe('Обновить ссылки');
    await shot(b, 'question');

    const started = Date.now();
    await b.keys('Enter');
    await b.waitUntil(async () => (await noticeTexts(b)).some((text) => text.includes(`Ссылки обновлены в ${REFERRERS + 1} заметке`)), {
      timeout: 30_000,
      interval: 50,
      timeoutMsg: 'нет уведомления «Ссылки обновлены…»',
    });
    console.log(`[relink-app] от ответа до уведомления ${Date.now() - started} мс`);
    expect(await dialogTitle(b)).toBeNull();

    await waitForFiles(vault, expected(NEW, TYPED), 'после переименования');
    expect(readFileSync(path.join(vault, `${NEW}.md`)).equals(Buffer.from(HUB_TEXT, 'utf8'))).toBe(true);
    expect(existsSync(path.join(vault, `${OLD}.md`))).toBe(false);
    // Открытая заметка осталась открытой, набранный текст на месте.
    await b.waitUntil(async () => (await editorLines(b)).some((line) => line.includes(TYPED.trim())), {
      timeout: 5_000,
      timeoutMsg: 'набранный текст пропал из редактора',
    });
    expect(await b.$(TEXT).isExisting()).toBe(true);
    await shot(b, 'updated');
  });

  it('Ctrl+Z — прежнее имя и ссылки без второго окна; файлы снова побайтно как были', async () => {
    const b = browser();
    // Фокус в дерево на «Хаб» (щелчок открывает заметку, это не мешает).
    await b.$(`[role="treeitem"][aria-label="${NEW}"]`).click();
    await b.keys(['Control', 'z']);

    await b.waitUntil(async () => existsSync(path.join(vault, `${OLD}.md`)), { timeout: 30_000, timeoutMsg: 'имя не вернулось' });
    expect(await dialogTitle(b)).toBeNull();
    await waitForFiles(vault, expected(OLD, TYPED), 'после Ctrl+Z', 30_000);
    expect(readFileSync(path.join(vault, `${OLD}.md`)).equals(Buffer.from(HUB_TEXT, 'utf8'))).toBe(true);
    expect(existsSync(path.join(vault, `${NEW}.md`))).toBe(false);
    await shot(b, 'undone');
  });
});
