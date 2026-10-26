// Живой предпросмотр и режим исходного текста (задача 3.1 по 2.5; С3.3,
// решение 72, макет Э13). Заметка со всеми элементами С3.3 на настоящем ядре:
// снимки в светлой и тёмной теме — курсор вне разметки, курсор на строке с
// разметкой, режим исходного текста (Ctrl+E); переключение режима не меняет
// файл; чекбокс щелчком; выбранный режим помнится после перезапуска.
import { readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, waitForAppExit, type Session } from './driver.ts';
import { saveScreenshot } from './screenshot.ts';

const TEXT = '[role="textbox"][aria-label="Текст заметки"]';
const NOTE = 'Образец';

const SAMPLE = [
  '# Заголовок 1',
  '## Заголовок 2',
  '### Заголовок 3',
  '#### Заголовок 4',
  '##### Заголовок 5',
  '###### Заголовок 6',
  '',
  'Текст с **жирным**, *курсивом*, ~~зачёркнутым~~ и `кодом в строке`.',
  '',
  '> Цитата первого уровня',
  '> > Вложенная цитата',
  '',
  '- Пункт списка',
  '- Второй пункт',
  '  - Вложенный пункт',
  '',
  '1. Первый',
  '2. Второй',
  '',
  '- [ ] Открытая задача',
  '- [x] Сделанная задача',
  '',
  '---',
  '',
  '```',
  'function ответ() {',
  '    return 42;',
  '}',
  '```',
  '',
  'Ссылка: [портал регламентов](https://example.com/reglament).',
  '',
  'Последняя строка.',
  '',
].join('\n');

let step = 0;

async function shotThemes(browser: WebdriverIO.Browser, name: string): Promise<void> {
  step += 1;
  await browser.execute(async () => {
    await document.fonts.ready;
  });
  for (const theme of ['light', 'dark'] as const) {
    await browser.execute((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    await saveScreenshot(browser, `preview-${String(step).padStart(2, '0')}-${name}-${theme}`);
  }
  await browser.execute(() => {
    delete document.documentElement.dataset.theme;
  });
}

/** Видимый текст строки редактора, где есть `fragment` (скрытая разметка в DOM не попадает). */
function lineText(browser: WebdriverIO.Browser, fragment: string): Promise<string> {
  return browser.execute(
    (selector, wanted) =>
      [...document.querySelectorAll(`${selector} .cm-line`)]
        .map((line) => line.textContent ?? '')
        .find((text) => text.includes(wanted)) ?? `нет строки «${wanted}»`,
    TEXT,
    fragment,
  );
}

/** Сколько элементов с классом `cls` внутри текста заметки. */
function count(browser: WebdriverIO.Browser, cls: string): Promise<number> {
  return browser.execute((selector, name) => document.querySelectorAll(`${selector} .${name}`).length, TEXT, cls);
}

/** Поставить курсор в конец строки, где есть `fragment`. */
async function cursorTo(browser: WebdriverIO.Browser, fragment: string): Promise<void> {
  const index = await browser.execute(
    (selector, wanted) =>
      [...document.querySelectorAll(`${selector} .cm-line`)].findIndex((line) =>
        (line.textContent ?? '').includes(wanted),
      ),
    TEXT,
    fragment,
  );
  expect(index, `строка «${fragment}»`).toBeGreaterThanOrEqual(0);
  await (await browser.$$(`${TEXT} .cm-line`))[index]?.click();
  await browser.keys('End');
}

function modeChecked(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(
    () => document.querySelector('[role="radiogroup"] [role="radio"][aria-checked="true"]')?.getAttribute('aria-label') ?? '',
  );
}

/**
 * Отступы и рамка фокуса текста (находки Н7, Н3; задача 2.10). Базовая тема
 * CodeMirror вставляется элементом <style>, а CSP окна (`style-src 'self'`)
 * его запрещает — у текста был `white-space: normal`, пробелы в начале строки
 * и подряд схлопывались. Правила базовой темы теперь в статическом
 * src/editor/codemirror-base.css, CSP та же.
 */
async function expectIndentsVisible(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await cursorTo(browser, 'Последняя строка.');
  const look = await browser.execute((selector) => {
    const lines = [...document.querySelectorAll(`${selector} .cm-line`)];
    /** Левый край первого видимого символа строки (не пробела). */
    const start = (fragment: string) => {
      const line = lines.find((item) => (item.textContent ?? '').includes(fragment));
      if (!line) return -1;
      const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.textContent ?? '';
        const at = text.search(/\S/);
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        return Math.round(range.getBoundingClientRect().left);
      }
      return -1;
    };
    const content = document.querySelector(selector) as HTMLElement;
    return {
      whiteSpace: getComputedStyle(content).whiteSpace,
      outline: getComputedStyle(content).outlineStyle,
      focused: document.activeElement === content,
      nested: start('Вложенный пункт'),
      top: start('Второй пункт'),
      codeInner: start('return 42;'),
      codeOuter: start('function ответ'),
    };
  }, TEXT);
  const detail = JSON.stringify(look);
  expect(['pre-wrap', 'break-spaces'], detail).toContain(look.whiteSpace);
  // Два пробела перед «- Вложенный пункт», четыре перед «return».
  expect(look.nested - look.top, detail).toBeGreaterThanOrEqual(4);
  expect(look.codeInner - look.codeOuter, detail).toBeGreaterThanOrEqual(8);
  expect(look.focused, detail).toBe(true);
  expect(look.outline, detail).toBe('none');
  await browser.execute(() => {
    const line = [...document.querySelectorAll('.cm-line')].find((item) => (item.textContent ?? '').includes('Пункт списка'));
    line?.scrollIntoView({ block: 'start' });
  });
  await shotThemes(browser, name);
}

describe('живой предпросмотр и режим исходного текста', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';
  let file = '';

  beforeAll(async () => {
    vault = createTestVault('Предпросмотр', { [`${NOTE}.md`]: SAMPLE });
    profile = createTestProfile();
    file = path.join(vault, `${NOTE}.md`);
    session = await startApp({ vault, profile });
    const { browser } = session;
    const row = browser.$(`[role="treeitem"][aria-label="${NOTE}"]`);
    await row.waitForDisplayed({ timeout: 20_000 });
    await row.click();
    await browser.$(TEXT).waitForDisplayed({ timeout: 10_000 });
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

  it('курсор вне разметки: разметка спрятана, элементы оформлены (С3.3)', async () => {
    const b = browser();
    await cursorTo(b, 'Последняя строка.');
    expect(await modeChecked(b)).toContain('Живой предпросмотр');
    // Заголовки 1–6 без «#».
    for (let level = 1; level <= 6; level += 1) {
      expect(await lineText(b, `Заголовок ${level}`)).toBe(`Заголовок ${level}`);
      expect(await count(b, `cm-rz-h${level}`), `заголовок ${level}`).toBe(1);
    }
    expect(await lineText(b, 'жирным')).toBe('Текст с жирным, курсивом, зачёркнутым и кодом в строке.');
    for (const cls of ['cm-rz-strong', 'cm-rz-em', 'cm-rz-strike', 'cm-rz-code']) {
      expect(await count(b, cls), cls).toBeGreaterThan(0);
    }
    expect(await count(b, 'cm-rz-quote')).toBeGreaterThanOrEqual(2);
    expect(await lineText(b, 'Вложенная цитата')).not.toContain('>');
    expect(await count(b, 'cm-rz-bullet')).toBeGreaterThanOrEqual(3);
    expect(await count(b, 'cm-rz-checkbox')).toBe(2);
    expect(await count(b, 'cm-rz-checkbox-checked')).toBe(1);
    expect(await count(b, 'cm-rz-rule')).toBe(1);
    expect(await count(b, 'cm-rz-codeblock')).toBeGreaterThanOrEqual(1);
    expect(await lineText(b, 'Ссылка:')).toBe('Ссылка: портал регламентов.');
    expect(await count(b, 'cm-rz-link')).toBe(1);
    expect(await count(b, 'cm-rz-markup')).toBe(0);
    await shotThemes(b, 'cursor-outside-bottom');
    // Начало заметки (заголовки 1–6): прокрутка вверх, курсор остаётся внизу.
    await b.execute(() => {
      const body = document.querySelector('main .body');
      if (body) body.scrollTop = 0;
    });
    await shotThemes(b, 'cursor-outside-top');
  });

  it('курсор на строке с разметкой: разметка этой строки видна, остальные — нет', async () => {
    const b = browser();
    await cursorTo(b, 'жирным');
    expect(await lineText(b, 'жирным')).toBe(
      'Текст с **жирным**, *курсивом*, ~~зачёркнутым~~ и `кодом в строке`.',
    );
    expect(await count(b, 'cm-rz-markup')).toBeGreaterThan(0);
    expect(await lineText(b, 'Заголовок 2')).toBe('Заголовок 2');
    await shotThemes(b, 'cursor-on-markup');
    await cursorTo(b, 'Заголовок 2');
    expect(await lineText(b, 'Заголовок 2')).toBe('## Заголовок 2');
    expect(await lineText(b, 'жирным')).toBe('Текст с жирным, курсивом, зачёркнутым и кодом в строке.');
    await shotThemes(b, 'cursor-on-heading');
  });

  it('живой предпросмотр: пробелы в начале строки видны — вложенный пункт и код сдвинуты (Н7), рамки фокуса нет (Н3)', async () => {
    await expectIndentsVisible(browser(), 'indents-preview');
  });

  it('Ctrl+E — режим исходного текста: разметка как есть, без оформления; файл не меняется', async () => {
    const b = browser();
    await b.keys(['Control', 'e']);
    await b.waitUntil(async () => (await modeChecked(b)).includes('Исходный текст'), {
      timeout: 5_000,
      timeoutMsg: 'Ctrl+E не переключил режим',
    });
    await cursorTo(b, 'Последняя строка.');
    expect(await lineText(b, 'Заголовок 1')).toBe('# Заголовок 1');
    expect(await lineText(b, 'Ссылка:')).toBe('Ссылка: [портал регламентов](https://example.com/reglament).');
    expect(await lineText(b, 'Открытая задача')).toBe('- [ ] Открытая задача');
    for (const cls of ['cm-rz-heading', 'cm-rz-strong', 'cm-rz-checkbox', 'cm-rz-rule', 'cm-rz-bullet']) {
      expect(await count(b, cls), cls).toBe(0);
    }
    await shotThemes(b, 'source-mode');
    expect(readFileSync(file, 'utf8')).toBe(SAMPLE);
  });

  it('исходный текст: пробелы в начале строки видны (Н7), рамки фокуса нет (Н3)', async () => {
    await expectIndentsVisible(browser(), 'indents-source');
  });

  it('выбранный режим помнится после перезапуска (С3.3, решение 72)', async () => {
    await session?.stop();
    session = undefined;
    // Программа выходит после записи ввода (решение 53): замок одного экземпляра освобождается не сразу.
    expect(await waitForAppExit(20_000), 'программа не завершилась').toBeGreaterThanOrEqual(0);
    session = await startApp({ profile });
    const b = browser();
    await b.$(TEXT).waitForDisplayed({ timeout: 30_000 });
    await b.waitUntil(async () => (await modeChecked(b)).includes('Исходный текст'), {
      timeout: 10_000,
      timeoutMsg: `режим после перезапуска: «${await modeChecked(b)}»`,
    });
    expect(await lineText(b, 'Заголовок 1')).toBe('# Заголовок 1');
    // Кнопка «книга» возвращает живой предпросмотр.
    await b.$('[role="radiogroup"] [role="radio"][aria-label^="Живой предпросмотр"]').click();
    await b.waitUntil(async () => (await modeChecked(b)).includes('Живой предпросмотр'), { timeout: 5_000 });
    expect(readFileSync(file, 'utf8')).toBe(SAMPLE);
  }, 150_000);

  it('щелчок по чекбоксу переключает [ ] ↔ [x] в файле', async () => {
    const b = browser();
    await cursorTo(b, 'Последняя строка.');
    await b.$(`${TEXT} .cm-rz-checkbox:not(.cm-rz-checkbox-checked)`).click();
    await b.waitUntil(() => readFileSync(file, 'utf8').includes('- [x] Открытая задача'), {
      timeout: 5_000,
      timeoutMsg: 'чекбокс не записан в файл',
    });
    expect(readFileSync(file, 'utf8')).toBe(SAMPLE.replace('- [ ] Открытая задача', '- [x] Открытая задача'));
    await shotThemes(b, 'checkbox-toggled');
  });
});
