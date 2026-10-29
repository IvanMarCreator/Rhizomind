// Вид живого предпросмотра по макету Э1 (задача 2.13, находки Н8 и замечание
// обзора 2.8б): вычисленные стили из нашего статического CSS
// (live-preview.css; CSP окна запрещает стили, которые вставляет CodeMirror —
// они здесь убраны). jsdom не считает раскладку: положение на экране —
// в сценарном тесте tests/e2e/live-preview.test.ts; здесь — стили и классы,
// от которых оно зависит.
import { EditorSelection } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createEditorState } from '../codemirror.ts';

const css = (file: string) => readFileSync(path.join(import.meta.dirname, file), 'utf8');
const OUR_CSS = [css('../codemirror-base.css'), css('live-preview.css')].join('\n');

const SAMPLE = [
  '# Заголовок 1',
  '## Заголовок 2',
  '### Заголовок 3',
  '#### Заголовок 4',
  '##### Заголовок 5',
  '###### Заголовок 6',
  'Текст',
  '> Цитата',
  '> > Вложенная',
  '> > > Третий уровень',
  '- Пункт списка',
  '- [ ] Задача',
  '```',
  'код',
  '```',
  'Ссылка: [портал регламентов](https://example.com/r), [[Встреча]] и [файл](Заметка.md).',
  'Последняя строка.',
].join('\n');

let view: EditorView | undefined;
const originalRects = Range.prototype.getClientRects;
const originalRect = Range.prototype.getBoundingClientRect;

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

afterAll(() => {
  Range.prototype.getClientRects = originalRects;
  Range.prototype.getBoundingClientRect = originalRect;
});

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.head.replaceChildren();
  document.body.replaceChildren();
});

/**
 * Редактор в живом предпросмотре, курсор на последней строке. `fontSize` —
 * размер шрифта текста: в окне его задаёт `--note-font-size` у .cm-editor
 * (NoteText.svelte); jsdom не подставляет `var()` в размер, поэтому здесь —
 * число в px.
 */
function mount(fontSize: number): EditorView {
  const style = document.createElement('style');
  style.textContent = `${OUR_CSS}\n.cm-editor { font-size: ${fontSize}px; }`;
  document.head.append(style);
  const state = createEditorState(SAMPLE, { mode: 'live_preview' });
  const created = new EditorView({
    parent: document.body,
    state: state.update({ selection: EditorSelection.cursor(state.doc.length) }).state,
  });
  // Как CSP: правила, вставленные CodeMirror (классы вида «ͼ1»), не действуют.
  for (const element of document.querySelectorAll('style')) {
    if ((element.textContent ?? '').includes('ͼ')) element.remove();
  }
  view = created;
  return created;
}

function line(target: EditorView, fragment: string): HTMLElement {
  const found = [...target.contentDOM.querySelectorAll<HTMLElement>('.cm-line')].find((item) =>
    (item.textContent ?? '').includes(fragment),
  );
  if (!found) throw new Error(`нет строки «${fragment}»`);
  return found;
}

const px = (value: string) => Number.parseFloat(value);

describe('заголовки: размеры от размера шрифта текста (обзор 2.8б), H1 меньше имени заметки (Н8)', () => {
  it.each([16, 20])('при %i px', (base) => {
    const target = mount(base);
    const sizes = [1, 2, 3, 4, 5, 6].map((level) => px(getComputedStyle(line(target, `Заголовок ${level}`)).fontSize));
    // tokens.md: 24 / 21 / 18 / 17 / 16 / 16 при 16 px.
    expect(sizes).toEqual([1.5, 1.3125, 1.125, 1.0625, 1, 1].map((ratio) => ratio * base));
    // Имя заметки над текстом — 28 px (Editor.svelte, tokens.md).
    if (base === 16) expect(sizes[0]).toBeLessThan(28);
  });

  it('над каждым заголовком отступ от размера шрифта: 12 px при 16, у H1 — 16 px', () => {
    const target = mount(16);
    for (let level = 2; level <= 6; level += 1) {
      expect(getComputedStyle(line(target, `Заголовок ${level}`)).paddingTop, `H${level}`).toBe(
        'calc(var(--note-font-size, 16px) * 0.75)',
      );
    }
    expect(getComputedStyle(line(target, 'Заголовок 1')).paddingTop).toBe('var(--note-font-size, 16px)');
    expect(getComputedStyle(line(target, 'Текст')).paddingTop).toBe('0px');
  });

  it('блок кода тоже от размера шрифта: 13 / 20 при 16 px', () => {
    const target = mount(20);
    const style = getComputedStyle(line(target, 'код'));
    expect(px(style.fontSize)).toBeCloseTo(13 * 1.25, 5);
    expect(style.lineHeight).toBe('1.5385');
  });
});

describe('цитата: по полосе 3 px на каждый уровень, зазор 14 (Н8)', () => {
  it.each([
    ['Цитата', 'cm-rz-quote-1', '17px', '0px 0px'],
    ['Вложенная', 'cm-rz-quote-2', '34px', '0px 0px, 17px 0px'],
    ['Третий уровень', 'cm-rz-quote-3', '51px', '0px 0px, 17px 0px, 34px 0px'],
  ])('%s', (fragment, cls, paddingLeft, bars) => {
    const element = line(mount(16), fragment);
    expect(element.classList.contains(cls)).toBe(true);
    const style = getComputedStyle(element);
    expect(style.paddingLeft).toBe(paddingLeft);
    expect(style.backgroundPosition).toBe(bars);
    expect(style.backgroundSize).toBe('3px 100%');
    expect(element.textContent).not.toContain('>');
  });
});

describe('списки: маркер в колонке 20, зазор 6 до текста (Н8)', () => {
  it('точка заменяет маркер вместе с пробелом: колонка 20 px и зазор 6 px', () => {
    const element = line(mount(16), 'Пункт списка');
    expect(element.textContent).toBe('Пункт списка');
    const bullet = element.querySelector('.cm-rz-bullet') as HTMLElement;
    const style = getComputedStyle(bullet);
    expect([style.width, style.marginRight]).toEqual(['20px', '6px']);
  });

  it('чекбокс 16 px по центру колонки 20 и зазор 6: поля 2 и 8 px', () => {
    const element = line(mount(16), 'Задача');
    expect(element.textContent).toBe('Задача');
    const style = getComputedStyle(element.querySelector('.cm-rz-checkbox') as HTMLElement);
    expect([style.width, style.marginLeft, style.marginRight]).toEqual(['16px', '2px', '8px']);
  });
});

describe('внешняя ссылка: «↗» акцентным цветом, без подчёркивания (Н8, вид 5Б)', () => {
  it('у внешней ссылки — стрелка с тем же адресом; у вики-ссылки и ссылки на файл — нет', () => {
    const element = line(mount(16), 'Ссылка:');
    const arrows = [...element.querySelectorAll<HTMLElement>('.cm-rz-link-external')];
    expect(arrows.map((arrow) => [arrow.textContent, arrow.getAttribute('data-rz-url'), arrow.getAttribute('aria-hidden')])).toEqual([
      ['↗', 'https://example.com/r', 'true'],
    ]);
    expect(element.textContent).toBe('Ссылка: портал регламентов↗, Встреча и файл.');
    const arrow = getComputedStyle(arrows[0] as HTMLElement);
    expect(arrow.color).toBe('var(--accent-text)');
    expect(arrow.marginLeft).toBe('4px'); // 0,25 em при 16 px — пробел, как в Э1
    const link = getComputedStyle(element.querySelector('.cm-rz-link') as HTMLElement);
    expect(link.textDecoration).toContain('none');
  });

  it('на строке с курсором стрелки нет: адрес виден как текст', () => {
    const target = mount(16);
    target.dispatch({ selection: { anchor: target.state.doc.toString().indexOf('портал') } });
    expect(target.contentDOM.querySelector('.cm-rz-link-external')).toBeNull();
  });
});

describe('блок кода без лишних полей (Н8)', () => {
  it('курсор вне блока: строки оград без высоты, остаётся отступ 12 px', () => {
    const target = mount(16);
    const fences = [...target.contentDOM.querySelectorAll<HTMLElement>('.cm-rz-codeblock-fence')];
    expect(fences).toHaveLength(2);
    for (const fence of fences) {
      const style = getComputedStyle(fence);
      expect([style.fontSize, style.lineHeight]).toEqual(['0px', '0']);
    }
    expect(getComputedStyle(fences[0] as HTMLElement).paddingTop).toBe('12px');
    expect(getComputedStyle(fences[1] as HTMLElement).paddingBottom).toBe('12px');
    expect(getComputedStyle(line(target, 'код')).paddingTop).toBe('0px');
  });

  it('курсор в блоке: ограды видны обычными строками', () => {
    const target = mount(16);
    target.dispatch({ selection: { anchor: target.state.doc.toString().indexOf('код') } });
    expect(target.contentDOM.querySelector('.cm-rz-codeblock-fence')).toBeNull();
  });
});
