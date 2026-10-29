// Живой предпросмотр и режим исходного текста (задача 3.1 по 2.5; С3.3,
// решение 72, макет Э13). Заметка со всеми элементами С3.3 на настоящем ядре:
// снимки в светлой и тёмной теме — курсор вне разметки, курсор на строке с
// разметкой, режим исходного текста (Ctrl+E); переключение режима не меняет
// файл; чекбокс щелчком; выбранный режим помнится после перезапуска.
import { readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, waitForAppExit, type Session } from './driver.ts';
import { unexpectedStyleViolations, watchCsp } from './csp.ts';
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

/** Заметка со ссылкой длиннее колонки (находка Е1, решение 83). */
const LONG_NOTE = 'Длинная ссылка';
// Без дефисов и других мест переноса: переносится только посреди слова.
const LONG_URL = `https://example.com/?q=${'abcdefghij'.repeat(32)}`;
const LONG_SAMPLE = ['Обычный абзац перед ссылкой.', '', `Адрес: ${LONG_URL}`, '', 'Обычный абзац после ссылки.', ''].join('\n');

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
    vault = createTestVault('Предпросмотр', { [`${NOTE}.md`]: SAMPLE, [`${LONG_NOTE}.md`]: LONG_SAMPLE });
    profile = createTestProfile();
    file = path.join(vault, `${NOTE}.md`);
    session = await startApp({ vault, profile });
    const { browser } = session;
    await browser.$(`[role="treeitem"][aria-label="${NOTE}"]`).waitForExist({ timeout: 20_000 });
    await watchCsp(browser);
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
    // «↗» за внешней ссылкой (Э1) — значок, текст заметки тот же.
    expect(await lineText(b, 'Ссылка:')).toBe('Ссылка: портал регламентов↗.');
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

  it('вид по макету Э1 (задача 2.13, Н8): цитаты, списки, «↗», заголовки, блок кода', async () => {
    const b = browser();
    await cursorTo(b, 'Последняя строка.');
    const look = await b.execute((selector) => {
      const content = document.querySelector(selector) as HTMLElement;
      const left = content.getBoundingClientRect().left;
      const lines = [...content.querySelectorAll<HTMLElement>('.cm-line')];
      const find = (fragment: string) => lines.find((item) => (item.textContent ?? '').includes(fragment));
      /** Левый край первого символа текста строки от края колонки. */
      const textStart = (fragment: string) => {
        const line = find(fragment);
        if (!line) return -1;
        const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          const at = (node.textContent ?? '').search(/\S/);
          if (at < 0) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          return Math.round(range.getBoundingClientRect().left - left);
        }
        return -1;
      };
      const size = (fragment: string) => Number.parseFloat(getComputedStyle(find(fragment) ?? content).fontSize);
      const pad = (fragment: string) => Number.parseFloat(getComputedStyle(find(fragment) ?? content).paddingTop);
      const fences = [...content.querySelectorAll<HTMLElement>('.cm-rz-codeblock-fence')];
      const link = content.querySelector('.cm-rz-link')?.getBoundingClientRect();
      const arrow = content.querySelector('.cm-rz-link-external');
      const title = document.querySelector('main h1');
      const measure = () => ({ h1: size('Заголовок 1'), h2: size('Заголовок 2'), h6: size('Заголовок 6'), text: size('Последняя строка') });
      const at16 = measure();
      // Размер шрифта из настроек (src/settings/apply.ts): заголовки — вместе с текстом.
      const root = document.documentElement.style;
      const setting = root.getPropertyValue('--note-font-size');
      root.setProperty('--note-font-size', '20px');
      const at20 = measure();
      if (setting) root.setProperty('--note-font-size', setting);
      else root.removeProperty('--note-font-size');
      return {
        quote1: textStart('Цитата первого уровня'),
        quote2: textStart('Вложенная цитата'),
        quote2Class: find('Вложенная цитата')?.className ?? '',
        bullet: textStart('Пункт списка'),
        task: textStart('Открытая задача'),
        arrow: arrow?.textContent ?? '',
        arrowAfterLink: Math.round((arrow?.getBoundingClientRect().left ?? 0) - (link?.right ?? 0)),
        title: title ? Number.parseFloat(getComputedStyle(title).fontSize) : 0,
        at16,
        at20,
        padH2: pad('Заголовок 2'),
        padH3: pad('Заголовок 3'),
        fences: fences.length,
        codeBlock: fences.length === 2 ? Math.round((fences[1]?.getBoundingClientRect().bottom ?? 0) - (fences[0]?.getBoundingClientRect().top ?? 0)) : -1,
      };
    }, TEXT);
    const detail = JSON.stringify(look);
    // Цитата: полоса 3 + зазор 14 на уровень.
    expect(look.quote1, detail).toBe(17);
    expect(look.quote2, detail).toBe(34);
    expect(look.quote2Class, detail).toContain('cm-rz-quote-2');
    // Списки: колонка маркера 20 + зазор 6.
    expect(look.bullet, detail).toBe(26);
    expect(look.task, detail).toBe(26);
    expect(look.arrow, detail).toBe('↗');
    expect(look.arrowAfterLink, detail).toBeGreaterThanOrEqual(2);
    // Заголовки: H1 меньше имени заметки; размеры и отступы — от размера шрифта.
    expect(look.at16, detail).toEqual({ h1: 24, h2: 21, h6: 16, text: 16 });
    expect(look.at16.h1, detail).toBeLessThan(look.title);
    expect(look.at20, detail).toEqual({ h1: 30, h2: 26.25, h6: 20, text: 20 });
    expect([look.padH2, look.padH3], detail).toEqual([12, 12]);
    // Блок кода из трёх строк: 12 + 3 × 20 + 12, ограды не занимают строк.
    expect(look.fences, detail).toBe(2);
    expect(look.codeBlock, detail).toBe(84);
    await b.execute(() => {
      const line = [...document.querySelectorAll('.cm-line')].find((item) => (item.textContent ?? '').includes('Цитата первого'));
      line?.scrollIntoView({ block: 'start' });
    });
    await shotThemes(b, 'look-e1');
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

  it('нарушений CSP по стилям нет, кроме стилей, которые вставляет сам CodeMirror (задача 2.10)', async () => {
    expect(await unexpectedStyleViolations(browser())).toEqual([]);
  });

  it('выбранный режим помнится после перезапуска (С3.3, решение 72)', async () => {
    await session?.stop();
    session = undefined;
    // Конец сессии WebDriver завершает программу силами драйвера (не цикл закрытия
    // решения 53 — его проверяет `close.test.ts`); замок одного экземпляра
    // освобождается не сразу — ждём выхода перед перезапуском.
    expect(await waitForAppExit(20_000), 'программа не завершилась').toBeGreaterThanOrEqual(0);
    session = await startApp({ profile });
    const b = browser();
    await watchCsp(b);
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

  it('ссылка длиннее колонки переносится, абзацы не шире колонки (Е1, решение 83); нарушений CSP по стилям нет', async () => {
    const b = browser();
    const row = b.$(`[role="treeitem"][aria-label="${LONG_NOTE}"]`);
    await row.click();
    await b.waitUntil(async () => (await lineText(b, 'Адрес:')).startsWith('Адрес:'), { timeout: 10_000 });
    const look = await b.execute((selector) => {
      const content = document.querySelector(selector) as HTMLElement;
      const column = content.getBoundingClientRect();
      const lines = [...content.querySelectorAll('.cm-line')].map((line) => {
        const rect = line.getBoundingClientRect();
        return { text: (line.textContent ?? '').slice(0, 12), right: Math.round(rect.right), height: Math.round(rect.height) };
      });
      return { column: Math.round(column.right), width: Math.round(column.width), lines };
    }, TEXT);
    const detail = JSON.stringify(look);
    expect(LONG_URL.length, 'длина ссылки').toBeGreaterThan(300);
    expect(look.width, detail).toBeLessThan(1000);
    for (const line of look.lines) expect(line.right, detail).toBeLessThanOrEqual(look.column + 1);
    const address = look.lines.find((line) => line.text.startsWith('Адрес:'));
    expect(address?.height ?? 0, detail).toBeGreaterThan(60);
    await shotThemes(b, 'long-link');
    expect(await unexpectedStyleViolations(b)).toEqual([]);
  });
});
