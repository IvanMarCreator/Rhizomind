// Вид «Теги» (0.2-2.3, С2-3.3): порядок тегов и строки заметок тега —
// чистые функции без состояния (модель — tags.svelte.ts).
import type { TagSummary, VaultPath } from '../api/index.ts';
import type { TagOrder } from '../core/layout/layout.ts';
import { nameKey } from '../core/names/index.ts';

/** Заметка в раскрытом теге: имя без `.md`, папка — только при повторе имени. */
export interface TagNoteRow {
  path: VaultPath;
  name: string;
  /** Папка для показа серым (`Проекты / Альфа`); `null` — имя не повторяется или заметка в корне. */
  folder: string | null;
}

/** Естественный порядок по-русски, как дерево файлов: «задача2» перед «задача10». */
const collator = new Intl.Collator('ru', { numeric: true });

/** Ключ тега — тот же, что у ядра для имён и тегов: NFC → регистр → NFC. */
export const tagKey = (tag: string): string => nameKey(tag.replace(/^#/, ''));

export function sortTags(tags: readonly TagSummary[], order: TagOrder): TagSummary[] {
  const byName = (a: TagSummary, b: TagSummary) => collator.compare(a.tag, b.tag) || collator.compare(a.key, b.key);
  return [...tags].sort(order === 'count' ? (a, b) => b.count - a.count || byName(a, b) : byName);
}

const NOTE_EXT = /\.md$/i;

export function noteRows(paths: readonly VaultPath[]): TagNoteRow[] {
  const named = paths.map((path) => ({ path, name: (path.at(-1) ?? '').replace(NOTE_EXT, '') }));
  const seen = new Map<string, number>();
  for (const { name } of named) seen.set(nameKey(name), (seen.get(nameKey(name)) ?? 0) + 1);
  return named
    .map(({ path, name }) => ({
      path,
      name,
      folder: (seen.get(nameKey(name)) ?? 0) > 1 && path.length > 1 ? path.slice(0, -1).join(' / ') : null,
    }))
    .sort((a, b) => collator.compare(a.name, b.name) || collator.compare(a.path.join('/'), b.path.join('/')));
}

