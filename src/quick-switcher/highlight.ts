// Подсветка совпадений в быстром переходе (Э3). Ядро отдаёт только найденные
// заметки, без позиций совпадений (контракт `switcher_search`), поэтому
// интерфейс находит буквы запроса сам: сначала подстрокой, иначе буквами по
// порядку, как нечёткий поиск. Это только вид: какие заметки найдены и в каком
// порядке, решает ядро.
import type { SwitcherItem } from '../api/index.ts';

export interface Segment {
  text: string;
  match: boolean;
}

/** Разделитель папок в строке пути справа от имени. */
export const FOLDER_SEPARATOR = ' / ';

/** Слова запроса: по пробелам и по `/` (как ищет ядро: «альфа/встр»). */
function words(query: string): string[][] {
  return query
    .split(/[\s/]+/u)
    .filter((word) => word.length > 0)
    .map((word) => Array.from(word.toLowerCase()));
}

/** Позиции подстроки `word` в `chars` или `null`. */
function substring(chars: string[], word: string[]): number[] | null {
  for (let start = 0; start + word.length <= chars.length; start++) {
    if (word.every((ch, i) => chars[start + i] === ch)) return word.map((_, i) => start + i);
  }
  return null;
}

/** Позиции букв `word` по порядку (жадно слева) или `null`. */
function subsequence(chars: string[], word: string[]): number[] | null {
  const found: number[] = [];
  let at = 0;
  for (const ch of word) {
    while (at < chars.length && chars[at] !== ch) at++;
    if (at === chars.length) return null;
    found.push(at++);
  }
  return found;
}

function find(lower: string[], word: string[]): number[] | null {
  return substring(lower, word) ?? subsequence(lower, word);
}

/** Строка, разбитая по символам (не по единицам UTF-16), и её строчный вид. */
class Text {
  readonly chars: string[];
  readonly lower: string[];
  readonly marked = new Set<number>();

  constructor(text: string) {
    this.chars = Array.from(text);
    // Посимвольно: длина строчного вида совпадает с исходным по позициям.
    this.lower = this.chars.map((ch) => {
      const low = ch.toLowerCase();
      return Array.from(low).length === 1 ? low : ch;
    });
  }

  /** Подсвечивает слово; `false` — слова в строке нет. */
  mark(word: string[]): boolean {
    const found = find(this.lower, word);
    if (!found) return false;
    for (const i of found) this.marked.add(i);
    return true;
  }

  segments(): Segment[] {
    const segments: Segment[] = [];
    this.chars.forEach((ch, i) => {
      const match = this.marked.has(i);
      const last = segments.at(-1);
      if (last && last.match === match) last.text += ch;
      else segments.push({ text: ch, match });
    });
    return segments;
  }
}

/** Куски строки с отметкой совпадения каждого слова запроса. */
export function highlight(text: string, query: string): Segment[] {
  const target = new Text(text);
  for (const word of words(query)) target.mark(word);
  return target.segments();
}

export interface HighlightedItem {
  name: Segment[];
  /** Папки через « / »; пусто — заметка в корне. */
  folders: Segment[];
}

/** Подсветка элемента: слово ищется в имени, а если там его нет — в папках. */
export function highlightItem(item: SwitcherItem, query: string): HighlightedItem {
  const name = new Text(item.name);
  const folders = new Text(item.path.slice(0, -1).join(FOLDER_SEPARATOR));
  for (const word of words(query)) {
    if (!name.mark(word)) folders.mark(word);
  }
  return { name: name.segments(), folders: folders.segments() };
}
