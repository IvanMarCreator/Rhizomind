// Модель дерева файлов (С3.2) без фреймворка: плоский список элементов от ядра
// (`tree_list`) → дерево со своей сортировкой, применение изменений из
// события `vault:changed`, имена для показа и правила перетаскивания.
import type { EntryKind, TreeChange, TreeEntry, VaultPath } from '../api/index.ts';

/** Элементы дерева по ключу пути. */
export type EntryMap = ReadonlyMap<string, TreeEntry>;

export interface TreeNode {
  /** Имя на диске (заметка — с `.md`). */
  name: string;
  path: VaultPath;
  kind: EntryKind;
  /** Содержимое папки, отсортированное; у файлов пусто. */
  children: TreeNode[];
}

export interface TreeRow {
  node: TreeNode;
  /** Уровень вложенности: 1 — корень хранилища. */
  level: number;
}

/**
 * Ключ пути для карт и наборов. `/` в имени запрещён правилами имён
 * (С3.2), поэтому склейка однозначна.
 */
export function pathKey(path: VaultPath): string {
  return path.join('/');
}

export function samePath(a: VaultPath, b: VaultPath): boolean {
  return a.length === b.length && a.every((part, i) => part === b[i]);
}

/** `path` — это `prefix` или лежит внутри него. */
export function isInside(path: VaultPath, prefix: VaultPath): boolean {
  return prefix.length <= path.length && prefix.every((part, i) => part === path[i]);
}

export function parentOf(path: VaultPath): VaultPath {
  return path.slice(0, -1);
}

/** Скрыто в дереве: `.rhizomind/`, всё, что начинается с точки, и всё внутри таких папок. */
export function isHidden(path: VaultPath): boolean {
  return path.some((part) => part.startsWith('.'));
}

/** Естественный порядок по-русски: «Заметка 2» перед «Заметка 10» (С5.3). */
const collator = new Intl.Collator('ru', { numeric: true });

function compareNodes(a: TreeNode, b: TreeNode): number {
  const folderFirst = Number(b.kind === 'folder') - Number(a.kind === 'folder');
  return folderFirst || collator.compare(a.name, b.name);
}

export function entryMap(entries: readonly TreeEntry[]): Map<string, TreeEntry> {
  return new Map(entries.map((entry) => [pathKey(entry.path), { kind: entry.kind, path: [...entry.path] }]));
}

/** Дерево из плоского списка: скрытые элементы убраны, недостающие папки достроены. */
export function buildTree(entries: EntryMap): TreeNode[] {
  const root: TreeNode = { name: '', path: [], kind: 'folder', children: [] };
  const folders = new Map<string, TreeNode>([['', root]]);

  const folderAt = (path: VaultPath): TreeNode => {
    const key = pathKey(path);
    const known = folders.get(key);
    if (known) return known;
    const node: TreeNode = { name: path.at(-1) ?? '', path, kind: 'folder', children: [] };
    folders.set(key, node);
    folderAt(parentOf(path)).children.push(node);
    return node;
  };

  for (const entry of entries.values()) {
    if (entry.path.length === 0 || isHidden(entry.path)) continue;
    if (entry.kind === 'folder') folderAt(entry.path);
    else folderAt(parentOf(entry.path)).children.push({ name: entry.path.at(-1) ?? '', ...entry, children: [] });
  }

  const sort = (node: TreeNode) => {
    node.children.sort(compareNodes);
    node.children.forEach(sort);
  };
  sort(root);
  return root.children;
}

/** Строки, которые видны при раскрытых папках `expanded` (ключи путей). */
export function flattenVisible(roots: readonly TreeNode[], expanded: ReadonlySet<string>): TreeRow[] {
  const rows: TreeRow[] = [];
  const visit = (nodes: readonly TreeNode[], level: number) => {
    for (const node of nodes) {
      rows.push({ node, level });
      if (node.kind === 'folder' && expanded.has(pathKey(node.path))) visit(node.children, level + 1);
    }
  };
  visit(roots, 1);
  return rows;
}

// ---- Наборы ключей (раскрытые папки) -------------------------------------------
// Наборы не меняются на месте: каждый шаг даёт новый (состояние — `$state.raw`).

export function keySet(keys: Iterable<string>): ReadonlySet<string> {
  return new Set(keys);
}

export function toggleKey(keys: ReadonlySet<string>, key: string): ReadonlySet<string> {
  const next = new Set(keys);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

/** Ключи элемента `from` и всего внутри него переезжают на `to`. */
export function remapKeys(keys: ReadonlySet<string>, from: VaultPath, to: VaultPath): ReadonlySet<string> {
  const fromKey = pathKey(from);
  const toKey = pathKey(to);
  return new Set(
    [...keys].map((key) => (key === fromKey || key.startsWith(`${fromKey}/`) ? toKey + key.slice(fromKey.length) : key)),
  );
}

// ---- Имена ------------------------------------------------------------------

/** Расширение файла с точкой; у папок и имён без точки (или с точкой только в начале) — пусто. */
function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot) : '';
}

/** Имя для правки: у файлов правится часть без расширения, расширение сохраняется. */
export function splitName(entry: TreeEntry): { stem: string; ext: string } {
  const name = entry.path.at(-1) ?? '';
  const ext = entry.kind === 'folder' ? '' : extensionOf(name);
  return { stem: name.slice(0, name.length - ext.length), ext };
}

export function joinName(stem: string, ext: string): string {
  return stem + ext;
}

/** Имя в строке дерева: заметка и вложение — без расширения, папка — как есть. */
export function displayName(entry: TreeEntry): string {
  return splitName(entry).stem;
}

/** Метка типа у вложения (`PNG`, `PDF`); у заметок, папок и файлов без расширения — нет. */
export function typeLabel(entry: TreeEntry): string | null {
  if (entry.kind !== 'attachment') return null;
  const ext = extensionOf(entry.path.at(-1) ?? '');
  return ext ? ext.slice(1).toLocaleUpperCase('ru') : null;
}

/** Сравнение имён без учёта регистра и с нормализацией (окончательно решает ядро). */
function nameKey(name: string): string {
  return name.normalize('NFC').toLocaleLowerCase('ru').normalize('NFC');
}

/** Первое свободное имя: «Без названия», «Без названия 1», «Без названия 2»… */
export function untitledName(base: string, ext: string, siblings: readonly string[]): string {
  const taken = new Set(siblings.map(nameKey));
  for (let n = 0; ; n += 1) {
    const name = joinName(n === 0 ? base : `${base} ${n}`, ext);
    if (!taken.has(nameKey(name))) return name;
  }
}

/** Имена в папке `dir` (для подбора свободного имени). */
export function childNames(entries: EntryMap, dir: VaultPath): string[] {
  const names: string[] = [];
  for (const entry of entries.values()) {
    if (entry.path.length === dir.length + 1 && isInside(entry.path, dir)) names.push(entry.path.at(-1) ?? '');
  }
  return names;
}

// ---- Изменения ----------------------------------------------------------------

/** Вид файла по имени: так же делит ядро (`.md` — заметка). */
function fileKind(name: string): EntryKind {
  return /\.md$/i.test(name) ? 'note' : 'attachment';
}

export interface AppliedChanges {
  entries: EntryMap;
  /** Изменение не сходится с моделью (переименован неизвестный элемент): перечитать `tree_list`. */
  stale: boolean;
}

/**
 * Применяет изменения дерева. Повтор уже применённого изменения (своя
 * операция, о которой потом сообщило слежение за папкой) ничего не меняет.
 */
export function applyChanges(entries: EntryMap, changes: readonly TreeChange[]): AppliedChanges {
  let next: Map<string, TreeEntry> | null = null;
  const draft = () => (next ??= new Map(entries));
  let stale = false;

  for (const change of changes) {
    switch (change.kind) {
      case 'modified':
        break;
      case 'created':
        draft().set(pathKey(change.path), { kind: change.entry_kind, path: [...change.path] });
        break;
      case 'removed':
        for (const [key, entry] of draft()) if (isInside(entry.path, change.path)) draft().delete(key);
        break;
      case 'renamed': {
        const map = draft();
        const moved = [...map.values()].filter((entry) => isInside(entry.path, change.from));
        if (moved.length === 0) {
          stale ||= !map.has(pathKey(change.to));
          break;
        }
        for (const entry of moved) {
          map.delete(pathKey(entry.path));
          const path = [...change.to, ...entry.path.slice(change.from.length)];
          const kind = entry.kind === 'folder' ? 'folder' : fileKind(path.at(-1) ?? '');
          map.set(pathKey(path), { kind, path });
        }
        break;
      }
    }
  }
  return { entries: next ?? entries, stale };
}

// ---- Перетаскивание -------------------------------------------------------------

/** Можно ли переместить `path` в папку `dir`: не в ту же папку, не в себя и не внутрь себя. */
export function canMove(path: VaultPath, dir: VaultPath): boolean {
  if (samePath(parentOf(path), dir)) return false;
  return !isInside(dir, path);
}
