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
// - вики-ссылки `[[…]]` пропускаются — их оформление в задаче 2.6.
// Блоки кода с оградой (```` ``` ````, `~~~`) и блок свойств в начале заметки
// находит `scanBlocks`: они зависят от строк выше.

/** Участок строки: смещения от начала строки. */
export interface Span {
  from: number;
  to: number;
}

export type WidgetKind = 'bullet' | 'checkbox' | 'checkbox-checked' | 'rule';

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
  | ({ kind: 'mark'; cls: string; url?: string } & Span);

export interface LineMarkup {
  /** Классы строки (заголовок, цитата, выполненная задача). */
  lineClass: string | null;
  pieces: Piece[];
}

/** Строки длиннее не разбираются внутри: разбор выделения — до O(n²). */
export const MAX_INLINE_LINE = 10_000;

const QUOTE = /^ {0,3}> ?/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const HEADING = /^( {0,3})(#{1,6})(?:[ \t]+|$)/;
const HEADING_CLOSE = /[ \t]+#+[ \t]*$/;
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

  // Цитата, в том числе вложенная: `> > текст`.
  for (let m = QUOTE.exec(text); m; m = QUOTE.exec(text.slice(pos))) {
    const indent = m[0].indexOf('>');
    pieces.push({ kind: 'hide', from: pos + indent, to: pos + m[0].length });
    pos += m[0].length;
    if (!classes.includes('cm-rz-quote')) classes.push('cm-rz-quote');
  }
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
    const close = HEADING_CLOSE.exec(text.slice(contentFrom));
    if (close) {
      contentTo = contentFrom + close.index;
      pieces.push({ kind: 'hide', from: contentTo, to: contentTo + close[0].trimEnd().length });
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
      pieces.push({ kind: 'widget', widget: checked ? 'checkbox-checked' : 'checkbox', from: boxFrom, to: boxFrom + 3 });
      if (checked) classes.push('cm-rz-task-done');
      contentFrom = boxFrom + 3;
    } else if (ordered) {
      pieces.push({ kind: 'mark', cls: 'cm-rz-list-number', from: markerFrom, to: markerTo });
      contentFrom = markerTo;
    } else {
      pieces.push({ kind: 'widget', widget: 'bullet', from: markerFrom, to: markerTo });
      contentFrom = markerTo;
    }
  }

  if (text.length <= MAX_INLINE_LINE) parseInline(text, contentFrom, contentTo, pieces);
  return { lineClass: joinClasses(classes), pieces };
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
 * Код в строке, ссылки и выделение на участке `[from, to)` строки `text`.
 * Код и ссылки разбираются первыми: внутри кода разметки нет, выделение не
 * пересекает границу ссылки.
 */
function parseInline(text: string, from: number, to: number, out: Piece[]): void {
  const delimiters: Delimiter[] = [];
  let i = from;
  while (i < to) {
    const ch = text[i] ?? '';
    if (ch === '\\' && i + 1 < to && ASCII_PUNCT.test(text[i + 1] ?? '')) {
      i += 2;
    } else if (ch === '`') {
      i = codeSpan(text, i, to, out);
    } else if (ch === '[') {
      i = link(text, i, to, out);
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

/** Код в строке: серия обратных апострофов и такая же закрывающая. Возвращает, где продолжать. */
function codeSpan(text: string, start: number, to: number, out: Piece[]): number {
  let openEnd = start;
  while (openEnd < to && text[openEnd] === '`') openEnd++;
  const length = openEnd - start;
  let i = openEnd;
  while (i < to) {
    if (text[i] !== '`') {
      i++;
      continue;
    }
    let end = i;
    while (end < to && text[end] === '`') end++;
    if (end - i === length) {
      out.push({ kind: 'hide', from: start, to: openEnd });
      if (i > openEnd) out.push({ kind: 'mark', cls: 'cm-rz-code', from: openEnd, to: i });
      out.push({ kind: 'hide', from: i, to: end });
      return end;
    }
    i = end;
  }
  return openEnd;
}

/** Ссылка `[текст](адрес "заголовок")`. Возвращает, где продолжать. */
function link(text: string, start: number, to: number, out: Piece[]): number {
  // Вики-ссылка — задача 2.6: пропустить целиком, не разбирая.
  if (text[start + 1] === '[') {
    const close = text.indexOf(']]', start + 2);
    return close >= 0 && close + 2 <= to ? close + 2 : start + 2;
  }
  // Картинка `![…](…)` — выпуск 0.3: исходный текст.
  if (start > 0 && text[start - 1] === '!' && text[start - 2] !== '\\') return start + 1;

  let depth = 0;
  let close = -1;
  for (let i = start + 1; i < to; i++) {
    const ch = text[i];
    if (ch === '\\') i++;
    else if (ch === '[') depth++;
    else if (ch === ']') {
      if (depth === 0) {
        close = i;
        break;
      }
      depth--;
    }
  }
  if (close < 0 || text[close + 1] !== '(') return start + 1;

  const target = destination(text, close + 2, to);
  if (!target) return start + 1;
  out.push({ kind: 'hide', from: start, to: start + 1 });
  if (close > start + 1) {
    out.push({ kind: 'mark', cls: 'cm-rz-link', url: target.url, from: start + 1, to: close });
    parseInline(text, start + 1, close, out);
  }
  out.push({ kind: 'hide', from: close, to: target.end });
  return target.end;
}

/** Адрес и необязательный заголовок ссылки до `)`. */
function destination(text: string, start: number, to: number): { url: string; end: number } | null {
  let i = skipSpaces(text, start, to);
  let url: string;
  if (text[i] === '<') {
    const close = text.indexOf('>', i + 1);
    if (close < 0 || close >= to) return null;
    url = text.slice(i + 1, close);
    if (url.includes('<')) return null;
    i = close + 1;
  } else {
    const begin = i;
    let depth = 0;
    while (i < to) {
      const ch = text[i] ?? '';
      if (ch === '\\' && i + 1 < to) {
        i += 2;
        continue;
      }
      if (SPACE.test(ch)) break;
      if (ch === '(') depth++;
      if (ch === ')') {
        if (depth === 0) break;
        depth--;
      }
      i++;
    }
    url = text.slice(begin, i);
  }
  const afterUrl = i;
  i = skipSpaces(text, i, to);
  const quote = text[i];
  if (i > afterUrl && (quote === '"' || quote === "'" || quote === '(')) {
    const closing = quote === '(' ? ')' : quote;
    let j = i + 1;
    while (j < to && text[j] !== closing) j += text[j] === '\\' ? 2 : 1;
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

/** Сопоставление серий (CommonMark, «process emphasis»). */
function emphasis(delimiters: Delimiter[], out: Piece[]): void {
  for (let c = 0; c < delimiters.length; c++) {
    const closer = delimiters[c];
    if (!closer?.canClose) continue;
    for (let o = c - 1; o >= 0 && closer.to > closer.from; o--) {
      const opener = delimiters[o];
      if (!opener || opener.char !== closer.char || !opener.canOpen || opener.to === opener.from) continue;
      if (
        closer.char !== '~' &&
        (opener.canClose || closer.canOpen) &&
        (opener.length + closer.length) % 3 === 0 &&
        !(opener.length % 3 === 0 && closer.length % 3 === 0)
      ) {
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
      for (let k = o + 1; k < c; k++) {
        const between = delimiters[k];
        if (between) between.to = between.from;
      }
      // Остаток открывающей серии может закрыться следующей серией: начать с неё снова.
      o = opener.to > opener.from ? o + 1 : o;
    }
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
const PROPERTIES_OPEN = /^---[ \t]*$/;
const PROPERTIES_CLOSE = /^(?:---|\.\.\.)[ \t]*$/;

/** Строка может открывать или закрывать ограду или блок свойств. */
export const BLOCK_EDGE = /^ {0,3}(?:```|~~~)|^(?:---|\.\.\.)/;

export interface LineSource {
  readonly lines: number;
  line(n: number): { from: number; to: number; text: string };
}

/** Найти блоки кода и блок свойств во всём тексте. */
export function scanBlocks(doc: LineSource): Blocks {
  const code: CodeBlock[] = [];
  let properties: Span | null = null;
  let n = 1;

  const first = doc.line(1);
  if (doc.lines > 1 && PROPERTIES_OPEN.test(first.text)) {
    for (let k = 2; k <= doc.lines; k++) {
      const line = doc.line(k);
      if (PROPERTIES_CLOSE.test(line.text)) {
        properties = { from: first.from, to: line.to };
        n = k + 1;
        break;
      }
    }
  }

  let open: { from: number; char: string; length: number } | null = null;
  for (; n <= doc.lines; n++) {
    const line = doc.line(n);
    const fence = FENCE.exec(line.text);
    if (!open) {
      if (!fence) continue;
      const marker = fence[1] ?? '';
      const char = marker[0] ?? '`';
      if (char === '`' && (fence[2] ?? '').includes('`')) continue;
      open = { from: line.from, char, length: marker.length };
    } else if (fence) {
      const marker = fence[1] ?? '';
      if (marker[0] === open.char && marker.length >= open.length && (fence[2] ?? '').trim() === '') {
        code.push({ from: open.from, to: line.to, closeLine: line.from });
        open = null;
      }
    }
  }
  if (open) {
    const last = doc.line(doc.lines);
    code.push({ from: open.from, to: last.to, closeLine: null });
  }
  return { code, properties };
}
