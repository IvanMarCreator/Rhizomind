// Живой предпросмотр и режим исходного текста (задача 3.1 по 2.5; С3.3,
// решение 72, макет Э13). Заметка со всеми элементами С3.3 на настоящем ядре:
// снимки в светлой и тёмной теме — курсор вне разметки, курсор у элемента
// (решение 93: раскрыт только он), режим исходного текста (Ctrl+E);
// переключение режима не меняет файл; чекбокс щелчком; выбранный режим
// помнится после перезапуска; Enter продолжает список (решение 92).
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

/** Несколько строчных элементов на строке (решение 93, задача 2.16). */
const INLINE_NOTE = 'Строчные элементы';
const INLINE_SAMPLE = [
  'Начало.',
  '> текст [ссылка](https://a.example/1) и ~~зачёркнутое~~',
  'См. [а](https://a.example/1), ~~б~~ и [в](https://a.example/2) конец',
  'Конец.',
  '',
].join('\n');

/**
 * Колонка текста Э1 (22-259 / 22-535) в Markdown — для сверки промежутков
 * между блоками (задача 2.20). Блоки идут подряд, где Markdown это позволяет;
 * два абзаца разделяет пустая строка.
 */
const E1_NOTE = 'Встреча по запуску';
const E1_SAMPLE = [
  '**Дата:** 22.10.2026 · **Участники:** Н. Орлова, Д. Ким, П. Захаров',
  '',
  'Продолжаем обсуждение из [[Встреча]]. Цель — согласовать пилот и сроки запуска.',
  '## Решения',
  '- Пилот запускаем в отделе закупок с **3 ноября**.',
  '- Доступы выдаём по [[Регламент доступа]], срок — два рабочих дня.',
  '- Инструкцию для пользователей готовит Д. Ким: [[Инструкция пользователя]].',
  '> Пилот не расширяем, пока нет заключения службы ИБ.',
  '## Задачи',
  '- [x] Согласовать список пилотных пользователей',
  '- [ ] Подготовить тестовый стенд к 30.10',
  '- [ ] Сверить итоги с прошлой встречей: [[Альфа/Встреча]]',
  'Параметры пилота (файл `pilot.yaml`):',
  '```yaml',
  '# пилот «Альфы»',
  'pilot:',
  '  department: закупки',
  '  users: 25',
  '  start: 2026-11-03',
  '```',
  'Процесс согласования описан на [портале регламентов](https://example.com/reglament).',
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

/** Нажать клавишу `times` раз. */
async function press(browser: WebdriverIO.Browser, key: string, times: number): Promise<void> {
  for (let k = 0; k < times; k += 1) await browser.keys(key);
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
    vault = createTestVault('Предпросмотр', { [`${NOTE}.md`]: SAMPLE, [`${LONG_NOTE}.md`]: LONG_SAMPLE, [`${INLINE_NOTE}.md`]: INLINE_SAMPLE, [`${E1_NOTE}.md`]: E1_SAMPLE });
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
      /** Левый край слова `fragment` от края колонки (после номера списка — само слово). */
      const wordStart = (fragment: string) => {
        const line = find(fragment);
        if (!line) return -1;
        const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          const at = (node.textContent ?? '').indexOf(fragment);
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
      const titleSize = () => (title ? Number.parseFloat(getComputedStyle(title).fontSize) : 0);
      /** Значки списков и отступы (задача 2.20) — от размера шрифта. */
      const lists = () => {
        const center = (element: Element | null | undefined) => {
          const rect = element?.getBoundingClientRect();
          return rect ? { x: Math.round((rect.left + rect.right) / 2 - left), y: (rect.top + rect.bottom) / 2 } : { x: -1, y: -1 };
        };
        const task = find('Открытая задача');
        const box = task?.querySelector('.cm-rz-checkbox');
        const boxRect = box?.getBoundingClientRect();
        const taskRect = task?.getBoundingClientRect();
        const bullet = find('Пункт списка')?.querySelector('.cm-rz-bullet');
        const number = find('Первый')?.querySelector('.cm-rz-list-number');
        return {
          title: titleSize(),
          bullet: textStart('Пункт списка'),
          nested: textStart('Вложенный пункт'),
          ordered: wordStart('Первый'),
          numberX: center(number).x,
          bulletX: center(bullet).x,
          dot: bullet ? Number.parseFloat(getComputedStyle(bullet, '::before').width) : -1,
          box: boxRect ? Math.round(boxRect.width) : -1,
          // Середина чекбокса — от середины строки (строка без промежутка: 26 px при 16).
          boxOffCenter: boxRect && taskRect ? Math.abs(center(box).y - (taskRect.top + taskRect.bottom) / 2) : -1,
          taskText: textStart('Открытая задача'),
        };
      };
      const at16 = measure();
      const lists16 = lists();
      // Размер шрифта из настроек (src/settings/apply.ts): заголовки — вместе с текстом.
      const root = document.documentElement.style;
      const setting = root.getPropertyValue('--note-font-size');
      root.setProperty('--note-font-size', '20px');
      const at20 = measure();
      const title20 = titleSize();
      root.setProperty('--note-font-size', '32px');
      const lists32 = lists();
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
        title: titleSize(),
        at16,
        at20,
        title20,
        lists16,
        lists32,
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
    // Имя заметки — 1,75 от размера текста (задача 2.20): больше H1 при любом размере.
    expect(look.title, detail).toBe(28);
    expect(look.title20, detail).toBe(35);
    expect(look.at20.h1, detail).toBeLessThan(look.title20);
    // Списки (задача 2.20): вложенный пункт — на 26 правее, номер — в колонке маркера, как точка;
    // значки от размера шрифта, чекбокс по центру строки. При 32 px — всё вдвое.
    for (const [lists, k] of [[look.lists16, 1], [look.lists32, 2]] as const) {
      expect(lists.bullet, detail).toBe(26 * k);
      expect(lists.nested, detail).toBe(52 * k);
      expect(lists.ordered, detail).toBe(26 * k);
      expect(lists.taskText, detail).toBe(26 * k);
      expect(Math.abs(lists.numberX - 10 * k), detail).toBeLessThanOrEqual(1);
      expect(Math.abs(lists.bulletX - 10 * k), detail).toBeLessThanOrEqual(1);
      expect(lists.dot, detail).toBe(5 * k);
      expect(lists.box, detail).toBe(16 * k);
      expect(lists.boxOffCenter, detail).toBeLessThanOrEqual(1.5 * k);
    }
    expect(look.lists32.title, detail).toBe(56);
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

  it('курсор у элемента: видна разметка только этого элемента (решение 93); маркер заголовка — на всей строке', async () => {
    const b = browser();
    // Конец строки — за «.»: ни один элемент не задет, строка оформлена.
    await cursorTo(b, 'жирным');
    expect(await lineText(b, 'жирным')).toBe('Текст с жирным, курсивом, зачёркнутым и кодом в строке.');
    expect(await count(b, 'cm-rz-markup')).toBe(0);
    // Шаг влево — к краю кода в строке: раскрыт только он.
    await b.keys('ArrowLeft');
    expect(await lineText(b, 'жирным')).toBe('Текст с жирным, курсивом, зачёркнутым и `кодом в строке`.');
    expect(await count(b, 'cm-rz-markup')).toBe(2);
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

  it('Enter продолжает список, пустой Enter завершает его (решение 92, задача 2.15)', async () => {
    const b = browser();
    const before = readFileSync(file, 'utf8');
    await cursorTo(b, 'Последняя строка.');
    // По одной клавише: WebKitWebDriver склеивает одинаковые клавиши подряд в одном вызове.
    for (const key of ['Enter', ...'- one', 'Enter', ...'two', 'Enter', ...'three', 'Enter', 'Enter', ...'end']) await b.keys(key);
    const expected = before.replace('Последняя строка.\n', 'Последняя строка.\n- one\n- two\n- three\nend\n');
    expect(expected, 'образец изменён').not.toBe(before);
    await b.waitUntil(() => readFileSync(file, 'utf8') === expected, {
      timeout: 10_000,
      timeoutMsg: `в файле не тот текст: ${JSON.stringify(readFileSync(file, 'utf8').slice(-80))}`,
    });
    await shotThemes(b, 'list-continued');
  });

  it('парная скобка: `- [` + пробел + текст — готовая задача в файле (решение 95, задача 2.18)', async () => {
    const b = browser();
    const before = readFileSync(file, 'utf8');
    await cursorTo(b, 'Последняя строка.');
    // По одной клавише: WebKitWebDriver склеивает одинаковые клавиши подряд в одном вызове.
    for (const key of ['Enter', '-', ' ', '[', ' ', ...'task']) await b.keys(key);
    const expected = before.replace('Последняя строка.\n', 'Последняя строка.\n- [ ] task\n');
    expect(expected, 'образец изменён').not.toBe(before);
    await b.waitUntil(() => readFileSync(file, 'utf8') === expected, {
      timeout: 10_000,
      timeoutMsg: `в файле не тот текст: ${JSON.stringify(readFileSync(file, 'utf8').slice(-80))}`,
    });
    await shotThemes(b, 'bracket-task');
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
  it('строка с цитатой, двумя ссылками и зачёркнутым: раскрыт только элемент с курсором, `>` — на всей строке (решение 93)', async () => {
    const b = browser();
    await b.$(`[role="treeitem"][aria-label="${INLINE_NOTE}"]`).click();
    await b.waitUntil(async () => (await lineText(b, 'Начало.')) === 'Начало.', { timeout: 10_000 });
    // Курсор — клавишами: щелчок по оформленной ссылке открыл бы её.
    await cursorTo(b, 'Начало.');
    await b.keys(['ArrowDown', 'Home']);
    expect(await lineText(b, 'текст')).toBe('> текст ссылка↗ и зачёркнутое');
    // По одной клавише: повторы в одном массиве WebdriverIO нажимает как одну.
    await press(b, 'ArrowRight', 3);
    // Курсор в слове «текст»: виден `>`, ссылка и зачёркнутое оформлены.
    expect(await lineText(b, 'текст')).toBe('> текст ссылка↗ и зачёркнутое');
    await shotThemes(b, 'element-quote-text');
    // Шесть шагов вправо — внутрь ссылки: видны `>` и её разметка, зачёркнутое оформлено.
    await press(b, 'ArrowRight', 6);
    expect(await lineText(b, 'текст')).toBe('> текст [ссылка](https://a.example/1) и зачёркнутое');
    await shotThemes(b, 'element-quote-link');
    // Строка ниже: курсор в начале — всё оформлено; у второй ссылки — раскрыта только она.
    await b.keys(['ArrowDown', 'Home']);
    expect(await lineText(b, 'См.')).toBe('См. а↗, б и в↗ конец');
    // Курсор ушёл со строки цитаты — `>` снова спрятан (полоса цитаты).
    expect(await lineText(b, 'текст')).toBe('текст ссылка↗ и зачёркнутое');
    await b.keys('End');
    await press(b, 'ArrowLeft', 8);
    expect(await lineText(b, 'См.')).toBe('См. а↗, б и [в](https://a.example/2) конец');
    await shotThemes(b, 'element-second-link');
    expect(readFileSync(path.join(vault, `${INLINE_NOTE}.md`), 'utf8')).toBe(INLINE_SAMPLE);
  });

  it('промежутки между блоками по Э1 (задача 2.20): 10 px между блоками, не между пунктами; щелчок, стрелки и выделение по ним', async () => {
    const b = browser();
    await b.$(`[role="treeitem"][aria-label="${E1_NOTE}"]`).click();
    await b.waitUntil(async () => (await lineText(b, 'Решения')) === 'Решения', { timeout: 10_000 });
    // Курсор — в пустую последнюю строку: ни один элемент не раскрыт.
    await cursorTo(b, 'Дата:');
    await b.keys(['Control', 'End']);
    const look = await b.execute((selector) => {
      const content = document.querySelector(selector) as HTMLElement;
      const lines = [...content.querySelectorAll<HTMLElement>('.cm-line')];
      const find = (fragment: string) => lines.find((item) => (item.textContent ?? '').includes(fragment));
      const height = (fragment: string) => Math.round(find(fragment)?.getBoundingClientRect().height ?? -1);
      /** Промежуток под строкой: прозрачная граница снизу, px (строка может переноситься — высоту не сравниваем). */
      const gap = (fragment: string) => {
        const line = find(fragment);
        return line ? Math.round(Number.parseFloat(getComputedStyle(line).borderBottomWidth)) : -1;
      };
      const fences = [...content.querySelectorAll<HTMLElement>('.cm-rz-codeblock-fence')];
      const process = find('Процесс согласования');
      return {
        gaps: {
          // Абзац перед пустой строкой и перед заголовком, заголовок перед списком.
          date: gap('Дата:'),
          paragraph: gap('Продолжаем'),
          h2: gap('Решения'),
          // Пункты одного списка — без промежутка, последний (перед цитатой) — с ним.
          item1: gap('Пилот запускаем'),
          item2: gap('Доступы'),
          item3: gap('Инструкцию'),
          quote: gap('Пилот не расширяем'),
          task1: gap('Согласовать'),
          task2: gap('Подготовить'),
          task3: gap('Сверить итоги'),
          params: gap('Параметры пилота'),
          // Под блоком кода — сверху у следующей строки, у ограды — нет: фон блока без промежутка.
          codeFence: fences[1] ? Math.round(Number.parseFloat(getComputedStyle(fences[1]).borderBottomWidth)) : -1,
          processTop: process ? Math.round(Number.parseFloat(getComputedStyle(process).borderTopWidth)) : -1,
        },
        // Пункт без промежутка — ровно строка 26; с промежутком — 26 + 10.
        item1: height('Пилот запускаем'),
        quote: height('Пилот не расширяем'),
        // Блок кода: 12 + 5 × 20 + 12.
        codeBlock: fences.length === 2 ? Math.round((fences[1]?.getBoundingClientRect().bottom ?? 0) - (fences[0]?.getBoundingClientRect().top ?? 0)) : -1,
      };
    }, TEXT);
    const detail = JSON.stringify(look);
    expect(look, detail).toEqual({
      gaps: { date: 10, paragraph: 10, h2: 10, item1: 0, item2: 0, item3: 10, quote: 10, task1: 0, task2: 0, task3: 10, params: 10, codeFence: 0, processTop: 10 },
      item1: 26,
      quote: 36,
      codeBlock: 124,
    });
    await b.execute(() => {
      const body = document.querySelector('main .body');
      if (body) body.scrollTop = 0;
    });
    await shotThemes(b, 'gaps-e1');

    /** Строка, где стоит курсор (фокус выделения). */
    const caretLine = () =>
      b.execute(() => {
        const node = document.getSelection()?.focusNode ?? null;
        const element = node instanceof Element ? node : (node?.parentElement ?? null);
        return element?.closest('.cm-line')?.textContent ?? '';
      });
    // Щелчок в промежуток под пунктом «Инструкцию…» ставит курсор в этот пункт.
    const index = await b.execute(
      (selector) => [...document.querySelectorAll(`${selector} .cm-line`)].findIndex((line) => (line.textContent ?? '').includes('Инструкцию')),
      TEXT,
    );
    const item = (await b.$$(`${TEXT} .cm-line`))[index];
    if (!item) throw new Error('нет строки «Инструкцию»');
    const size = await item.getSize();
    await item.click({ x: Math.round(-size.width / 2 + 60), y: Math.round(size.height / 2 - 4) });
    expect(await caretLine()).toContain('Инструкцию');
    // Стрелки ↓ / ↑ через промежутки — на соседние строки, не застревая.
    await b.keys('ArrowDown');
    expect(await caretLine()).toContain('Пилот не расширяем');
    await b.keys('ArrowDown');
    expect(await caretLine()).toContain('Задачи');
    await b.keys('ArrowUp');
    expect(await caretLine()).toContain('Пилот не расширяем');
    await b.keys('ArrowUp');
    expect(await caretLine()).toContain('Инструкцию');
    // Выделение мышью через промежутки: от первого пункта до цитаты.
    const points = await b.execute((selector) => {
      const lines = [...document.querySelectorAll<HTMLElement>(`${selector} .cm-line`)];
      const at = (fragment: string) => {
        const rect = lines.find((line) => (line.textContent ?? '').includes(fragment))?.getBoundingClientRect();
        return rect ? { x: Math.round(rect.left + 60), y: Math.round(rect.top + 13) } : { x: 0, y: 0 };
      };
      return { from: at('Пилот запускаем'), to: at('Пилот не расширяем') };
    }, TEXT);
    await b
      .action('pointer')
      .move({ x: points.from.x, y: points.from.y })
      .down({ button: 0 })
      .move({ x: points.to.x, y: points.to.y - 6, duration: 100 })
      .move({ x: points.to.x, y: points.to.y, duration: 100 })
      .up({ button: 0 })
      .perform();
    const selected = await b.execute(() => document.getSelection()?.toString() ?? '');
    // Начало и конец — посреди слов (60 px от края строки): целиком — средние строки.
    expect(selected).toContain('запускаем');
    expect(selected).toContain('Доступы');
    expect(selected).toContain('Инструкцию');
    await shotThemes(b, 'gaps-e1-selection');
    expect(readFileSync(path.join(vault, `${E1_NOTE}.md`), 'utf8')).toBe(E1_SAMPLE);
  });
});
