// Переход по вики-ссылке (С3.4). Куда ведёт ссылка, решает ядро
// (`links_resolve`). Неразрешённая ссылка создаёт заметку и открывает её:
// - имя без пути — в папке заметки со ссылкой;
// - с путём (`Папка/Имя`, `/Папка/Имя`) — по этому пути от корня
//   хранилища, недостающие папки создаются (решение 84, как в Obsidian).
// Недопустимая цель (`..`, `.`, абсолютный путь, `\\?\`, UNC, `file:`,
// запрещённые и управляющие символы — правила parseTarget, те же, что в
// ядре) — ни одного вызова ядра. Пути уходят в ядро только по компонентам,
// ядро проверяет их ещё раз (контур хранилища). Внешние адреса здесь не
// открываются никогда: это не `open_external`.
import { CoreCallError, type TreeEntry, type VaultPath } from '../../api/index.ts';
import { parseTarget } from '../../core/wikilinks/index.ts';
import { targetKey } from './display.ts';

export interface FollowDeps {
  /** `links_resolve` одной цели: заметка или `null`. */
  resolve(from: VaultPath, target: string): Promise<VaultPath | null>;
  treeList(): Promise<readonly TreeEntry[]>;
  folderCreate(dir: VaultPath, name: string): Promise<VaultPath>;
  noteCreate(dir: VaultPath, name: string): Promise<VaultPath>;
}

export interface FollowResult {
  path: VaultPath;
  /** Заголовок для перехода (`[[Заметка#Заголовок]]`); у новой заметки — `null`. */
  heading: string | null;
  /** Заметка создана по неразрешённой ссылке. */
  created: boolean;
}

/** Ключ имени для сравнения папок: NFC, без учёта регистра (окончательно сравнивает ядро). */
function nameKey(name: string): string {
  return name.normalize('NFC').toLocaleLowerCase('ru').normalize('NFC');
}

/**
 * Папки пути от корня: существующие — с именем как на диске, недостающие
 * создаются по одной (`folder_create`).
 */
async function ensureFolders(folders: readonly string[], deps: FollowDeps): Promise<VaultPath> {
  if (folders.length === 0) return [];
  const entries = await deps.treeList();
  let dir: VaultPath = [];
  for (const folder of folders) {
    const existing = entries.find(
      (entry) =>
        entry.kind === 'folder' &&
        entry.path.length === dir.length + 1 &&
        dir.every((part, i) => entry.path[i] === part) &&
        nameKey(entry.path[dir.length] ?? '') === nameKey(folder),
    );
    dir = existing ? [...existing.path] : await deps.folderCreate(dir, folder);
  }
  return dir;
}

/**
 * Куда ведёт ссылка (текст между `[[` и `]]`) из заметки `from`; при
 * необходимости заметка создаётся. `null` — цель недопустима, ничего не
 * сделано. Ошибка ядра (имя запрещено в Windows, папки нет) — исключение
 * `CoreCallError`.
 */
export async function followWikilink(inner: string, from: VaultPath, deps: FollowDeps): Promise<FollowResult | null> {
  const target = parseTarget(inner);
  if ('invalid' in target) return null;
  const key = targetKey(target);
  const path = await deps.resolve(from, key);
  if (path) return { path, heading: target.heading, created: false };

  const dir = target.folders.length > 0 || target.from_root ? await ensureFolders(target.folders, deps) : from.slice(0, -1);
  const name = `${target.name}.md`;
  try {
    return { path: await deps.noteCreate(dir, name), heading: null, created: true };
  } catch (error) {
    // Заметку успели создать (другое окно, Проводник): открыть её.
    if (error instanceof CoreCallError && error.error.code === 'already_exists') {
      const again = await deps.resolve(from, key);
      if (again) return { path: again, heading: target.heading, created: false };
    }
    throw error;
  }
}
