// Состояние дерева файлов (С3.2): элементы хранилища, раскрытые папки, выбор,
// поле переименования, ожидающее подтверждения удаление. Все операции с
// файлами — командами ядра; оболочке окна дерево сообщает только итог
// (открыть заметку, путь изменился, элемент удалён) через TreeHost.
import { CoreCallError, core, onCoreEvent } from '../api/index.ts';
import type {
  EntryKind,
  HotkeyOverrides,
  TreeChange,
  TreeEntry,
  Unsubscribe,
  VaultChangedPayload,
  VaultPath,
} from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { hasMessage, t } from '../i18n/index.ts';
import {
  applyChanges,
  buildTree,
  canMove,
  childNames,
  displayName,
  entryMap,
  flattenVisible,
  isInside,
  parentOf,
  keySet,
  pathKey,
  remapKeys,
  samePath,
  toggleKey,
  untitledName,
  type EntryMap,
} from './model.ts';

/** Что дерево ждёт от оболочки окна (реализует WindowModel). */
export interface TreeHost {
  /** Путь открытой заметки; `null` — заметка не открыта. */
  readonly currentPath: VaultPath | null;
  /** Настройка хранилища «подтверждение удаления». */
  readonly confirmDelete: boolean;
  /** Назначения горячих клавиш хранилища (F2, Delete в дереве); нет — по умолчанию. */
  readonly hotkeys?: HotkeyOverrides;
  open(path: VaultPath): void;
  /** Элемент переименован или перемещён в приложении: пути внутри меняются. */
  entryRenamed(from: VaultPath, to: VaultPath): void;
  /** Элемент удалён в приложении: открытая заметка внутри закрывается. */
  entryRemoved(path: VaultPath): void;
  notify(text: string): void;
}

/** Сколько раз подбирать свободное «Без названия», если ядро отвечает «занято». */
const UNTITLED_ATTEMPTS = 20;

function isDuplicate(error: unknown): boolean {
  if (!(error instanceof CoreCallError)) return false;
  const failure = error.error;
  return failure.code === 'already_exists' || (failure.code === 'invalid_name' && failure.name_problem === 'duplicate');
}

export class FileTreeState {
  // Карта и набор заменяются целиком (`$state.raw`), а не меняются на месте.
  entries = $state.raw<EntryMap>(entryMap([]));
  /** Раскрытые папки (ключи путей). */
  expanded = $state.raw<ReadonlySet<string>>(keySet([]));
  selected = $state.raw<VaultPath | null>(null);
  /** Элемент с открытым полем переименования. */
  renaming = $state.raw<VaultPath | null>(null);
  /** Удаление, ждущее подтверждения в окне. */
  pendingDelete = $state.raw<TreeEntry | null>(null);
  /** Первое чтение дерева закончилось (успешно или с ошибкой). */
  loaded = $state(false);

  readonly roots = $derived(buildTree(this.entries));
  readonly rows = $derived(flattenVisible(this.roots, this.expanded));

  readonly host: TreeHost;
  #loadSeq = 0;
  /**
   * Последнее переименование в этом окне — для Ctrl+Z (решение 47); стека нет.
   * `gone` — элемент с тех пор удалили (в том числе снаружи): новый файл с тем
   * же именем — уже другой, его отмена не трогает (ревью 2.3, задача 2.9).
   */
  #lastRename: { from: VaultPath; to: VaultPath; gone?: boolean } | null = null;
  /** Элементы, удаление которых уже идёт: повторный Delete их не трогает. */
  readonly #deleting: VaultPath[] = [];

  constructor(host: TreeHost) {
    this.host = host;
  }

  /** Подписка на `vault:changed` и первое чтение дерева. Возвращает отписку. */
  start(): () => void {
    let disposed = false;
    let unlisten: Unsubscribe | null = null;
    onCoreEvent('vault:changed', (payload) => {
      if (!disposed) this.applyEvent(payload);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error: unknown) => this.host.notify(userMessage(error)));
    void this.load();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }

  /** Перечитать дерево целиком (`tree_list`). Ответ устаревшего чтения отбрасывается. */
  async load(): Promise<void> {
    const seq = ++this.#loadSeq;
    try {
      const { entries } = await core.treeList();
      if (seq !== this.#loadSeq) return;
      this.entries = entryMap(entries);
      this.#dropMissing();
    } catch (error) {
      if (seq === this.#loadSeq) this.host.notify(userMessage(error));
    } finally {
      if (seq === this.#loadSeq) this.loaded = true;
    }
  }

  applyEvent(payload: VaultChangedPayload): void {
    if (payload.rescan) {
      void this.load();
      return;
    }
    if (this.#apply(payload.changes)) void this.load();
  }

  entry(path: VaultPath): TreeEntry | undefined {
    return this.entries.get(pathKey(path));
  }

  isExpanded(path: VaultPath): boolean {
    return this.expanded.has(pathKey(path));
  }

  toggle(path: VaultPath): void {
    this.expanded = toggleKey(this.expanded, pathKey(path));
  }

  setExpanded(path: VaultPath, open: boolean): void {
    if (this.isExpanded(path) !== open) this.toggle(path);
  }

  /** Раскрыть папку и все папки над ней. */
  reveal(dir: VaultPath): void {
    const keys = dir.map((_, i) => pathKey(dir.slice(0, i + 1)));
    this.expanded = keySet([...this.expanded, ...keys]);
  }

  select(path: VaultPath | null): void {
    this.selected = path;
  }

  /** Клик или Enter: папка раскрывается, заметка открывается, вложение только выбирается (С3.2). */
  activate(path: VaultPath): void {
    const entry = this.entry(path);
    if (!entry) return;
    this.selected = entry.path;
    if (entry.kind === 'folder') this.toggle(entry.path);
    else if (entry.kind === 'note') this.host.open(entry.path);
  }

  // ---- Создание ------------------------------------------------------------

  /** Новая заметка «Без названия» в папке `dir`: создаётся, открывается, поле имени открыто. */
  createNote(dir: VaultPath): Promise<void> {
    return this.#createUntitled(dir, 'note');
  }

  /** Новая папка «Без названия» в папке `dir`, поле имени открыто. */
  createFolder(dir: VaultPath): Promise<void> {
    return this.#createUntitled(dir, 'folder');
  }

  async #createUntitled(dir: VaultPath, kind: Exclude<EntryKind, 'attachment'>): Promise<void> {
    const ext = kind === 'note' ? '.md' : '';
    const taken = childNames(this.entries, dir);
    for (let attempt = 0; attempt < UNTITLED_ATTEMPTS; attempt += 1) {
      const name = untitledName(t('tree.untitled'), ext, taken);
      try {
        const request = { dir, name };
        const { path } = kind === 'note' ? await core.noteCreate(request) : await core.folderCreate(request);
        this.#apply([{ kind: 'created', path, entry_kind: kind }]);
        this.reveal(dir);
        this.selected = path;
        if (kind === 'note') this.host.open(path);
        this.renaming = path;
        return;
      } catch (error) {
        if (!isDuplicate(error)) {
          this.host.notify(userMessage(error));
          return;
        }
        taken.push(name);
      }
    }
  }

  // ---- Переименование и перемещение ----------------------------------------

  startRename(path: VaultPath): void {
    if (!this.entry(path)) return;
    this.selected = path;
    this.renaming = path;
  }

  cancelRename(): void {
    this.renaming = null;
  }

  /**
   * Переименование: `newName` — полное имя на диске. Успех — `null`; отказ —
   * текст для показа под полем ввода. Ссылки в 0.1 не обновляются (С3.2).
   */
  async rename(path: VaultPath, newName: string): Promise<string | null> {
    try {
      const { path: to } = await core.entryRename({ path, new_name: newName });
      this.#relocated(path, to);
      this.#lastRename = samePath(path, to) ? null : { from: path, to };
      // Поле в дереве закрывается, только если оно было на этом элементе
      // (переименование из заголовка заметки не трогает поле на другом).
      if (this.renaming && samePath(this.renaming, to)) this.renaming = null;
      return null;
    } catch (error) {
      return userMessage(error);
    }
  }

  /**
   * Ctrl+Z в дереве или в заголовке заметки: последнее переименование
   * отменяется, если элемент на месте и прежнее имя свободно (`name_check`);
   * иначе — уведомление, ничего не меняется. Отмена одна: повтор ничего не делает.
   */
  async undoRename(): Promise<void> {
    const last = this.#lastRename;
    if (!last) return;
    this.#lastRename = null;
    const oldName = last.from.at(-1) ?? '';
    if (last.gone || !this.entry(last.to)) {
      this.host.notify(t('tree.undo.missing'));
      return;
    }
    try {
      const dir = parentOf(last.to);
      const check = await core.nameCheck({ dir, name: oldName, renaming: last.to });
      if (check.status === 'rejected') {
        this.host.notify(
          check.problem === 'duplicate'
            ? t('tree.undo.taken', { name: displayName({ kind: this.entry(last.to)?.kind ?? 'note', path: last.from }) })
            : t('tree.undo.failed', { reason: hasMessage(check.message_key) ? t(check.message_key) : '' }),
        );
        return;
      }
      const { path: to } = await core.entryRename({ path: last.to, new_name: oldName });
      this.#relocated(last.to, to);
    } catch (error) {
      if (error instanceof CoreCallError && error.error.code === 'not_found') this.host.notify(t('tree.undo.missing'));
      else this.host.notify(t('tree.undo.failed', { reason: userMessage(error) }));
    }
  }

  /** Перемещение перетаскиванием в папку `dir` (`[]` — корень). */
  async move(path: VaultPath, dir: VaultPath): Promise<void> {
    if (!canMove(path, dir)) return;
    try {
      const { path: to } = await core.entryMove({ path, new_dir: dir });
      this.#relocated(path, to);
    } catch (error) {
      this.host.notify(userMessage(error));
    }
  }

  #relocated(from: VaultPath, to: VaultPath): void {
    this.#apply([{ kind: 'renamed', from, to }]);
    this.host.entryRenamed(from, to);
  }

  // ---- Удаление ------------------------------------------------------------

  /**
   * Удаление `path` (или папки над ним) идёт в этом окне: событие ядра об
   * удалении может прийти раньше ответа команды — это не удаление «на диске».
   */
  isDeleting(path: VaultPath): boolean {
    return this.#deleting.some((deleting) => isInside(path, deleting));
  }

  /** Delete или пункт меню: подтверждение по настройке, затем в корзину. */
  requestDelete(path: VaultPath): void {
    const entry = this.entry(path);
    if (!entry || this.#deleting.some((deleting) => samePath(deleting, entry.path))) return;
    if (this.host.confirmDelete) this.pendingDelete = entry;
    else void this.#trash(entry);
  }

  cancelDelete(): void {
    this.pendingDelete = null;
  }

  async confirmPendingDelete(): Promise<void> {
    const entry = this.pendingDelete;
    this.pendingDelete = null;
    if (entry) await this.#trash(entry);
  }

  /**
   * В корзину ОС. На диске без корзины ядро само спрашивает о безвозвратном
   * удалении своим окном (инвариант 4): ответ `cancelled` — ничего не удалено.
   */
  async #trash(entry: TreeEntry): Promise<void> {
    if (this.#deleting.some((deleting) => samePath(deleting, entry.path))) return;
    const deleting = entry.path;
    this.#deleting.push(deleting);
    try {
      const result = await core.entryTrash({ path: entry.path });
      if (result.outcome === 'cancelled') return;
      this.#apply([{ kind: 'removed', path: entry.path }]);
      this.host.entryRemoved(entry.path);
      if (result.outcome === 'deleted_permanently') {
        this.host.notify(t('tree.deletedPermanently', { name: entry.path.at(-1) ?? '' }));
      }
    } catch (error) {
      this.host.notify(userMessage(error));
    } finally {
      this.#deleting.splice(this.#deleting.indexOf(deleting), 1);
    }
  }

  // ---- Внутреннее ------------------------------------------------------------

  /** Применяет изменения; `true` — модель разошлась с диском, нужно перечитать. */
  #apply(changes: readonly TreeChange[]): boolean {
    const { entries, stale } = applyChanges(this.entries, changes);
    for (const change of changes) {
      if (change.kind === 'removed' && this.#lastRename && isInside(this.#lastRename.to, change.path)) {
        this.#lastRename = { ...this.#lastRename, gone: true };
      }
      if (change.kind !== 'renamed') continue;
      this.expanded = remapKeys(this.expanded, change.from, change.to);
      if (this.selected && isInside(this.selected, change.from)) {
        this.selected = [...change.to, ...this.selected.slice(change.from.length)];
      }
      if (this.#lastRename && isInside(this.#lastRename.to, change.from)) {
        const { from, to } = this.#lastRename;
        this.#lastRename = { from, to: [...change.to, ...to.slice(change.from.length)] };
      }
      if (this.renaming && isInside(this.renaming, change.from)) {
        this.renaming = [...change.to, ...this.renaming.slice(change.from.length)];
      }
    }
    this.entries = entries;
    this.#dropMissing();
    return stale;
  }

  /** Выбор и поле имени у исчезнувшего элемента снимаются. */
  #dropMissing(): void {
    if (this.selected && !this.entry(this.selected)) this.selected = null;
    if (this.renaming && !this.entry(this.renaming)) this.renaming = null;
  }
}

