// Блок свойств (frontmatter) и теги из него (выпуск 0.2, С2-3.3 «Теги в
// блоке свойств», Р14, Р17 п.7, Р18) — для редактора (строка «Свойства»,
// src/editor/live-preview/frontmatter.ts) и прохода по тексту (index.ts).
// Те же правила, что у ядра (src-tauri/src/markdown/frontmatter.rs); обе
// стороны проверяются общими образцами tests/fixtures/frontmatter/.
//
// Блок — от строки `---` в самом начале текста до следующей строки `---`
// или `...` (концевые пробелы и табуляции допустимы); незакрытый — не блок.
// Из блока читаются только ключи `tags` и `tag` (регистр ключа не важен) в
// формах `[a, b]`, `a, b`, `a` и списком строками `- a` (с отступом или
// без); значения в кавычках и с `#` впереди допустимы, значение не по
// правилу тега пропускается. Свой разбор ровно этих форм, без библиотеки YAML.
import { nameKey } from '../names/index.ts';
import type { LineSpan } from './blocks.ts';
import { isTagName } from './rules.ts';

/** Открывающая строка блока свойств. */
export const FRONTMATTER_OPEN = /^---[ \t]*$/;
/** Закрывающая строка блока свойств. */
export const FRONTMATTER_CLOSE = /^(?:---|\.\.\.)[ \t]*$/;

/** Блок свойств: от первой `---` до конца закрывающей строки без перевода строки; теги по порядку. */
export interface Frontmatter extends LineSpan {
  tags: string[];
}

/** Строки с позиции `from`: переводы `\r\n`, `\r`, `\n` (как у ядра); после последнего — ещё одна строка. */
export function* linesFrom(text: string, from = 0): Generator<LineSpan> {
  let start = from;
  for (let i = from; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    if (ch !== 0x0a && ch !== 0x0d) continue;
    yield { from: start, to: i };
    if (ch === 0x0d && text.charCodeAt(i + 1) === 0x0a) i++;
    start = i + 1;
  }
  yield { from: start, to: text.length };
}

const SPACES = /^[ \t]*|[ \t]*$/g;

/** Ключ `tags:`/`tag:` в начале строки (регистр ключа не важен) — значение после `:`. */
function tagsValue(line: string): string | null {
  const colon = line.indexOf(':');
  if (colon < 0) return null;
  const key = line.slice(0, colon).toLowerCase();
  return key === 'tags' || key === 'tag' ? line.slice(colon + 1) : null;
}

/** Элемент списка `- значение` (отступ пробелами и табуляциями допустим) — значение. */
function listItem(line: string): string | null {
  const rest = line.replace(/^[ \t]*/, '');
  if (!rest.startsWith('-')) return null;
  const after = rest.slice(1);
  return after === '' || after.startsWith(' ') || after.startsWith('\t') ? after : null;
}

/** Одно значение: без пробелов по краям, без парных кавычек и `#` впереди; тег, если подходит целиком. */
function pushValue(raw: string, out: string[]): void {
  let value = raw.replace(SPACES, '');
  if (value.length >= 2 && ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'")))) {
    value = value.slice(1, -1);
  }
  if (value.startsWith('#')) value = value.slice(1);
  if (isTagName(value)) out.push(value);
}

function splitValues(raw: string, out: string[]): void {
  for (const part of raw.split(',')) pushValue(part, out);
}

/**
 * Блок свойств в начале `text` и теги ключей `tags`/`tag` из него (формы
 * `[a, b]`, `a, b`, `a`, список строками `- a`); `null` — блока нет
 * (незакрытый — не блок). Размер текста здесь не проверяется: заметка
 * больше 10 МБ (решение 77) отсекается вызывающим.
 */
export function parseFrontmatter(text: string): Frontmatter | null {
  const all = linesFrom(text);
  const first = all.next();
  if (first.done || !FRONTMATTER_OPEN.test(text.slice(first.value.from, first.value.to))) return null;
  const tags: string[] = [];
  let inList = false;
  for (const span of all) {
    const line = text.slice(span.from, span.to);
    if (FRONTMATTER_CLOSE.test(line)) return { from: 0, to: span.to, tags };
    if (inList) {
      const item = listItem(line);
      if (item !== null) {
        pushValue(item, tags);
        continue;
      }
    }
    inList = false;
    const value = tagsValue(line);
    if (value === null) continue;
    const raw = value.replace(SPACES, '');
    if (raw === '') inList = true;
    else if (raw.startsWith('[')) {
      if (raw.length >= 2 && raw.endsWith(']')) splitValues(raw.slice(1, -1), tags);
    } else splitValues(raw, tags);
  }
  return null;
}

/** Теги блока свойств для строки «Свойства»: без повторов (`#Проект` и `#проект` — один), в первом написании. */
export function frontmatterTags(text: string): string[] {
  const seen = new Set<string>();
  return (parseFrontmatter(text)?.tags ?? []).filter((tag) => {
    const key = nameKey(tag);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
