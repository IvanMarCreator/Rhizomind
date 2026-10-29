// Разбор Markdown для живого предпросмотра (С3.3) — только то, что нужно для
// оформления: где разметка и что она оформляет. Текст не меняется и не
// пересобирается (ADR 0001): результат — участки строки, которые редактор
// прячет, заменяет значком или оформляет классом.
//
// Разбор построчный, по правилам CommonMark и GFM, упрощённо:
// - строка разбирается сама по себе: выделение (`*…*`), продолжающееся на
//   следующей строке абзаца, остаётся исходным текстом;
// - заголовок «текст + строка `===`/`---`» (setext), код отступом в 4 пробела,
//   картинки, таблицы, HTML не оформляются (показываются как исходный текст);
// - вики-ссылки `[[…]]` — части от `wikiPieces` (src/editor/wikilinks,
//   задача 2.6); пары `[[` → `]]` находятся одним проходом по строке.
// Блоки кода с оградой (```` ``` ````, `~~~`) и блок свойств в начале заметки
// находит `scanBlocks`: они зависят от строк выше.
import { wikiPieces } from '../wikilinks/display.ts';

/** Участок строки: смещения от начала строки. */
export interface Span {
  from: number;
  to: number;
}

/** `wiki-heading` — « › » вместо `#` в «Встреча › Итоги» (С3.4). */
export type WidgetKind = 'bullet' | 'checkbox' | 'checkbox-checked' | 'rule' | 'wiki-heading';

/**
 * Оформляемый участок. У ссылки `[текст](адрес)` — `url`; у вики-ссылки —
 * `wiki` (текст между `[[` и `]]`) и `key` (цель для разрешения ядром).
 * `link` — вся ссылка в строке, со скобками: по ней находится ссылка под
 * Ctrl+щелчком в режиме исходного текста.
 */
export interface MarkPiece extends Span {
  kind: 'mark';
  cls: string;
  url?: string;
  wiki?: string;
  key?: string;
  link?: Span;
}

/**
 * Что сделать с участком строки:
 * - `hide` — разметка: скрыть вне строки с курсором, приглушить на ней;
 * - `widget` — разметка, которую вне строки с курсором заменяет значок
 *   (точка списка, чекбокс, линия); на строке с курсором — приглушить;
 * - `mark` — оформить текст классом всегда.
 */
export type Piece =
  | ({ kind: 'hide' } & Span)
  | ({ kind: 'widget'; widget: WidgetKind } & Span)
  | MarkPiece;

export interface LineMarkup {
  /** Классы строки (заголовок, цитата, выполненная задача). */
  lineClass: string | null;
  pieces: Piece[];
}

/** Полос у вложенной цитаты не больше этого: глубже — как у шестого уровня (классы `cm-rz-quote-1…6`). */
export const MAX_QUOTE_BARS = 6;

/** Строки длиннее не разбираются внутри: так оформление строки не дольше нескольких миллисекунд. */
export const MAX_INLINE_LINE = 10_000;

const QUOTE = /^ {0,3}> ?/;
/** Цитата с позиции `lastIndex` (вложенные — подряд, без копирования строки). */
const QUOTE_AT = / {0,3}> ?/y;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const HEADING = /^( {0,3})(#{1,6})(?:[ \t]+|$)/;
const LIST = /^([ \t]*)([-*+]|\d{1,9}[.)])(?=[ \t]|$)/;
const TASK = /^[ \t]+\[([ xX])\](?=[ \t]|$)/;

/** Пустая строка, заголовок или линия выше: `---` под ней — линия, а не заголовок setext. */
export function breaksParagraph(line: string | null): boolean {
  if (line === null || line.trim() === '') return true;
  const text = line.replace(QUOTE, '');
  return HEADING.test(text) || RULE.test(text);
}

/**
 * Разметка одной строки вне блока кода.
 * `previous` — строка выше (`null` — первая строка или сразу после блока).
 */
export function parseLine(text: string, previous: string | null): LineMarkup {
  const pieces: Piece[] = [];
  const classes: string[] = [];
  let pos = 0;

  // Цитата, в том числе вложенная: `> > текст` — по полосе на уровень.
  let depth = 0;
  for (;;) {
    QUOTE_AT.lastIndex = pos;
    const m = QUOTE_AT.exec(text);
    if (!m) break;
    const indent = m[0].indexOf('>');
    pieces.push({ kind: 'hide', from: pos + indent, to: pos + m[0].length });
    pos += m[0].length;
    depth++;
  }
  if (depth > 0) classes.push(`cm-rz-quote cm-rz-quote-${Math.min(depth, MAX_QUOTE_BARS)}`);
  const rest = text.slice(pos);

  const rule = RULE.exec(rest);
  if (rule && (rule[1] !== '-' || breaksParagraph(previous))) {
    const from = pos + rest.length - rest.trimStart().length;
    const to = pos + rest.trimEnd().length;
    pieces.push({ kind: 'widget', widget: 'rule', from, to });
    return { lineClass: joinClasses(classes), pieces };
  }

  let contentFrom = pos;
  let contentTo = text.length;

  const heading = HEADING.exec(rest);
  const list = heading ? null : LIST.exec(rest);
  if (heading) {
    const level = heading[2]?.length ?? 1;
    pieces.push({ kind: 'hide', from: pos + (heading[1]?.length ?? 0), to: pos + heading[0].length });
    contentFrom = pos + heading[0].length;
    const close = headingClose(text, contentFrom);
    if (close) {
      contentTo = close.from;
      pieces.push({ kind: 'hide', from: close.from, to: close.to });
    }
    classes.push(`cm-rz-heading cm-rz-h${level}`);
  } else if (list) {
    const markerFrom = pos + (list[1]?.length ?? 0);
    const marker = list[2] ?? '';
    const markerTo = markerFrom + marker.length;
    const ordered = /\d/.test(marker);
    const task = TASK.exec(text.slice(markerTo));
    if (task) {
      const boxFrom = markerTo + task[0].length - 3;
      const checked = task[1] !== ' ';
      if (ordered) pieces.push({ kind: 'mark', cls: 'cm-rz-list-number', from: markerFrom, to: markerTo });
      else pieces.push({ kind: 'hide', from: markerFrom, to: boxFrom });
      // Значок с пробелом за ним: зазор до текста задаёт значок (tokens.md «Списки»).
      const boxTo = boxFrom + 3 + (isBlank(text[boxFrom + 3]) ? 1 : 0);
      pieces.push({ kind: 'widget', widget: checked ? 'checkbox-checked' : 'checkbox', from: boxFrom, to: boxTo });
      if (checked) classes.push('cm-rz-task-done');
      contentFrom = boxTo;
    } else if (ordered) {
      pieces.push({ kind: 'mark', cls: 'cm-rz-list-number', from: markerFrom, to: markerTo });
      contentFrom = markerTo;
    } else {
      const bulletTo = markerTo + (isBlank(text[markerTo]) ? 1 : 0);
      pieces.push({ kind: 'widget', widget: 'bullet', from: markerFrom, to: bulletTo });
      contentFrom = bulletTo;
    }
  }

  if (text.length <= MAX_INLINE_LINE) parseInline(inlineContext(text, contentFrom, contentTo), contentFrom, contentTo, pieces);
  return { lineClass: joinClasses(classes), pieces };
}

const isBlank = (ch: string | undefined) => ch === ' ' || ch === '\t';

/**
 * Закрывающие решётки заголовка `## текст ##` после `from`: пробелы, `#`,
 * пробелы до конца строки. `from` — начало пробелов перед решётками, `to` —
 * конец решёток. Проход с конца строки — без возвратов регулярного выражения.
 */
function headingClose(text: string, from: number): Span | null {
  let end = text.length;
  while (end > from && isBlank(text[end - 1])) end--;
  let hashes = end;
  while (hashes > from && text[hashes - 1] === '#') hashes--;
  if (hashes === end) return null;
  let spaces = hashes;
  while (spaces > from && isBlank(text[spaces - 1])) spaces--;
  return spaces < hashes ? { from: spaces, to: end } : null;
}

function joinClasses(classes: string[]): string | null {
  return classes.length > 0 ? classes.join(' ') : null;
}

// ---- Разметка внутри строки -------------------------------------------------

const ASCII_PUNCT = /[!-/:-@[-`{-~]/;
const PUNCT = /[\p{P}\p{S}]/u;
const SPACE = /\s/u;

interface Delimiter {
  char: '*' | '_' | '~';
  /** Начало и конец ещё не использованной части серии. */
  from: number;
  to: number;
  /** Длина серии до сопоставления (правило «кратно трём»). */
  length: number;
  canOpen: boolean;
  canClose: boolean;
}

/**
 * Общие для строки результаты поиска (задача 2.5а): каждый поиск вперёд
 * по строке делается один раз, а не от каждого символа `[`, `` ` `` или
 * `(` заново — так разбор строки линеен, а не квадратичен. Всё считается до
 * границы `to` разбора строки; вложенный разбор (текст ссылки) кончается на
 * `]`, поэтому найденное дальше своей границы он считает ненайденным.
 */
interface InlineContext {
  text: string;
  /** Участок строки, который разбирается (без разметки начала строки и закрывающих `#`). */
  from: number;
  to: number;
  /** `[` → парная `]` с учётом вложенности и `\` (как при поиске от каждой `[`). */
  brackets: Map<number, number> | null;
  /** Где кончается адрес ссылки, начатый в позиции: пробел или `)` без пары. */
  destinationEnd: Int32Array | null;
  /** Последний поиск по символу: от `from` первое вхождение — `at` (`to` — не найдено). */
  next: Map<string, { from: number; at: number }>;
  /** `[[` → её `]]` (вики-ссылки строки). */
  wiki: Map<number, number> | null;
}

function inlineContext(text: string, from: number, to: number): InlineContext {
  return { text, from, to, brackets: null, destinationEnd: null, next: new Map(), wiki: null };
}

/**
 * Код в строке, ссылки и выделение на участке `[from, to)` строки.
 * Код и ссылки разбираются первыми: внутри кода разметки нет, выделение не
 * пересекает границу ссылки.
 */
function parseInline(ctx: InlineContext, from: number, to: number, out: Piece[]): void {
  const { text } = ctx;
  const delimiters: Delimiter[] = [];
  let i = from;
  while (i < to) {
    const ch = text[i] ?? '';
    if (ch === '\\' && i + 1 < to && ASCII_PUNCT.test(text[i + 1] ?? '')) {
      i += 2;
    } else if (ch === '`') {
      i = codeSpan(ctx, i, to, out);
    } else if (ch === '[') {
      i = link(ctx, i, to, out);
    } else if (ch === '*' || ch === '_' || ch === '~') {
      let end = i;
      while (end < to && text[end] === ch) end++;
      if (ch !== '~' || end - i === 2) delimiters.push(delimiter(text, ch, i, end));
      i = end;
    } else {
      i++;
    }
  }
  emphasis(delimiters, out);
}

/**
 * Первое вхождение `target` с позиции `from` (`ctx.to` — нет). С `escapes`
 * символ после `\\` пропускается. Повторный поиск с позиции до найденного
 * прежде отвечает сразу: поиск начинается после открывающего символа, так
 * что пары «`\\` + символ» те же.
 */
function nextOf(ctx: InlineContext, target: string, from: number, escapes: boolean): number {
  const key = escapes ? `\\${target}` : target;
  const memo = ctx.next.get(key);
  if (memo && from >= memo.from && from <= memo.at) return memo.at;
  let i = from;
  while (i < ctx.to && ctx.text[i] !== target) i += escapes && ctx.text[i] === '\\' ? 2 : 1;
  const at = Math.min(i, ctx.to);
  ctx.next.set(key, { from, at });
  return at;
}

/** Код в строке: серия обратных апострофов и такая же закрывающая. Возвращает, где продолжать. */
function codeSpan(ctx: InlineContext, start: number, to: number, out: Piece[]): number {
  const { text } = ctx;
  let openEnd = start;
  while (openEnd < to && text[openEnd] === '`') openEnd++;
  const close = closingRun(ctx, openEnd, openEnd - start);
  if (close >= to) return openEnd;
  const end = close + (openEnd - start);
  out.push({ kind: 'hide', from: start, to: openEnd });
  if (close > openEnd) out.push({ kind: 'mark', cls: 'cm-rz-code', from: openEnd, to: close });
  out.push({ kind: 'hide', from: close, to: end });
  return end;
}

/**
 * Начало первой серии обратных апострофов длины `length` после `from` (конец
 * серии); `ctx.to` — нет. Серии одни и те же, откуда ни искать, поэтому
 * результат прошлого поиска той же длины годится для любого места до него.
 */
function closingRun(ctx: InlineContext, from: number, length: number): number {
  const key = `\`${length}`;
  const memo = ctx.next.get(key);
  if (memo && from >= memo.from && from <= memo.at) return memo.at;
  const { text, to } = ctx;
  let i = from;
  let at = to;
  while (i < to) {
    if (text[i] !== '`') {
      i++;
      continue;
    }
    let end = i;
    while (end < to && text[end] === '`') end++;
    if (end - i === length) {
      at = i;
      break;
    }
    i = end;
  }
  ctx.next.set(key, { from, at });
  return at;
}

/** Пары `[` → `]` во всей строке: стек вместо поиска от каждой `[`. */
function brackets(ctx: InlineContext): Map<number, number> {
  if (ctx.brackets) return ctx.brackets;
  const pairs = new Map<number, number>();
  const stack: number[] = [];
  const { text } = ctx;
  for (let i = ctx.from; i < ctx.to; i++) {
    const ch = text[i];
    if (ch === '\\') i++;
    else if (ch === '[') stack.push(i);
    else if (ch === ']') {
      const open = stack.pop();
      if (open !== undefined) pairs.set(open, i);
    }
  }
  ctx.brackets = pairs;
  return pairs;
}

/**
 * Пары `[[` → `]]` во всей строке за один проход, по правилам `findWikilinks`
 * (src/core/wikilinks): ссылку открывает последняя `[[` перед первой `]]`.
 * Внутри уже открытой ссылки `[[` ищется с каждой позиции (`[[[[a]]` — `[[a]]`).
 */
function wikiPairs(ctx: InlineContext): Map<number, number> {
  if (ctx.wiki) return ctx.wiki;
  const pairs = new Map<number, number>();
  const { text } = ctx;
  let open = -1;
  for (let i = ctx.from; i + 1 < ctx.to; ) {
    if (text[i] === '[' && text[i + 1] === '[') {
      const fresh = open < 0;
      open = i;
      i += fresh ? 2 : 1;
    } else if (open >= 0 && text[i] === ']' && text[i + 1] === ']') {
      pairs.set(open, i);
      open = -1;
      i += 2;
    } else {
      i++;
    }
  }
  ctx.wiki = pairs;
  return pairs;
}

/** Код в строке начинается внутри `[from, to)`: серия обратных апострофов, у которой есть пара дальше. */
function codeStartsIn(ctx: InlineContext, from: number, to: number): boolean {
  let at = nextOf(ctx, '`', from, false);
  while (at < to) {
    let end = at;
    while (end < ctx.to && ctx.text[end] === '`') end++;
    if (closingRun(ctx, end, end - at) < ctx.to) return true;
    at = nextOf(ctx, '`', end, false);
  }
  return false;
}

/** Вики-ссылка `[[…]]` с позиции `start`. Возвращает, где продолжать. */
function wikiLink(ctx: InlineContext, start: number, to: number, out: Piece[]): number {
  const close = wikiPairs(ctx).get(start);
  if (close === undefined || close + 2 > to || codeStartsIn(ctx, start + 2, close)) return start + 2;
  // Встраивание `![[…]]` — выпуск 0.3: исходный текст.
  const embed = start > ctx.from && ctx.text[start - 1] === '!';
  const pieces = embed ? null : wikiPieces(ctx.text, start, close);
  if (pieces) out.push(...pieces);
  return close + 2;
}

/** Ссылка `[текст](адрес "заголовок")`. Возвращает, где продолжать. */
function link(ctx: InlineContext, start: number, to: number, out: Piece[]): number {
  const { text } = ctx;
  if (text[start + 1] === '[') return wikiLink(ctx, start, to, out);
  // Картинка `![…](…)` — выпуск 0.3: исходный текст.
  if (start > 0 && text[start - 1] === '!' && text[start - 2] !== '\\') return start + 1;

  const close = brackets(ctx).get(start) ?? -1;
  if (close < 0 || close >= to || text[close + 1] !== '(') return start + 1;

  const target = destination(ctx, close + 2, to);
  if (!target) return start + 1;
  out.push({ kind: 'hide', from: start, to: start + 1 });
  if (close > start + 1) {
    out.push({ kind: 'mark', cls: 'cm-rz-link', url: target.url, link: { from: start, to: target.end }, from: start + 1, to: close });
    parseInline(ctx, start + 1, close, out);
  }
  out.push({ kind: 'hide', from: close, to: target.end });
  return target.end;
}

/**
 * Для каждой позиции — где кончится адрес ссылки, начатый в ней: первый
 * пробел или первая `)`, у которой нет пары `(` после начала (символ после
 * `\\` не в счёт). Два прохода по строке вместо прохода от каждой ссылки.
 */
function destinationEnds(ctx: InlineContext): Int32Array {
  if (ctx.destinationEnd) return ctx.destinationEnd;
  const { text, from, to } = ctx;
  const ends = new Int32Array(to + 1).fill(to);
  // Баланс скобок перед каждой позицией и символы, пропущенные после `\\`.
  const balance = new Int32Array(to + 1);
  const escaped = new Uint8Array(to + 1);
  let depth = 0;
  for (let i = from; i < to; i++) {
    balance[i] = depth;
    const ch = text[i];
    if (ch === '\\' && i + 1 < to) {
      escaped[i] = 1;
      escaped[i + 1] = 1;
      balance[i + 1] = depth;
      i++;
    } else if (ch === '(') depth++;
    else if (ch === ')') depth--;
  }
  let space = to;
  const closeAt = new Map<number, number>();
  for (let i = to - 1; i >= from; i--) {
    const ch = text[i] ?? '';
    if (!escaped[i]) {
      if (SPACE.test(ch)) space = i;
      else if (ch === ')') closeAt.set(balance[i] ?? 0, i);
    }
    ends[i] = Math.min(space, closeAt.get(balance[i] ?? 0) ?? to);
  }
  ctx.destinationEnd = ends;
  return ends;
}

/** Адрес и необязательный заголовок ссылки до `)`. */
function destination(ctx: InlineContext, start: number, to: number): { url: string; end: number } | null {
  const { text } = ctx;
  let i = skipSpaces(text, start, to);
  let url: string;
  if (text[i] === '<') {
    const close = nextOf(ctx, '>', i + 1, false);
    if (close >= to) return null;
    url = text.slice(i + 1, close);
    if (url.includes('<')) return null;
    i = close + 1;
  } else {
    const begin = i;
    i = Math.min(destinationEnds(ctx)[i] ?? to, to);
    url = text.slice(begin, i);
  }
  const afterUrl = i;
  i = skipSpaces(text, i, to);
  const quote = text[i];
  if (i > afterUrl && (quote === '"' || quote === "'" || quote === '(')) {
    const j = nextOf(ctx, quote === '(' ? ')' : quote, i + 1, true);
    if (j >= to) return null;
    i = skipSpaces(text, j + 1, to);
  }
  if (text[i] !== ')') return null;
  return { url, end: i + 1 };
}

function skipSpaces(text: string, i: number, to: number): number {
  while (i < to && (text[i] === ' ' || text[i] === '\t')) i++;
  return i;
}

function isSpaceAt(text: string, i: number): boolean {
  return i < 0 || i >= text.length || SPACE.test(charAt(text, i));
}

function isPunctAt(text: string, i: number): boolean {
  return i >= 0 && i < text.length && PUNCT.test(charAt(text, i));
}

/** Символ целиком (с суррогатной парой), который начинается или кончается в `i`. */
function charAt(text: string, i: number): string {
  const code = text.charCodeAt(i);
  if (code >= 0xdc00 && code <= 0xdfff && i > 0) return text.slice(i - 1, i + 1);
  if (code >= 0xd800 && code <= 0xdbff) return text.slice(i, i + 2);
  return text[i] ?? '';
}

/** Серия `*`, `_` или `~~`: может ли открывать и закрывать выделение (CommonMark 6.2). */
function delimiter(text: string, char: '*' | '_' | '~', from: number, to: number): Delimiter {
  const before = from - 1;
  const after = to;
  const left = !isSpaceAt(text, after) && (!isPunctAt(text, after) || isSpaceAt(text, before) || isPunctAt(text, before));
  const right = !isSpaceAt(text, before) && (!isPunctAt(text, before) || isSpaceAt(text, after) || isPunctAt(text, after));
  let canOpen = left;
  let canClose = right;
  if (char === '_') {
    canOpen = left && (!right || isPunctAt(text, before));
    canClose = right && (!left || isPunctAt(text, after));
  }
  return { char, from, to, length: to - from, canOpen, canClose };
}

const EMPHASIS_CLASS = { 1: 'cm-rz-em', 2: 'cm-rz-strong' } as const;

/**
 * Сопоставление серий (CommonMark, «process emphasis»). Линейно, как в
 * образце CommonMark: израсходованные серии пропускаются по ссылкам вниз
 * (со сжатием пути), а после неудачного поиска для закрывающих того же вида
 * запоминается нижняя граница (`openers_bottom`).
 */
function emphasis(delimiters: Delimiter[], out: Piece[]): void {
  const below = delimiters.map((_d, k) => k - 1);
  const used = (k: number) => {
    const d = delimiters[k];
    return !d || d.to === d.from;
  };
  /** Ближайшая неизрасходованная серия с номером ≤ `k` (`-1` — нет). */
  const live = (k: number): number => {
    let root = k;
    while (root >= 0 && used(root)) root = below[root] ?? -1;
    while (k > root) {
      const next = below[k] ?? -1;
      below[k] = root;
      k = next;
    }
    return root;
  };
  const bottom = new Map<string, number>();

  for (let c = 0; c < delimiters.length; c++) {
    const closer = delimiters[c];
    if (!closer?.canClose) continue;
    const key = `${closer.char}${closer.canOpen ? 1 : 0}${closer.length % 3}`;
    const floor = bottom.get(key) ?? -1;
    for (let o = live(c - 1); o > floor && closer.to > closer.from; ) {
      const opener = delimiters[o];
      if (
        !opener ||
        opener.char !== closer.char ||
        !opener.canOpen ||
        (closer.char !== '~' &&
          (opener.canClose || closer.canOpen) &&
          (opener.length + closer.length) % 3 === 0 &&
          !(opener.length % 3 === 0 && closer.length % 3 === 0))
      ) {
        o = live(o - 1);
        continue;
      }
      const use = closer.char === '~' ? 2 : Math.min(2, opener.to - opener.from, closer.to - closer.from) as 1 | 2;
      const cls = closer.char === '~' ? 'cm-rz-strike' : EMPHASIS_CLASS[use];
      const openMarker = opener.to - use;
      const closeMarker = closer.from + use;
      out.push({ kind: 'hide', from: openMarker, to: opener.to });
      if (closer.from > opener.to) out.push({ kind: 'mark', cls, from: opener.to, to: closer.from });
      out.push({ kind: 'hide', from: closer.from, to: closeMarker });
      opener.to = openMarker;
      closer.from = closeMarker;
      // Серии между парой больше ни с чем не сопоставляются.
      for (let k = live(c - 1); k > o; k = live(k - 1)) {
        const between = delimiters[k];
        if (between) between.to = between.from;
      }
      // Остаток открывающей серии может закрыться этой же серией: начать с неё снова.
      o = live(o);
    }
    if (closer.to > closer.from) bottom.set(key, c - 1);
  }
}

// ---- Блоки, которые зависят от строк выше --------------------------------

/** Блок кода с оградой: позиции в тексте. */
export interface CodeBlock {
  /** Начало строки открывающей ограды. */
  from: number;
  /** Конец закрывающей ограды или конец текста, если блок не закрыт. */
  to: number;
  /** Начало строки закрывающей ограды; `null` — блок не закрыт. */
  closeLine: number | null;
}

export interface Blocks {
  code: CodeBlock[];
  /** Блок свойств `---` … `---` в начале заметки (оформление — выпуск 0.2). */
  properties: Span | null;
}

const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
export const PROPERTIES_OPEN = /^---[ \t]*$/;
export const PROPERTIES_CLOSE = /^(?:---|\.\.\.)[ \t]*$/;

/**
 * Текст больше этого (в символах) — блоки кода и блок свойств не размечаются,
 * только построчное оформление видимой части (решение 77): разбор всего
 * текста при открытии заметки 100 МБ втрое медленнее самого открытия.
 */
export const MAX_BLOCK_TEXT = 10 * 1024 * 1024;

/** Строка похожа на ограду (```` ``` ```` или `~~~` с отступом до 3 пробелов). */
export function isFenceLike(text: string): boolean {
  let i = 0;
  while (i < 3 && text.charCodeAt(i) === 32) i++;
  const ch = text[i];
  return (ch === '`' || ch === '~') && text[i + 1] === ch && text[i + 2] === ch;
}

/** Открытая ограда: начало строки и маркер, который её закроет. */
export interface OpenFence {
  from: number;
  char: string;
  length: number;
}

/**
 * Шаг разбора оград: состояние после строки `text` (начало `from`, конец
 * `to`) при состоянии `open` перед ней. Закрытый строкой блок — в `closed`.
 */
export function fenceStep(open: OpenFence | null, text: string, from: number, to: number, closed: CodeBlock[]): OpenFence | null {
  if (!isFenceLike(text)) return open;
  const fence = FENCE.exec(text);
  if (!fence) return open;
  const marker = fence[1] ?? '';
  const info = fence[2] ?? '';
  if (!open) {
    const char = marker[0] ?? '`';
    if (char === '`' && info.includes('`')) return null;
    return { from, char, length: marker.length };
  }
  if (marker[0] === open.char && marker.length >= open.length && info.trim() === '') {
    closed.push({ from: open.from, to, closeLine: from });
    return null;
  }
  return open;
}

/** Ограда, открытая строкой с началом `from` (строка обязана быть оградой). */
export function openFenceAt(text: string, from: number): OpenFence {
  const marker = FENCE.exec(text)?.[1] ?? '```';
  return { from, char: marker[0] ?? '`', length: marker.length };
}

export interface LineSource {
  readonly length: number;
  readonly lines: number;
  line(n: number): { from: number; to: number; text: string };
}

/** Блок свойств в начале текста (`null` — его нет). */
export function findProperties(doc: LineSource): Span | null {
  return propertiesBlock(doc)?.span ?? null;
}

/** Блок свойств: первая строка `---`, конец — первая строка `---` или `...` ниже; `next` — номер строки после него. */
function propertiesBlock(doc: LineSource): { span: Span; next: number } | null {
  const first = doc.line(1);
  if (doc.lines < 2 || !PROPERTIES_OPEN.test(first.text)) return null;
  for (let k = 2; k <= doc.lines; k++) {
    const line = doc.line(k);
    if (PROPERTIES_CLOSE.test(line.text)) return { span: { from: first.from, to: line.to }, next: k + 1 };
  }
  return null;
}

/** Найти блоки кода и блок свойств во всём тексте. */
export function scanBlocks(doc: LineSource): Blocks {
  const code: CodeBlock[] = [];
  if (doc.length > MAX_BLOCK_TEXT) return { code, properties: null };
  const properties = propertiesBlock(doc);
  let open: OpenFence | null = null;
  for (let n = properties?.next ?? 1; n <= doc.lines; n++) {
    const line = doc.line(n);
    open = fenceStep(open, line.text, line.from, line.to, code);
  }
  if (open) code.push({ from: open.from, to: doc.length, closeLine: null });
  return { code, properties: properties?.span ?? null };
}
