// Классы символов и правило имени тега (С2-3.3, Р15, Р17, решение 113) — как
// `is_body`, `allows_tag_after` и `valid_name` в src-tauri/src/markdown/tags.rs.
// Таблицы Unicode общие с ядром (unicode-tables.ts), не `\p{…}` движка
// JavaScript: версия Unicode у обеих сторон одна и та же.
import { DECIMAL_DIGIT, PUNCT_SYMBOL, TAG_WORD, WHITE_SPACE, inTable } from './unicode-tables.ts';

/** Наибольшая длина имени тега в кодовых точках после NFC (Р17 п.1). */
export const MAX_TAG_CHARS = 200;

const isAsciiAlnum = (c: number) => (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a);

/** Символ тела тега: буквы, комбинируемые знаки, десятичные цифры, `_`, `-`, `/`. */
export function isBody(c: number): boolean {
  if (c < 0x80) return isAsciiAlnum(c) || c === 0x5f || c === 0x2d || c === 0x2f;
  return inTable(TAG_WORD, c);
}

function isDigit(c: number): boolean {
  return c < 0x80 ? c >= 0x30 && c <= 0x39 : inTable(DECIMAL_DIGIT, c);
}

/** Пробельный символ — Unicode White_Space (Р17 п.4). */
export function isSpace(c: number): boolean {
  return inTable(WHITE_SPACE, c);
}

/** ASCII-знаки, после которых `#` не начинает тег: `&`, `/`, `\`, `#`, `_`. */
const ASCII_NOT_BEFORE_TAG = new Set([0x26, 0x2f, 0x5c, 0x23, 0x5f]);

/** Символ, после которого `#` может начинать тег (Р17 п.3). */
export function allowsTagAfter(c: number): boolean {
  if (c < 0x80) {
    // Как `is_ascii_whitespace` Rust и `\x0b`; видимые ASCII, кроме букв и цифр, — P* и S*.
    if (c === 0x20 || (c >= 0x09 && c <= 0x0d)) return true;
    const punct = c > 0x20 && c < 0x7f && !isAsciiAlnum(c);
    return punct && !ASCII_NOT_BEFORE_TAG.has(c);
  }
  return isSpace(c) || inTable(PUNCT_SYMBOL, c);
}

/**
 * Имя подходит под правило тега целиком (без `#`): непустое, только символы
 * тела, хотя бы одна не-цифра, `/` не в начале, не в конце и не два подряд,
 * не длиннее 200 кодовых точек после NFC. Тем же правилом проверяются
 * значения блока свойств.
 */
export function isTagName(name: string): boolean {
  if (name === '' || name.startsWith('/') || name.endsWith('/') || name.includes('//')) return false;
  let digitsOnly = true;
  for (let i = 0; i < name.length; ) {
    const c = name.codePointAt(i) ?? 0;
    if (!isBody(c)) return false;
    if (!isDigit(c)) digitsOnly = false;
    i += c > 0xffff ? 2 : 1;
  }
  if (digitsOnly) return false;
  // NFC удлиняет имя не больше чем втрое: короткое имя не проверяется.
  if (name.length * 3 <= MAX_TAG_CHARS) return true;
  let count = 0;
  for (const _char of name.normalize('NFC')) {
    if (++count > MAX_TAG_CHARS) return false;
  }
  return true;
}
