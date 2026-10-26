// Создание заметки из быстрого перехода (С3.5): пункт «Создать заметку „…“»,
// если заметки с таким именем нет. Место — по настройке «где создавать новые
// заметки», как у Ctrl+N. Имя проверяет ядро (`name_check`, затем
// `note_create`); здесь — только что предложить и куда.
import type { NewNoteLocation, SwitcherItem, VaultPath } from '../api/index.ts';

const NOTE_EXTENSION = /\.md$/i;

/** Ключ для сравнения имён: Unicode NFC, без учёта регистра. */
function nameKey(name: string): string {
  return name.normalize('NFC').toLocaleLowerCase('ru').normalize('NFC');
}

/** Имя заметки из запроса: без пробелов по краям и без `.md` в конце. */
function titleFromQuery(query: string): string {
  return query.trim().replace(NOTE_EXTENSION, '').trim();
}

/**
 * Имя для пункта «Создать заметку „…“» или `null`, если создавать нечего:
 * запрос пуст или среди найденных уже есть заметка с этим именем.
 */
export function createOffer(query: string, items: readonly SwitcherItem[]): string | null {
  const title = titleFromQuery(query);
  if (title === '') return null;
  const key = nameKey(title);
  return items.some((item) => nameKey(item.name) === key) ? null : title;
}

/**
 * В имени есть разделитель папок (`/` или `\`). Заметка с папкой одним
 * действием не создаётся (решение 50): пункт создания показывается неактивным
 * с пояснением, ядро не вызывается. Остальные недопустимые символы проверяет
 * `name_check` после Enter.
 */
export function hasFolderSeparator(title: string): boolean {
  return /[\\/]/.test(title);
}

/** Имя файла новой заметки. */
export function noteFileName(title: string): string {
  return `${title}.md`;
}

/** Папка новой заметки по настройке; `current` — открытая заметка. */
export function newNoteDir(location: NewNoteLocation, current: VaultPath | null): VaultPath {
  switch (location.kind) {
    case 'vault_root':
      return [];
    case 'current_folder':
      return current ? current.slice(0, -1) : [];
    case 'folder':
      return [...location.path];
  }
}
