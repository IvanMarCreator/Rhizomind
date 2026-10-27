// Стили, которые CodeMirror вставляет сам (задача 2.10а, замечание QA З1).
//
// CodeMirror и его расширения кладут свои темы в один элемент <style>
// (пакет style-mod), а CSP окна (`style-src 'self'`) его запрещает — в окне
// действует только наш статический CSS. Сценарный тест (tests/e2e/csp.ts)
// пропускает этот элемент как известный, поэтому новая тема (новое
// расширение, новая версия CodeMirror) прошла бы незамеченной.
//
// Здесь собирается всё, что style-mod вставляет для editorExtensions() во
// всех режимах (живой предпросмотр, исходный текст, вики-ссылки с открытым
// списком подсказок). Каждый селектор (сгенерированные классы заменены на
// `&`, `&light`, `&dark`) должен быть в таблице COVERAGE: либо у него есть
// правило в нашем CSS, либо названа причина, почему оно не нужно. Новый
// селектор роняет тест — его нужно разобрать и внести в таблицу.
import { EditorState, type Extension } from '@codemirror/state';
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightTrailingWhitespace,
  highlightWhitespace,
  lineNumbers,
  placeholder,
} from '@codemirror/view';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { editorExtensions } from './codemirror.ts';
import type { WikiHost } from './wikilinks/host.ts';

const OUR_CSS = {
  base: 'codemirror-base.css',
  note: 'NoteText.svelte',
  wiki: 'wikilinks/wikilinks.css',
} as const;

/**
 * Чем закрыт селектор: правило в нашем CSS с ключевым свойством (`prop` —
 * текст объявления в блоке правила) либо причина, почему не нужен, и признак
 * в DOM редакторов, которого при этой причине быть не должно (`absent`).
 */
type Coverage = { file: keyof typeof OUR_CSS; rule: string; prop: string } | { skip: string; absent: string };

const css = (file: keyof typeof OUR_CSS, rule: string, prop: string): Coverage => ({ file, rule, prop });
const skip = (reason: string, absent: string): Coverage => ({ skip: reason, absent });

const NO_DRAW_SELECTION = skip('drawSelection не подключён: курсор и выделение рисует движок', '.cm-cursorLayer, .cm-selectionLayer, .cm-dropCursor');
const NO_GUTTERS = skip('полей номеров строк нет', '.cm-gutters, .cm-gutter');
const NO_PANELS = skip('панелей и диалогов CodeMirror (поиск и т. п.) нет', '.cm-panels, .cm-dialog');
const NO_SPACE_MARKS = skip('подсветки пробелов и табов нет', '.cm-highlightSpace, .cm-highlightTab, .cm-trailingSpace');
const NO_ACTIVE_LINE = skip('highlightActiveLine не подключён', '.cm-activeLine, .cm-activeLineGutter');
const NO_ARROW = skip('подсказки без стрелки', '.cm-tooltip-arrow');

/** Каждый селектор тем CodeMirror в окне — и чем он закрыт. */
const COVERAGE: Record<string, Coverage> = {
  '&': css('base', '.cm-editor {', 'display: flex !important'),
  '&.cm-focused': css('base', '.cm-editor.cm-focused {', 'outline: none'),
  '& .cm-scroller': css('base', '.cm-editor .cm-scroller {', 'align-items: flex-start !important'),
  '& .cm-content': css('base', '.cm-editor .cm-content {', 'white-space: pre'),
  '& .cm-content[contenteditable=true]': css('base', ".cm-editor .cm-content[contenteditable='true'] {", '-webkit-user-modify: read-write-plaintext-only'),
  '& .cm-lineWrapping': css('base', '.cm-editor .cm-lineWrapping {', 'white-space: break-spaces'),
  '& .cm-line': css('base', '.cm-editor .cm-line {', 'display: block'),
  '& .cm-layer': css('base', '.cm-editor .cm-layer {', 'contain: size style'),
  '& .cm-layer > *': css('base', '.cm-editor .cm-layer > * {', 'position: absolute'),
  '& .cm-cursorLayer': css('base', '.cm-editor .cm-cursorLayer {', 'pointer-events: none'),
  '& .cm-iso': css('base', '.cm-editor .cm-iso {', 'unicode-bidi: isolate'),
  '& .cm-announced': css('base', '.cm-editor .cm-announced {', 'top: -10000px'),
  '@media print & .cm-announced': css('base', '@media print', 'display: none'),
  '& .cm-tab': css('base', '.cm-editor .cm-tab {', 'display: inline-block'),
  '& .cm-widgetBuffer': css('base', '.cm-editor .cm-widgetBuffer {', 'width: 0'),
  '& .cm-tooltip': css('base', '.cm-editor .cm-tooltip {', 'box-sizing: border-box'),
  '&light .cm-content': css('note', ':global(.cm-content) {', 'caret-color: var(--accent)'),
  '&dark .cm-content': css('note', ':global(.cm-content) {', 'caret-color: var(--accent)'),
  '&light .cm-specialChar': css('note', ':global(.cm-specialChar) {', 'color: var(--status-error)'),
  '&dark .cm-specialChar': css('note', ':global(.cm-specialChar) {', 'color: var(--status-error)'),
  '&light .cm-tooltip': css('wiki', '.cm-tooltip.cm-rz-suggest {', 'background: var(--bg-popover)'),
  '&dark .cm-tooltip': css('wiki', '.cm-tooltip.cm-rz-suggest {', 'background: var(--bg-popover)'),

  '& .cm-cursor': NO_DRAW_SELECTION,
  '& .cm-dropCursor': NO_DRAW_SELECTION,
  '&dark .cm-cursor': NO_DRAW_SELECTION,
  '&.cm-focused > .cm-scroller > .cm-cursorLayer': NO_DRAW_SELECTION,
  '&.cm-focused > .cm-scroller > .cm-cursorLayer .cm-cursor': NO_DRAW_SELECTION,
  '& .cm-selectionHandle': NO_DRAW_SELECTION,
  '& .cm-selectionHandle-start::before': NO_DRAW_SELECTION,
  '& .cm-selectionHandle-end::before': NO_DRAW_SELECTION,
  '&light .cm-selectionBackground': NO_DRAW_SELECTION,
  '&dark .cm-selectionBackground': NO_DRAW_SELECTION,
  '&light.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': NO_DRAW_SELECTION,
  '&dark.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': NO_DRAW_SELECTION,
  '@keyframes cm-blink': NO_DRAW_SELECTION,
  '@keyframes cm-blink2': NO_DRAW_SELECTION,

  '& .cm-gutters': NO_GUTTERS,
  '& .cm-gutters-before': NO_GUTTERS,
  '& .cm-gutters-after': NO_GUTTERS,
  '&light .cm-gutters': NO_GUTTERS,
  '&light .cm-gutters.cm-gutters-before': NO_GUTTERS,
  '&light .cm-gutters.cm-gutters-after': NO_GUTTERS,
  '&dark .cm-gutters': NO_GUTTERS,
  '& .cm-gutter': NO_GUTTERS,
  '& .cm-gutterElement': NO_GUTTERS,
  '& .cm-lineNumbers .cm-gutterElement': NO_GUTTERS,
  '&light .cm-activeLineGutter': NO_GUTTERS,
  '&dark .cm-activeLineGutter': NO_GUTTERS,

  '& .cm-panels': NO_PANELS,
  '&light .cm-panels': NO_PANELS,
  '&dark .cm-panels': NO_PANELS,
  '& .cm-panels-top': NO_PANELS,
  '& .cm-panels-bottom': NO_PANELS,
  '&light .cm-panels-top': NO_PANELS,
  '&light .cm-panels-bottom': NO_PANELS,
  '& .cm-dialog': NO_PANELS,
  '& .cm-dialog label': NO_PANELS,
  '& .cm-dialog-close': NO_PANELS,
  '& .cm-button': NO_PANELS,
  '&light .cm-button': NO_PANELS,
  '&light .cm-button:active': NO_PANELS,
  '&dark .cm-button': NO_PANELS,
  '&dark .cm-button:active': NO_PANELS,
  '& .cm-textfield': NO_PANELS,
  '&light .cm-textfield': NO_PANELS,
  '&dark .cm-textfield': NO_PANELS,

  '& .cm-highlightSpace': NO_SPACE_MARKS,
  '& .cm-highlightTab': NO_SPACE_MARKS,
  '& .cm-trailingSpace': NO_SPACE_MARKS,
  '& .cm-placeholder': skip('placeholder не подключён', '.cm-placeholder'),
  '&light .cm-activeLine': NO_ACTIVE_LINE,
  '&dark .cm-activeLine': NO_ACTIVE_LINE,

  '& .cm-tooltip-arrow': NO_ARROW,
  '& .cm-tooltip-arrow:before': NO_ARROW,
  '& .cm-tooltip-arrow:after': NO_ARROW,
  '& .cm-tooltip-above .cm-tooltip-arrow': NO_ARROW,
  '& .cm-tooltip-above .cm-tooltip-arrow:before': NO_ARROW,
  '& .cm-tooltip-above .cm-tooltip-arrow:after': NO_ARROW,
  '& .cm-tooltip-below .cm-tooltip-arrow': NO_ARROW,
  '& .cm-tooltip-below .cm-tooltip-arrow:before': NO_ARROW,
  '& .cm-tooltip-below .cm-tooltip-arrow:after': NO_ARROW,
  '&dark .cm-tooltip .cm-tooltip-arrow:before': NO_ARROW,
  '&dark .cm-tooltip .cm-tooltip-arrow:after': NO_ARROW,
  '&light .cm-tooltip-section:not(:first-child)': skip('в списке подсказок одна секция', '.cm-tooltip-section'),
};

const host: WikiHost = {
  resolve: (keys) => Promise.resolve(new Map(keys.map((key) => [key, true]))),
  open: () => undefined,
  suggest: () => Promise.resolve([{ insert: 'План', name: 'План', path: ['План.md'], linkable: true }]),
  subscribe: () => () => undefined,
};

const views: EditorView[] = [];

// jsdom не считает раскладку: без заглушки CodeMirror пишет в журнал
// TypeError из измерения (Range.getClientRects).
const originalRects = Range.prototype.getClientRects;
const originalRect = Range.prototype.getBoundingClientRect;

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

afterAll(() => {
  for (const view of views) view.destroy();
  Range.prototype.getClientRects = originalRects;
  Range.prototype.getBoundingClientRect = originalRect;
});

/** Текст с пробелами подряд, табом и пробелом в конце строки — для подсветки пробелов. */
const TEXT = '# Заголовок\n**жирный** [[Цель]] `код`\n- [ ] задача\n\tтаб  и  пробелы \n';

/**
 * Редакторы как в окне, в `container`: оба режима, `[[` открывает список
 * подсказок; третий — с пустым документом (там появился бы placeholder).
 */
async function mountEditors(container: HTMLElement, extra: Extension = []): Promise<void> {
  for (const mode of ['source', 'live_preview'] as const) {
    const state = EditorState.create({ doc: TEXT, extensions: [editorExtensions({ mode, wiki: host }), extra] });
    const view = new EditorView({ parent: container, state });
    views.push(view);
    view.focus();
    const at = view.state.doc.length;
    view.dispatch({ changes: { from: at, insert: '[[П' }, selection: { anchor: at + 3 }, userEvent: 'input.type' });
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  const empty = EditorState.create({ doc: '', extensions: [editorExtensions({ mode: 'live_preview', wiki: host }), extra] });
  views.push(new EditorView({ parent: container, state: empty }));
}

/** Причины «не нужен», которые DOM редакторов опровергает. */
function absentViolations(container: HTMLElement): string[] {
  const found = new Set<string>();
  for (const [selector, coverage] of Object.entries(COVERAGE)) {
    if ('absent' in coverage && container.querySelector(coverage.absent)) found.add(`${coverage.absent} (${selector}: ${coverage.skip})`);
  }
  return [...found];
}

function newContainer(): HTMLElement {
  const container = document.createElement('div');
  document.body.append(container);
  return container;
}

/** Классы базовой темы: общий, светлый и тёмный (по редакторам с тёмной темой и без). */
function themeIds(): Map<string, string> {
  const light = new EditorView({ parent: document.body });
  const dark = new EditorView({ parent: document.body, extensions: EditorView.darkTheme.of(true) });
  const generated = (view: EditorView) => view.dom.className.split(/\s+/).filter((name) => name.startsWith('ͼ'));
  const [base, lightId] = generated(light);
  const [, darkId] = generated(dark);
  light.destroy();
  dark.destroy();
  if (!base || !lightId || !darkId) throw new Error('классы базовой темы не найдены');
  return new Map([
    [base, '&'],
    [lightId, '&light'],
    [darkId, '&dark'],
  ]);
}

function normalize(selector: string, ids: Map<string, string>): string {
  return selector
    .trim()
    .replace(/\.ͼ[0-9a-z]+/g, (cls) => ids.get(cls.slice(1)) ?? '&theme')
    .replace(/\s+/g, ' ');
}

/** Селекторы правил элемента <style> от style-mod. */
function insertedSelectors(ids: Map<string, string>): string[] {
  const sheets = [...document.querySelectorAll('style')].filter((style) => (style.textContent ?? '').includes('ͼ'));
  expect(sheets, 'элемент <style> от CodeMirror').toHaveLength(1);
  const found = new Set<string>();
  const walk = (rules: CSSRuleList, prefix: string) => {
    for (const rule of rules) {
      if (rule instanceof CSSStyleRule) {
        for (const part of rule.selectorText.split(',')) found.add(prefix + normalize(part, ids));
      } else if (rule instanceof CSSMediaRule) {
        walk(rule.cssRules, `@media ${rule.media.mediaText} `);
      } else if (rule instanceof CSSKeyframesRule) {
        found.add(`@keyframes ${rule.name}`);
      } else {
        found.add(`? ${rule.cssText.slice(0, 60)}`);
      }
    }
  };
  const sheet = sheets[0]?.sheet;
  if (sheet) walk(sheet.cssRules, '');
  return [...found].sort();
}

describe('темы CodeMirror в окне закрыты нашим статическим CSS (З1)', () => {
  let main: HTMLElement;

  beforeAll(async () => {
    main = newContainer();
    await mountEditors(main);
  });

  it('набор селекторов, которые вставляет CodeMirror, совпадает с таблицей', () => {
    expect(main.querySelectorAll('.cm-tooltip.cm-rz-suggest').length, 'список подсказок открыт').toBeGreaterThan(0);
    const selectors = insertedSelectors(themeIds());
    // Новый селектор — разобрать: правило в нашем CSS или причина, почему не нужно.
    expect(selectors).toEqual(Object.keys(COVERAGE).sort());
  });

  it('причины «не нужен» верны: признаков этих возможностей в редакторах нет (Р1)', () => {
    expect(absentViolations(main)).toEqual([]);
  });

  it.each([
    ['highlightActiveLine()', highlightActiveLine()],
    ['lineNumbers()', lineNumbers()],
    ['placeholder()', placeholder('Пусто')],
    ['highlightWhitespace()', highlightWhitespace()],
    ['highlightTrailingWhitespace()', highlightTrailingWhitespace()],
    ['drawSelection()', drawSelection()],
  ])('подключённое %s опровергает причину — проверка падает', async (_name, extension) => {
    const container = newContainer();
    await mountEditors(container, extension);
    expect(absentViolations(container).length).toBeGreaterThan(0);
  });

  it('для каждого закрытого селектора правило с ключевым свойством есть в нашем CSS (Р2)', () => {
    const sources = Object.fromEntries(
      Object.entries(OUR_CSS).map(([key, file]) => [key, readFileSync(path.join(import.meta.dirname, file), 'utf8')]),
    ) as Record<keyof typeof OUR_CSS, string>;
    const missing: string[] = [];
    for (const [selector, coverage] of Object.entries(COVERAGE)) {
      if (!('file' in coverage)) continue;
      const source = sources[coverage.file];
      const at = source.indexOf(coverage.rule);
      // Блок правила — до первой «}» (у @media — первое вложенное правило).
      const block = at < 0 ? '' : source.slice(at, source.indexOf('}', at));
      if (!block.includes(`${coverage.prop};`)) missing.push(`${selector}: ${OUR_CSS[coverage.file]} «${coverage.rule}» без «${coverage.prop}»`);
    }
    expect(missing).toEqual([]);
  });

  it('новая тема (EditorView.theme) роняет проверку набора', () => {
    // Темы расширений из проверок выше тоже уже в элементе — сравнивается прирост.
    const before = new Set(insertedSelectors(themeIds()));
    const view = new EditorView({
      parent: newContainer(),
      extensions: EditorView.theme({ '.cm-rz-new': { color: 'red' } }),
    });
    views.push(view);
    const extra = insertedSelectors(themeIds()).filter((selector) => !before.has(selector));
    expect(extra).toEqual(['&theme .cm-rz-new']);
    expect(extra.filter((selector) => !(selector in COVERAGE))).toEqual(extra);
  });
});
