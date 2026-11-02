// Теги в тексте заметки (выпуск 0.2, С2-3.3, Р15, Р17, решение 113) — для
// оформления в редакторе. Те же правила, что у ядра (src-tauri/src/markdown,
// `scan` → теги текста); обе стороны проверяются общими образцами
// tests/fixtures/tags/. Теги плоские: `-` и `/` — обычные символы имени,
// `#работа/отчёт` — один самостоятельный тег.
//
// Проход как у ядра: блок свойств в начале пропускается (его теги разбирает
// src/core/tags/frontmatter.ts, задача 0.2-3.2), строки текста
// делятся на код (ограда, отступ) и абзацы, в каждом абзаце ищутся теги вне
// кода в строке, вики-ссылок и адресов ссылок Markdown.
import { nameKey } from '../names/index.ts';
import { LineClassifier, type LineSpan } from './blocks.ts';
import { FRONTMATTER_CLOSE, FRONTMATTER_OPEN, linesFrom } from './frontmatter.ts';
import { tagsInParagraph, type TextTag } from './inline.ts';

export { LineClassifier, indentColumns, type LineKind, type LineSpan } from './blocks.ts';
export { tagsInParagraph, type TextTag } from './inline.ts';
export { isTagName, MAX_TAG_CHARS } from './rules.ts';
export { frontmatterTags, parseFrontmatter, type Frontmatter } from './frontmatter.ts';

/** Заметка больше этого (в байтах UTF-8) — блок свойств не распознаётся (решение 77, как у ядра). */
export const FRONTMATTER_MAX_NOTE_BYTES = 10 * 1024 * 1024;

/**
 * Ключ тега: как ключ имени заметки (NFC → приведение регистра → NFC).
 * `#Проект` и `#проект` — один тег, `#ё` и `#е` — разные. `#` впереди не входит.
 */
export function tagKey(name: string): string {
  return nameKey(name.startsWith('#') ? name.slice(1) : name);
}

function utf8Length(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x80) bytes += 1;
    else if (c < 0x800) bytes += 2;
    else if (c >= 0xd800 && c <= 0xdbff) {
      bytes += 4;
      i++;
    } else bytes += 3;
  }
  return bytes;
}

function tooLargeForFrontmatter(text: string): boolean {
  if (text.length > FRONTMATTER_MAX_NOTE_BYTES) return true;
  if (text.length * 3 <= FRONTMATTER_MAX_NOTE_BYTES) return false;
  return utf8Length(text) > FRONTMATTER_MAX_NOTE_BYTES;
}

/**
 * Где начинается текст после блока свойств: `0` — блока нет, `null` — после
 * закрывающей строки текста нет. Блок — от строки `---` в самом начале до
 * следующей строки `---` или `...`; незакрытый — не блок.
 */
export function bodyStart(text: string): number | null {
  if (tooLargeForFrontmatter(text)) return 0;
  const lines = linesFrom(text, 0);
  const first = lines.next();
  if (first.done || !FRONTMATTER_OPEN.test(text.slice(first.value.from, first.value.to))) return 0;
  for (const line of lines) {
    if (!FRONTMATTER_CLOSE.test(text.slice(line.from, line.to))) continue;
    if (line.to >= text.length) return null;
    return line.to + (text.startsWith('\r\n', line.to) ? 2 : 1);
  }
  return 0;
}

/** Теги текста заметки по порядку (кроме блока свойств); положения — в единицах UTF-16. */
export function findTags(text: string): TextTag[] {
  const start = bodyStart(text);
  if (start === null) return [];
  const tags: TextTag[] = [];
  const blocks = new LineClassifier();
  let paragraph: LineSpan[] = [];
  const flush = () => {
    if (paragraph.length > 0) tags.push(...tagsInParagraph(text, paragraph));
    paragraph = [];
  };
  for (const line of linesFrom(text, start)) {
    if (blocks.classify(text.slice(line.from, line.to)) === 'text') paragraph.push(line);
    else flush();
  }
  flush();
  return tags;
}
