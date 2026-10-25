// Вики-ссылки в тексте заметки — для отображения в редакторе (С3.4, задача 1.8).
//
// Разбор цели — те же правила, что в ядре (src-tauri/src/links, `parse_target`):
// обе стороны проверяются общими образцами tests/fixtures/wikilinks/parse.json.
// `[[#Заголовок]]` недопустима — в редакторе это обычный текст: ссылки только на
// другие заметки, якоря внутри заметки будут другим синтаксисом позже (С3.4).
// Куда ведёт ссылка, решает ядро (команда `links_resolve`); здесь только
// синтаксис: где в тексте ссылки и из чего они состоят.

/** Цель вики-ссылки: текст между `[[` и `]]` после разбора. */
export interface LinkTarget {
  /** Папки пути (`Проекты/Альфа/Встреча` → `["Проекты", "Альфа"]`). */
  folders: string[];
  /** Имя заметки без `.md`. */
  name: string;
  /** Заголовок после `#`. */
  heading: string | null;
  /** Псевдоним после `|`. */
  alias: string | null;
  /** Путь от корня хранилища (`[[/Встреча]]`). */
  from_root: boolean;
}

/** Почему текст ссылки не может быть целью. */
export type InvalidTarget = 'empty_target' | 'empty_component' | 'dot_component' | 'forbidden_chars';

/** Разобранная цель или причина, по которой она недопустима. */
export type ParsedTarget = LinkTarget | { invalid: InvalidTarget };

/** Вики-ссылка в тексте. */
export interface WikiLink {
  /** Начало в единицах UTF-16 (индекс строки JavaScript); у `![[…]]` — позиция `!`. */
  from: number;
  /** Конец — позиция после `]]`. */
  to: number;
  /** Текст ссылки как в заметке. */
  raw: string;
  /** Встраивание `![[…]]`. */
  embed: boolean;
  /** Разбор текста между `[[` и `]]`. */
  target: ParsedTarget;
}

/** Запрещённые в именах Windows символы (как `names::FORBIDDEN` в ядре). */
const FORBIDDEN = new Set(['\\', '/', ':', '*', '?', '"', '<', '>', '|']);

/** Управляющий символ — общая категория Cc, как `char::is_control` в Rust. */
function isControl(code: number): boolean {
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
}

function hasForbiddenChar(text: string): boolean {
  for (const char of text) {
    if (FORBIDDEN.has(char) || isControl(char.codePointAt(0) ?? 0)) return true;
  }
  return false;
}

/** Пробелы и табуляции вокруг (неразрывный пробел остаётся — Windows разрешает его в конце имени). */
function trimBlanks(text: string): string {
  return text.replace(/^[ \t]+|[ \t]+$/g, '');
}

function nonEmpty(text: string): string | null {
  const trimmed = trimBlanks(text);
  return trimmed === '' ? null : trimmed;
}

/** Имя без последнего `.md` (без учёта регистра), если остаётся непустая основа. */
function noteStem(file: string): string {
  const dot = file.lastIndexOf('.');
  if (dot > 0 && /^[mM][dD]$/.test(file.slice(dot + 1))) return file.slice(0, dot);
  return file;
}

function splitOnce(text: string, separator: string): [string, string | undefined] {
  const at = text.indexOf(separator);
  return at < 0 ? [text, undefined] : [text.slice(0, at), text.slice(at + separator.length)];
}

/**
 * Разбор текста между `[[` и `]]` по правилам общих образцов parse.json:
 * псевдоним — после первой `|` (`\|` в таблице — тоже разделитель), заголовок —
 * после первого `#`, ведущая `/` — путь от корня, последнее `.md` отбрасывается.
 * Проверяется только синтаксис пути.
 */
export function parseTarget(inner: string): ParsedTarget {
  const [targetRaw, aliasRaw] = splitOnce(inner, '|');
  const target = aliasRaw !== undefined && targetRaw.endsWith('\\') ? targetRaw.slice(0, -1) : targetRaw;
  const alias = aliasRaw === undefined ? null : nonEmpty(aliasRaw);
  const [pathRaw, headingRaw] = splitOnce(target, '#');
  const heading = headingRaw === undefined ? null : nonEmpty(headingRaw);
  let path = trimBlanks(pathRaw);
  const fromRoot = path.startsWith('/');
  if (fromRoot) path = path.slice(1);
  if (path === '') return { invalid: 'empty_target' };
  const components = path.split('/');
  if (components.some((c) => c === '')) return { invalid: 'empty_component' };
  if (components.some((c) => c === '.' || c === '..')) return { invalid: 'dot_component' };
  if (components.some(hasForbiddenChar)) return { invalid: 'forbidden_chars' };
  const file = components.pop() ?? '';
  return { folders: components, name: noteStem(file), heading, alias, from_root: fromRoot };
}

interface Line {
  start: number;
  end: number;
  text: string;
}

function splitLines(text: string): Line[] {
  const lines: Line[] = [];
  const breaks = /\r\n|\r|\n/g;
  let start = 0;
  for (let match = breaks.exec(text); match !== null; match = breaks.exec(text)) {
    lines.push({ start, end: match.index, text: text.slice(start, match.index) });
    start = match.index + match[0].length;
  }
  lines.push({ start, end: text.length, text: text.slice(start) });
  return lines;
}

/** Ширина отступа в колонках: табуляция — до следующей кратной 4. */
function indentColumns(line: string): number {
  let columns = 0;
  for (const char of line) {
    if (char === ' ') columns += 1;
    else if (char === '\t') columns += 4 - (columns % 4);
    else break;
  }
  return columns;
}

const BLANK = /^[ \t]*$/;
const FENCE_OPEN = /^ {0,3}(`{3,}|~{3,})(.*)$/;
const LIST_ITEM = /^ {0,3}(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)/;

/**
 * Абзацы — участки текста вне блоков кода: ссылки и код в строке ищутся
 * только в них. Блоки ``` и ~~~ (отступ до трёх пробелов, закрывает строка
 * из тех же символов не короче открывающей) и код с отступом (4 колонки
 * после пустой строки или в начале текста, кроме продолжения пункта списка).
 */
function paragraphs(text: string): Array<[number, number]> {
  const result: Array<[number, number]> = [];
  let fence: { closing: RegExp } | null = null;
  let previous: 'start' | 'blank' | 'code' | 'text' = 'start';
  let inList = false;
  let open: [number, number] | null = null;
  const close = (): void => {
    if (open) result.push(open);
    open = null;
  };
  for (const line of splitLines(text)) {
    let kind: 'blank' | 'code' | 'text';
    if (fence) {
      if (fence.closing.test(line.text)) fence = null;
      kind = 'code';
    } else if (BLANK.test(line.text)) {
      kind = 'blank';
    } else {
      const indent = indentColumns(line.text);
      const opening = FENCE_OPEN.exec(line.text);
      const afterBreak = previous !== 'text';
      if (indent >= 4 && afterBreak && !inList) {
        kind = 'code';
      } else if (opening && (opening[1]?.startsWith('~') || !opening[2]?.includes('`'))) {
        const marks = opening[1] ?? '```';
        // Закрывающая строка — те же символы не короче открывающей; RegExp — один на блок.
        fence = { closing: new RegExp(`^ {0,3}\\${marks.charAt(0)}{${marks.length},}[ \\t]*$`) };
        kind = 'code';
      } else {
        kind = 'text';
        if (LIST_ITEM.test(line.text)) inList = true;
        else if (previous === 'blank' && indent < 2) inList = false;
      }
    }
    if (kind === 'text') {
      if (open) open[1] = line.end;
      else open = [line.start, line.end];
    } else {
      close();
    }
    previous = kind;
  }
  close();
  return result;
}

/** Участки кода в строке внутри абзаца: пары серий обратных кавычек одной длины. */
function codeSpans(text: string, start: number, end: number): Array<[number, number]> {
  const spans: Array<[number, number]> = [];
  const runAt = (i: number): number => {
    let j = i;
    while (j < end && text[j] === '`') j += 1;
    return j - i;
  };
  let i = start;
  while (i < end) {
    if (text[i] !== '`') {
      i += 1;
      continue;
    }
    const length = runAt(i);
    let j = i + length;
    let closedAt = -1;
    while (j < end) {
      if (text[j] === '`') {
        const other = runAt(j);
        if (other === length) {
          closedAt = j;
          break;
        }
        j += other;
      } else {
        j += 1;
      }
    }
    if (closedAt < 0) {
      i += length;
    } else {
      spans.push([i, closedAt + length]);
      i = closedAt + length;
    }
  }
  return spans;
}

/** Все вики-ссылки текста по порядку, кроме `[[` внутри кода. */
export function findWikilinks(text: string): WikiLink[] {
  const links: WikiLink[] = [];
  for (const [start, end] of paragraphs(text)) {
    const spans = codeSpans(text, start, end);
    let span = 0;
    let i = start;
    while (i < end - 1) {
      while (span < spans.length && (spans[span]?.[1] ?? 0) <= i) span += 1;
      const code = spans[span];
      if (code && code[0] <= i) {
        i = code[1];
        continue;
      }
      if (text[i] !== '[' || text[i + 1] !== '[') {
        i += 1;
        continue;
      }
      const codeStart = code ? code[0] : end;
      let open = i;
      let k = i + 2;
      let closed = -1;
      while (k < codeStart) {
        const char = text[k];
        if (char === '\n' || char === '\r') break;
        if (char === ']' && text[k + 1] === ']') {
          closed = k;
          break;
        }
        if (char === '[' && text[k + 1] === '[') open = k;
        k += 1;
      }
      if (closed < 0 || closed + 2 > codeStart) {
        i = Math.max(k, i + 2);
        continue;
      }
      const embed = open > start && text[open - 1] === '!';
      const from = embed ? open - 1 : open;
      const to = closed + 2;
      links.push({
        from,
        to,
        raw: text.slice(from, to),
        embed,
        target: parseTarget(text.slice(open + 2, closed)),
      });
      i = to;
    }
  }
  return links;
}
