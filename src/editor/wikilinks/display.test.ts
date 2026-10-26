// Отображение вики-ссылок в живом предпросмотре (С3.4, вид 5Б): ссылка с
// путём показывает только имя, ссылка на заголовок — «Имя › Заголовок»,
// псевдоним — только псевдоним. На строке с курсором разметка видна
// приглушённо. Проверяется по оформлению состояния редактора, как в
// live-preview/preview.test.ts.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { EditorSelection, type EditorState } from '@codemirror/state';
import type { Decoration } from '@codemirror/view';
import { describe, expect, it } from 'vitest';
import { findWikilinks, type WikiLink } from '../../core/wikilinks/index.ts';
import { createEditorState } from '../codemirror.ts';
import { previewDecorations } from '../live-preview/index.ts';
import { parseLine } from '../live-preview/markdown.ts';
import { resolvedEffect } from './resolve.ts';

function stateWithCursor(marked: string): EditorState {
  // Курсор — `¦`: `|` занят псевдонимом.
  const at = marked.indexOf('¦');
  const text = marked.slice(0, at) + marked.slice(at + 1);
  return createEditorState(text, { mode: 'live_preview' }).update({ selection: EditorSelection.cursor(at) }).state;
}

const spec = (value: Decoration) => value.spec as { rz?: string; class?: string; attributes?: Record<string, string>; widget?: unknown };
const isReplace = (value: Decoration) => spec(value).rz === 'hide' || spec(value).rz === 'widget';

/** Что видно на строке: скрытое убрано, значки — своим текстом. */
function visible(state: EditorState, line = 2): string {
  const { from, to } = state.doc.line(line);
  const cuts: { from: number; to: number; text: string }[] = [];
  previewDecorations(state).between(from, to, (a, b, value) => {
    if (a < b && isReplace(value)) {
      const widget = spec(value).widget as { text?: string } | undefined;
      cuts.push({ from: a, to: b, text: widget?.text ?? '' });
    }
  });
  cuts.sort((x, y) => x.from - y.from);
  let out = '';
  let pos = from;
  for (const cut of cuts) {
    out += state.sliceDoc(pos, cut.from) + cut.text;
    pos = cut.to;
  }
  return out + state.sliceDoc(pos, to);
}

/** Участки, оформленные как вики-ссылка: `класс: текст → data-rz-wiki`. */
function wikiMarks(state: EditorState): string[] {
  const out: string[] = [];
  previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
    const attrs = spec(value).attributes;
    if (from < to && attrs?.['data-rz-wiki'] !== undefined) {
      out.push(`${spec(value).class ?? ''}: ${state.sliceDoc(from, to)} → ${attrs['data-rz-wiki']}`);
    }
  });
  return out;
}

const away = (line: string) => stateWithCursor(`¦курсор\n${line}`);
const on = (line: string) => stateWithCursor(`курсор\n${line}¦`);

describe('вики-ссылка вне строки с курсором (С3.4, вид 5Б)', () => {
  it.each([
    ['имя', 'См. [[Встреча]].', 'См. Встреча.'],
    ['путь — только имя', 'См. [[Альфа/Встреча]].', 'См. Встреча.'],
    ['от корня — только имя', 'См. [[/Встреча]].', 'См. Встреча.'],
    ['полный путь от корня', '[[/Проекты/Альфа/Встреча]]', 'Встреча'],
    ['заголовок — «Имя › Заголовок»', '[[Встреча#Итоги]]', 'Встреча › Итоги'],
    ['путь и заголовок', '[[Альфа/Встреча#Итоги]]', 'Встреча › Итоги'],
    ['псевдоним — только он', 'после [[Встреча|встречи]] —', 'после встречи —'],
    ['псевдоним с путём и заголовком', '[[Альфа/Встреча#Итоги|итогов]]', 'итогов'],
    ['псевдоним в таблице `\\|`', '| [[Встреча\\|встречи]] |', '| встречи |'],
    ['пустой псевдоним — имя', '[[Встреча| ]]', 'Встреча'],
    ['пустой заголовок — имя', '[[Встреча#]]', 'Встреча'],
    ['расширение .md не показывается', '[[Альфа/Встреча.md]]', 'Встреча'],
    ['пробелы внутри скобок', '[[ Встреча ]]', 'Встреча'],
    ['две ссылки', '[[А]] и [[Б|бэ]]', 'А и бэ'],
    ['рядом с оформлением', '**жирно** [[Встреча]] *курсив*', 'жирно Встреча курсив'],
  ])('%s', (_name, line, shown) => {
    expect(visible(away(line))).toBe(shown);
  });

  it('оформление и цель для щелчка — текст между скобками', () => {
    expect(wikiMarks(away('[[Альфа/Встреча#Итоги|итогов]] и [[План]]'))).toEqual([
      'cm-rz-wikilink: итогов → Альфа/Встреча#Итоги|итогов',
      'cm-rz-wikilink: План → План',
    ]);
    expect(wikiMarks(away('[[Встреча#Итоги]]'))).toEqual(['cm-rz-wikilink: Встреча#Итоги → Встреча#Итоги']);
  });

  it.each([
    ['якорь внутри заметки — обычный текст (С3.4)', '[[#Заголовок]]'],
    ['пустая', '[[]]'],
    ['выход из папки', '[[../Секрет]]'],
    ['путь Windows', '[[C:\\Windows\\win.ini]]'],
    ['длинный путь Windows', '[[\\\\?\\C:\\x]]'],
    ['UNC', '[[//server/share/x]]'],
    ['адрес file:', '[[file:///C:/x]]'],
    ['встраивание — выпуск 0.3', '![[Схема.png]]'],
    ['в коде в строке', '`[[Встреча]]`'],
    ['не закрыта', '[[Встреча'],
  ])('не ссылка: %s', (_name, line) => {
    expect(wikiMarks(away(line))).toEqual([]);
  });

  it('в блоке кода — не ссылка', () => {
    expect(wikiMarks(stateWithCursor('¦курсор\n```\n[[Встреча]]\n```\n'))).toEqual([]);
  });
});

describe('вики-ссылка на строке с курсором', () => {
  it('разметка видна приглушённо, ссылка оформлена', () => {
    const state = on('См. [[Альфа/Встреча#Итоги|итогов]].');
    expect(visible(state)).toBe('См. [[Альфа/Встреча#Итоги|итогов]].');
    expect(wikiMarks(state)).toEqual(['cm-rz-wikilink: итогов → Альфа/Встреча#Итоги|итогов']);
    const faint: string[] = [];
    previewDecorations(state).between(0, state.doc.length, (from, to, value) => {
      if (spec(value).class === 'cm-rz-markup') faint.push(state.sliceDoc(from, to));
    });
    expect(faint).toEqual(['[[Альфа/Встреча#Итоги|', ']]']);
  });

  it('заголовок: на строке с курсором `#` виден как есть', () => {
    expect(visible(on('[[Встреча#Итоги]]'))).toBe('[[Встреча#Итоги]]');
  });
});

describe('неразрешённая ссылка выглядит иначе', () => {
  it('ответ ядра «неразрешена» — класс cm-rz-wikilink-unresolved; разрешённая — без него', () => {
    const state = away('[[Есть]] и [[Нет]] и [[/Альфа/Есть]]').update({
      effects: resolvedEffect.of(new Map([['Есть', true], ['Нет', false], ['/Альфа/Есть', true]])),
    }).state;
    expect(wikiMarks(state)).toEqual([
      'cm-rz-wikilink: Есть → Есть',
      'cm-rz-wikilink cm-rz-wikilink-unresolved: Нет → Нет',
      'cm-rz-wikilink: Есть → /Альфа/Есть',
    ]);
  });

  it('ответа ещё нет — ссылка выглядит разрешённой (без мигания)', () => {
    expect(wikiMarks(away('[[Нет]]'))).toEqual(['cm-rz-wikilink: Нет → Нет']);
  });
});

// Разбор строки в предпросмотре и разбор `findWikilinks` (общие образцы
// text.json) находят одни и те же ссылки в однострочном тексте.
interface TextCase {
  text: string;
  note: string;
  links: WikiLink[];
}
const FIXTURES = join(import.meta.dirname, '..', '..', '..', 'tests', 'fixtures', 'wikilinks');
const textCases = (JSON.parse(readFileSync(join(FIXTURES, 'text.json'), 'utf8')) as { cases: TextCase[] }).cases;

/** Ссылки строки по разбору предпросмотра: `[from, to)` всей ссылки. */
function previewLinks(line: string): [number, number][] {
  return parseLine(line, null)
    .pieces.flatMap((piece) => (piece.kind === 'mark' && piece.wiki !== undefined && piece.link ? [[piece.link.from, piece.link.to] as [number, number]] : []));
}

/** Ссылки по общему разбору: допустимые, не встраивания. */
function coreLinks(line: string): [number, number][] {
  return findWikilinks(line)
    .filter((link) => !link.embed && !('invalid' in link.target))
    .map((link) => [link.from, link.to]);
}

describe('разбор предпросмотра совпадает с общим разбором ссылок', () => {
  const single = textCases.filter((c) => !/[\n\r]/.test(c.text) && !/^( {4}|\t|```|~~~)/.test(c.text) && !c.text.includes('\\'));

  it('образцов в одну строку достаточно', () => {
    expect(single.length).toBeGreaterThanOrEqual(8);
  });

  it.each(single.map((c) => [c.note, c.text] as const))('%s', (_note, text) => {
    expect(previewLinks(text)).toEqual(coreLinks(text));
  });

  it('случайные строки', () => {
    const alphabet = ['[', '[', ']', ']', '`', '|', '#', '/', '!', ' ', 'а', 'b', '*'];
    let seed = 7;
    const next = (): number => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed;
    };
    for (let n = 0; n < 3000; n += 1) {
      let text = '';
      for (let i = next() % 40; i > 0; i -= 1) text += alphabet[next() % alphabet.length];
      expect(previewLinks(text), JSON.stringify(text)).toEqual(coreLinks(text));
    }
  });
});
