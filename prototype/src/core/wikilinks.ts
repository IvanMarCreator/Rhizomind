/**
 * Разбор вики-ссылок в синтаксисе Obsidian.
 * Чистый модуль без зависимостей: используется и в main, и в интерфейсе.
 *
 *   [[Заметка]]
 *   [[Заметка|Псевдоним]]
 *   [[Заметка#Заголовок]]
 *   ![[Заметка]]            — встраивание
 */
export interface WikiLink {
  /** Текст ссылки целиком, например "![[Заметка#Раздел|Текст]]". */
  readonly raw: string;
  /** Цель без заголовка и псевдонима: "Заметка" или "Папка/Заметка". */
  readonly target: string;
  readonly heading?: string;
  readonly alias?: string;
  readonly embed: boolean;
  /** Позиция начала ссылки в тексте. */
  readonly index: number;
}

const LINK_RE = /(!?)\[\[([^\[\]\n]+?)\]\]/g;
const FENCE_RE = /^(```|~~~)/;

/** Находит все вики-ссылки, пропуская блоки кода и `инлайн-код`. */
export function parseWikiLinks(text: string): WikiLink[] {
  const links: WikiLink[] = [];
  let offset = 0;
  let inFence = false;

  for (const line of text.split('\n')) {
    if (FENCE_RE.test(line.trimStart())) {
      inFence = !inFence;
    } else if (!inFence) {
      const masked = maskInlineCode(line);
      for (const m of masked.matchAll(LINK_RE)) {
        const link = toLink(line.slice(m.index, m.index + m[0].length), m[1] === '!', m[2] ?? '', offset + m.index);
        if (link) links.push(link);
      }
    }
    offset += line.length + 1;
  }
  return links;
}

function toLink(raw: string, embed: boolean, inner: string, index: number): WikiLink | null {
  const pipe = inner.indexOf('|');
  const ref = pipe >= 0 ? inner.slice(0, pipe) : inner;
  const alias = pipe >= 0 ? inner.slice(pipe + 1).trim() : undefined;
  const hash = ref.indexOf('#');
  const target = (hash >= 0 ? ref.slice(0, hash) : ref).trim();
  const heading = hash >= 0 ? ref.slice(hash + 1).trim() : undefined;
  if (!target) return null; // [[#Заголовок]] — ссылка внутри той же заметки, не связь
  return {
    raw,
    target,
    embed,
    index,
    ...(heading ? { heading } : {}),
    ...(alias ? { alias } : {}),
  };
}

/** Заменяет содержимое `инлайн-кода` пробелами, сохраняя позиции символов. */
function maskInlineCode(line: string): string {
  return line.replace(/`[^`]*`/g, (s) => ' '.repeat(s.length));
}

/** Нормализует цель ссылки для сравнения: без .md, без регистра, «/» как разделитель. */
export function normalizeTarget(target: string): string {
  return target
    .replace(/\\/g, '/')
    .replace(/\.md$/i, '')
    .trim()
    .toLowerCase();
}

/**
 * Ищет заметку по цели ссылки, как Obsidian:
 * сначала точное совпадение пути, затем совпадение имени файла.
 */
export function resolveLinkTarget<T extends { path: string }>(target: string, notes: readonly T[]): T | null {
  const wanted = normalizeTarget(target);
  if (!wanted) return null;
  const byPath = notes.find((n) => normalizeTarget(n.path) === wanted);
  if (byPath) return byPath;
  if (wanted.includes('/')) return null;
  return notes.find((n) => normalizeTarget(n.path.split('/').pop() ?? '') === wanted) ?? null;
}
