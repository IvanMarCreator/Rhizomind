// Теги в абзаце: `#` вне кода в строке, вики-ссылок и адресов ссылок Markdown
// — как `find_in_paragraph`, `link_destinations` (src-tauri/src/markdown/tags.rs)
// и `wikilinks::find` (src-tauri/src/markdown/wikilinks.rs). Каждый поиск
// линейный по длине абзаца.
import { codeSpans, type LineSpan } from './blocks.ts';
import { allowsTagAfter, isBody, isSpace, isTagName } from './rules.ts';

/** Первое вхождение символа `ch` на участке `[from, to)`; `-1` — нет. Поиск не выходит за участок. */
export function indexIn(text: string, ch: number, from: number, to: number): number {
  for (let i = from; i < to; i++) if (text.charCodeAt(i) === ch) return i;
  return -1;
}

/** Тег в тексте: `from` — позиция `#`, `to` — конец имени (единицы UTF-16). */
export interface TextTag {
  /** Имя без `#`, как в тексте (без нормализации и смены регистра). */
  name: string;
  from: number;
  to: number;
}

/**
 * Вики-ссылки абзаца `[from, to)` вне кода `code`: ссылку открывает
 * последняя `[[` перед первой `]]`, ссылка не переходит через перенос строки
 * и не заходит в код (как `findWikilinks` в src/core/wikilinks).
 */
function wikilinkSpans(text: string, from: number, to: number, code: readonly LineSpan[]): LineSpan[] {
  const links: LineSpan[] = [];
  let span = 0;
  let i = from;
  while (i + 1 < to) {
    while (span < code.length && (code[span]?.to ?? 0) <= i) span++;
    const c = code[span];
    if (c && c.from <= i) {
      i = c.to;
      continue;
    }
    if (text[i] !== '[' || text[i + 1] !== '[') {
      i++;
      continue;
    }
    const codeStart = c ? c.from : to;
    let open = i;
    let k = i + 2;
    let closed = -1;
    while (k < codeStart) {
      const ch = text[k];
      if (ch === '\n' || ch === '\r') break;
      if (ch === ']' && text[k + 1] === ']') {
        closed = k;
        break;
      }
      if (ch === '[' && text[k + 1] === '[') open = k;
      k++;
    }
    if (closed < 0 || closed + 2 > codeStart) {
      i = Math.max(k, i + 2);
      continue;
    }
    const embed = open > from && text[open - 1] === '!';
    links.push({ from: embed ? open - 1 : open, to: closed + 2 });
    i = closed + 2;
  }
  return links;
}

/** Участки кода и вики-ссылок: «внутри ли позиция» для позиций по возрастанию. */
class Excluded {
  readonly #lists: Array<{ list: readonly LineSpan[]; next: number }>;

  constructor(code: readonly LineSpan[], links: readonly LineSpan[]) {
    this.#lists = [
      { list: code, next: 0 },
      { list: links, next: 0 },
    ];
  }

  /** Позиция внутри участка; позиции запрашиваются по возрастанию. */
  covers(at: number): boolean {
    return this.#lists.some((entry) => {
      while (entry.next < entry.list.length && (entry.list[entry.next]?.to ?? 0) <= at) entry.next++;
      const span = entry.list[entry.next];
      return span !== undefined && span.from <= at;
    });
  }

  /** Участок, который начинается в позиции `at` или содержит её (без сдвига указателей). */
  containing(at: number): LineSpan | null {
    for (const { list } of this.#lists) {
      let lo = 0;
      let hi = list.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if ((list[mid]?.to ?? 0) <= at) lo = mid + 1;
        else hi = mid;
      }
      const span = list[lo];
      if (span && span.from <= at) return span;
    }
    return null;
  }
}

const isAsciiPunct = (ch: string | undefined) => ch !== undefined && /^[!-/:-@[-`{-~]$/.test(ch);

/**
 * Поиски по строке для адресов ссылок — каждый один раз на строку, чтобы
 * разбор строки с множеством `[…](` оставался линейным.
 */
class Destinations {
  readonly #text: string;
  readonly #from: number;
  readonly #to: number;
  #brackets: Map<number, number> | null = null;
  #ends: Int32Array | null = null;
  readonly #next = new Map<string, { from: number; at: number }>();

  constructor(text: string, from: number, to: number) {
    this.#text = text;
    this.#from = from;
    this.#to = to;
  }

  /** `[` → парная `]` строки с учётом вложенности и `\`. */
  closeBracket(open: number): number | undefined {
    if (!this.#brackets) {
      const pairs = new Map<number, number>();
      const stack: number[] = [];
      for (let i = this.#from; i < this.#to; i++) {
        const ch = this.#text[i];
        if (ch === '\\') i++;
        else if (ch === '[') stack.push(i);
        else if (ch === ']') {
          const o = stack.pop();
          if (o !== undefined) pairs.set(o, i);
        }
      }
      this.#brackets = pairs;
    }
    return this.#brackets.get(open);
  }

  /** Первое вхождение `target` с позиции `from` до конца строки; с `escapes` символ после `\` пропускается. */
  nextOf(target: string, from: number, escapes: boolean): number {
    const key = escapes ? `\\${target}` : target;
    const memo = this.#next.get(key);
    if (memo && from >= memo.from && from <= memo.at) return memo.at;
    let i = from;
    while (i < this.#to && this.#text[i] !== target) i += escapes && this.#text[i] === '\\' ? 2 : 1;
    const at = Math.min(i, this.#to);
    this.#next.set(key, { from, at });
    return at;
  }

  /** Конец адреса, начатого в позиции: первый пробельный символ или `)` без пары `(` после начала. */
  destinationEnd(at: number): number {
    if (!this.#ends) {
      const text = this.#text;
      const [from, to] = [this.#from, this.#to];
      const length = to - from;
      const balance = new Int32Array(length + 1);
      const escaped = new Uint8Array(length + 1);
      let depth = 0;
      for (let i = from; i < to; i++) {
        balance[i - from] = depth;
        const ch = text[i];
        if (ch === '\\' && i + 1 < to) {
          escaped[i - from] = 1;
          escaped[i + 1 - from] = 1;
          balance[i + 1 - from] = depth;
          i++;
        } else if (ch === '(') depth++;
        else if (ch === ')') depth--;
      }
      const ends = new Int32Array(length + 1).fill(to);
      let space = to;
      const closeAt = new Map<number, number>();
      for (let i = to - 1; i >= from; i--) {
        const k = i - from;
        if (!escaped[k]) {
          const c = text.charCodeAt(i);
          if (isSpace(c)) space = i;
          else if (c === 0x29) closeAt.set(balance[k] ?? 0, i);
        }
        ends[k] = Math.min(space, closeAt.get(balance[k] ?? 0) ?? to);
      }
      this.#ends = ends;
    }
    return this.#ends[at - this.#from] ?? this.#to;
  }

  /** Адрес и необязательный заголовок с позиции `start` до `)`; конец — после `)`, `-1` — не ссылка. */
  destination(start: number, to: number): number {
    const text = this.#text;
    const skip = (i: number) => {
      while (i < to && (text[i] === ' ' || text[i] === '\t')) i++;
      return i;
    };
    let i = skip(start);
    if (text[i] === '<') {
      const close = this.nextOf('>', i + 1, false);
      if (close >= to || text.slice(i + 1, close).includes('<')) return -1;
      i = close + 1;
    } else {
      i = Math.min(this.destinationEnd(i), to);
    }
    const afterUrl = i;
    i = skip(i);
    const quote = text[i];
    if (i > afterUrl && i < to && (quote === '"' || quote === "'" || quote === '(')) {
      const j = this.nextOf(quote === '(' ? ')' : quote, i + 1, true);
      if (j >= to) return -1;
      i = skip(j + 1);
    }
    return i < to && text[i] === ')' ? i + 1 : -1;
  }
}

/**
 * Адреса ссылок Markdown `[текст](адрес "заголовок")` и картинок в строке —
 * от `]` до `)` включительно: там тегов нет (Р17 п.6). `[` в коде, вики-ссылке
 * или после `\` не открывает ссылку; текст ссылки разбирается как обычный.
 */
function linkDestinations(text: string, line: LineSpan, excluded: Excluded): LineSpan[] {
  const out: LineSpan[] = [];
  const ctx = new Destinations(text, line.from, line.to);
  const stack: Array<{ close: number; end: number }> = [];
  let to = line.to;
  let i = line.from;
  for (;;) {
    for (let top = stack.at(-1); top && i >= top.close; top = stack.at(-1)) {
      stack.pop();
      i = top.end;
      to = stack.at(-1)?.close ?? line.to;
    }
    if (i >= to) break;
    const inside = excluded.containing(i);
    if (inside) {
      i = inside.to;
      continue;
    }
    const ch = text[i];
    if (ch === '\\' && i + 1 < to && isAsciiPunct(text[i + 1])) {
      i += 2;
    } else if (ch === '[' && text[i + 1] === '[') {
      i += 2;
    } else if (ch === '[') {
      const close = ctx.closeBracket(i);
      const end = close !== undefined && close < to && text[close + 1] === '(' ? ctx.destination(close + 2, to) : -1;
      if (close !== undefined && end >= 0) {
        out.push({ from: close, to: end });
        stack.push({ close, end });
        to = close;
      }
      i++;
    } else {
      i++;
    }
  }
  return out.sort((a, b) => a.from - b.from);
}

/** Кодовая точка, которая кончается перед позицией `at`. */
function codePointBefore(text: string, at: number): number {
  const low = text.charCodeAt(at - 1);
  if (low >= 0xdc00 && low <= 0xdfff && at >= 2) {
    const high = text.charCodeAt(at - 2);
    if (high >= 0xd800 && high <= 0xdbff) return (high - 0xd800) * 0x400 + (low - 0xdc00) + 0x10000;
  }
  return low;
}

/** Конец тела тега с позиции `from` (до `to`): первый символ не из тела. */
function bodyEnd(text: string, from: number, to: number): number {
  let i = from;
  while (i < to) {
    const c = text.codePointAt(i) ?? 0;
    if (!isBody(c)) break;
    i += c > 0xffff ? 2 : 1;
  }
  return Math.min(i, to);
}

const HASH = 0x23;
const BRACKET = 0x5b;

/**
 * Теги абзаца — строк `lines` подряд (без строк кода между ними): `#` вне кода
 * в строке, вики-ссылок и адресов ссылок Markdown, перед ним начало строки,
 * пробельный символ или знак (Р17 п.3), имя — по правилу тега.
 */
export function tagsInParagraph(text: string, lines: readonly LineSpan[]): TextTag[] {
  const first = lines[0];
  const last = lines.at(-1);
  if (!first || !last) return [];
  const out: TextTag[] = [];
  const code = codeSpans(text, first.from, last.to);
  const links = wikilinkSpans(text, first.from, last.to, code);
  const excluded = new Excluded(code, links);
  for (const line of lines) {
    let hash = indexIn(text, HASH, line.from, line.to);
    if (hash < 0) continue;
    const urls = indexIn(text, BRACKET, line.from, line.to) >= 0 ? linkDestinations(text, line, excluded) : [];
    let url = 0;
    while (hash >= 0) {
      const p = hash;
      let next = p + 1;
      while (url < urls.length && (urls[url]?.to ?? 0) <= p) url++;
      const inUrl = (urls[url]?.from ?? Infinity) <= p;
      if (!excluded.covers(p) && !inUrl && (p === line.from || allowsTagAfter(codePointBefore(text, p)))) {
        const end = bodyEnd(text, p + 1, line.to);
        const name = text.slice(p + 1, end);
        if (isTagName(name)) {
          out.push({ name, from: p, to: end });
          next = end;
        }
      }
      hash = indexIn(text, HASH, next, line.to);
    }
  }
  return out;
}
