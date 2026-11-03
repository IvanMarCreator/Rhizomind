// Состояние дерева файлов (С3.2): элементы хранилища, раскрытые папки, выбор,
// поле переименования, ожидающее подтверждения удаление, окно вопроса об
// обновлении ссылок (0.2-5.2). Все операции с файлами — командами ядра;
// оболочке окна дерево сообщает только итог (открыть заметку, путь изменился,
// элемент удалён) через TreeHost.
import { CoreCallError, core, onCoreEvent } from '../api/index.ts';
import type {
  EntryKind,
  EntryMovedResponse,
  HotkeyOverrides,
  IndexProgress,
  IndexStatus,
  LinksMode,
  RelinkPreview,
  RelinkPreviewRequest,
  RenameLinks,
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
  /** Настройка хранилища «При переименовании заметки, на которую есть ссылки» (С2-3.5); нет — «Всегда спрашивать». */
  readonly renameLinks?: RenameLinks;
  /** Галочка «Больше не спрашивать» с ответом «Обновить ссылки» (решение 109): записать настройку. */
  setRenameLinks?(mode: RenameLinks): void;
  open(path: VaultPath): void;
  /** Элемент переименован или перемещён в приложении: пути внутри меняются. */
  entryRenamed(from: VaultPath, to: VaultPath): void;
  /** Элемент удалён в приложении: открытая заметка внутри закрывается. */
  entryRemoved(path: VaultPath): void;
  notify(text: string): void;
  /**
   * Ctrl+Z с обновлением ссылок начинается: итог этого переименования —
   * «Переименование отменено, ссылки возвращены…» (ревью 0.2-5.2, М4).
   */
  undoingRename?(rename: { from: VaultPath; to: VaultPath }): void;
}

/** Сколько раз подбирать свободное «Без названия», если ядро отвечает «занято». */
const UNTITLED_ATTEMPTS = 20;

/** Пока индекс строится, предпросмотр спрашивается снова не реже, чем раз в столько (кроме `index:status`). */
const INDEX_POLL_MS = 1_000;

/** Переименование или перемещение, для которого нужен ответ об обновлении ссылок. */
export type RelinkOp =
  | { kind: 'rename'; path: VaultPath; newName: string }
  | { kind: 'move'; path: VaultPath; dir: VaultPath };

/** Ответ в окне вопроса (решение 109). */
export type RenameAnswer = 'update' | 'keep' | 'cancel';

/**
 * Окно вопроса при переименовании (макеты Д-А, Д-1). `waiting` — индекс
 * строится (решение 114): в режиме «Всегда спрашивать» кнопки недоступны, в
 * режиме «Всегда обновлять» — только «Отменить переименование».
 */
export interface RenameQuestion {
  readonly state: 'waiting' | 'ask';
  /** Режим на момент вопроса: в окне ожидания — какие кнопки показать. */
  readonly mode: RenameLinks;
  readonly op: 'rename' | 'move';
  /** Что переименовывается (заметка или папка). */
  readonly kind: EntryKind;
  /** Имя для заголовка: заметка — без `.md`, папка — имя папки. */
  readonly name: string;
  /** «Было → станет» в плашке: имена у переименования заметки, пути — у папки и переноса. */
  readonly from: string;
  readonly to: string;
  /** Сколько заметок ссылается (только `ask`). */
  readonly notes: number;
  /** Ход сборки индекса (только `waiting`); `total: 0` — неизвестно. */
  readonly progress: IndexProgress | null;
}

/** Итог переименования с вопросом: путь и ответ — для Ctrl+Z (Р9); `null` — отменено. */
type RelinkResult = { path: VaultPath; links: LinksMode } | null;

const stem = (name: string) => name.replace(/\.md$/i, '');

function changeText(kind: EntryKind, path: VaultPath, to: VaultPath, op: RelinkOp['kind']): { from: string; to: string } {
  if (op === 'rename' && kind !== 'folder') return { from: stem(path.at(-1) ?? ''), to: stem(to.at(-1) ?? '') };
  const show = (p: VaultPath) => (kind === 'folder' ? p : [...p.slice(0, -1), stem(p.at(-1) ?? '')]).join('/');
  return { from: show(path), to: show(to) };
}

function targetOf(op: RelinkOp): VaultPath {
  return op.kind === 'rename' ? [...parentOf(op.path), op.newName] : [...op.dir, op.path.at(-1) ?? ''];
}

function previewRequest(op: RelinkOp): RelinkPreviewRequest {
  return op.kind === 'rename'
    ? { op: 'rename', path: op.path, new_name: op.newName }
    : { op: 'move', path: op.path, new_dir: op.dir };
}

/** Знак плана не принят (просрочен, окно ждало дольше 10 минут): спросить предпросмотр заново. */
function isStaleToken(error: unknown): boolean {
  return error instanceof CoreCallError && error.error.code === 'invalid_argument' && error.error.field === 'token';
}

/**
 * Текст ошибки переименования. `io_failed` при обновлении ссылок: ввод не
 * записан (или файл занят) — переименование не выполнено (ревью 0.2-4.6).
 */
function renameError(error: unknown, links: LinksMode): string {
  if (links === 'update' && error instanceof CoreCallError && error.error.code === 'io_failed') {
    return t('relink.renameNotDone');
  }
  return userMessage(error);
}

/** Отказ переименования с готовым текстом для показа (ошибка ядра — в `cause`). */
class RelinkFailed extends Error {
  constructor(message: string, cause: unknown) {
    super(message, { cause });
    this.name = 'RelinkFailed';
  }
}

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
  /** Окно вопроса об обновлении ссылок (0.2-5.2); `null` — закрыто. */
  question = $state.raw<RenameQuestion | null>(null);
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
  #lastRename: { from: VaultPath; to: VaultPath; links: LinksMode; gone?: boolean } | null = null;
  /** Ответ окна вопроса ждёт переименование (или отмена во время ожидания индекса). */
  #answer: ((answer: RenameAnswer, dontAsk: boolean) => void) | null = null;
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
   * Переименование: `newName` — полное имя на диске. Успех или отмена в окне
   * вопроса — `null`; отказ — текст для показа под полем ввода. Ссылки — по
   * настройке и ответу (С2-3.5): переписывать нечего — сразу, «Всегда
   * обновлять» — сразу с обновлением, «Всегда спрашивать» — окно до
   * переименования; пока индекс строится — окно ожидания (решение 114).
   */
  async rename(path: VaultPath, newName: string): Promise<string | null> {
    if (this.question) return null;
    try {
      const done = await this.#relink({ kind: 'rename', path, newName }, true);
      if (!done) return null;
      this.#lastRename = samePath(path, done.path) ? null : { from: path, to: done.path, links: done.links };
      // Поле в дереве закрывается, только если оно было на этом элементе
      // (переименование из заголовка заметки не трогает поле на другом).
      if (this.renaming && samePath(this.renaming, done.path)) this.renaming = null;
      return null;
    } catch (error) {
      return error instanceof RelinkFailed ? error.message : userMessage(error);
    }
  }

  /**
   * Ctrl+Z в дереве или в заголовке заметки: последнее переименование
   * отменяется, если элемент на месте и прежнее имя свободно (`name_check`);
   * иначе — уведомление, ничего не меняется. Отмена одна: повтор ничего не делает.
   * Ссылки — по ответу, данному при переименовании, без второго окна (Р9).
   */
  async undoRename(): Promise<void> {
    const last = this.#lastRename;
    if (!last || this.question) return;
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
      const op: RelinkOp = { kind: 'rename', path: last.to, newName: oldName };
      if (last.links === 'update') {
        this.host.undoingRename?.({ from: last.to, to: [...dir, oldName] });
        await this.#relink(op, false);
      }
      else await this.#run(op, 'keep');
    } catch (error) {
      const cause = error instanceof RelinkFailed ? error.cause : error;
      if (cause instanceof CoreCallError && cause.error.code === 'not_found') this.host.notify(t('tree.undo.missing'));
      else this.host.notify(t('tree.undo.failed', { reason: error instanceof RelinkFailed ? error.message : userMessage(error) }));
    }
  }

  /** Перемещение перетаскиванием в папку `dir` (`[]` — корень); ссылки — как у переименования. */
  async move(path: VaultPath, dir: VaultPath): Promise<void> {
    if (!canMove(path, dir) || this.question) return;
    try {
      await this.#relink({ kind: 'move', path, dir }, true);
    } catch (error) {
      this.host.notify(error instanceof RelinkFailed ? error.message : userMessage(error));
    }
  }

  /** Ответ в окне вопроса; `dontAsk` — галочка «Больше не спрашивать» (только с «Обновить ссылки»). */
  answerQuestion(answer: RenameAnswer, dontAsk = false): void {
    const resolve = this.#answer;
    if (!resolve) return;
    // В окне ожидания доступна только отмена (решение 114).
    if (this.question?.state === 'waiting' && answer !== 'cancel') return;
    this.#answer = null;
    resolve(answer, dontAsk);
  }

  // ---- Обновление ссылок (0.2-5.2, С2-3.5) ------------------------------------

  /**
   * Предпросмотр → (окно ожидания индекса) → (вопрос) → `entry_rename` /
   * `entry_move` с `links` и знаком плана. `ask: false` — Ctrl+Z: ответ уже
   * дан, окна вопроса нет. `null` — пользователь отменил.
   */
  async #relink(op: RelinkOp, ask: boolean): Promise<RelinkResult> {
    try {
      let preview = await this.#preview(op, ask);
      if (!preview) return null;
      let links: LinksMode = 'update';
      if (preview.notes === 0) {
        links = 'keep';
      } else if (ask && (this.host.renameLinks ?? 'ask') === 'ask') {
        const answer = await this.#ask(op, preview);
        if (answer === 'cancel') return null;
        links = answer;
      }
      this.question = null;
      try {
        const path = await this.#run(op, links, preview.token);
        // Нечего было переписывать — Ctrl+Z тоже спросит предпросмотр (вдруг ссылки появились).
        return { path, links: preview.notes === 0 ? 'update' : links };
      } catch (error) {
        if (links !== 'update' || !isStaleToken(error)) throw error;
        // Знак просрочен — новый план и то же решение, без второго окна.
        preview = await this.#preview(op, false);
        if (!preview) return null;
        const retry: LinksMode = preview.notes === 0 ? 'keep' : 'update';
        return { path: await this.#run(op, retry, preview.token), links: 'update' };
      }
    } catch (error) {
      // Ошибки команд переименования уже с текстом; здесь — предпросмотр.
      if (error instanceof CoreCallError) throw new RelinkFailed(userMessage(error), error);
      throw error;
    } finally {
      this.question = null;
      this.#answer = null;
    }
  }

  /** Команда ядра; при `update` — со знаком плана. Новый путь применяется к дереву и оболочке. */
  async #run(op: RelinkOp, links: LinksMode, token?: string): Promise<VaultPath> {
    const extra = links === 'update' && token !== undefined ? { links, token } : {};
    let response: EntryMovedResponse;
    try {
      response =
        op.kind === 'rename'
          ? await core.entryRename({ path: op.path, new_name: op.newName, ...extra })
          : await core.entryMove({ path: op.path, new_dir: op.dir, ...extra });
    } catch (error) {
      if (links === 'update' && isStaleToken(error)) throw error;
      throw new RelinkFailed(renameError(error, links), error);
    }
    this.#relocated(op.path, response.path);
    return response.path;
  }

  /**
   * Предпросмотр. Пока индекс строится — окно ожидания: предпросмотр
   * спрашивается снова по каждому `index:status` и раз в секунду (после
   * `ready` индекс ещё сверяется с диском и может коротко отвечать
   * `index_building`, ревью 0.2-4.6). `null` — отменено в окне ожидания.
   */
  async #preview(op: RelinkOp, ask: boolean): Promise<RelinkPreview | null> {
    let stop: Unsubscribe | null = null;
    let wake: (() => void) | null = null;
    let cancelled = false;
    try {
      for (;;) {
        const reply = await core.relinkPreview(previewRequest(op));
        if (cancelled) return null;
        if (reply.status === 'ready') return reply;
        const progress = { done: reply.done, total: reply.total };
        this.question = this.#question(op, 'waiting', { mode: ask ? (this.host.renameLinks ?? 'ask') : 'update', progress });
        this.#answer = () => {
          cancelled = true;
          wake?.();
        };
        stop ??= await onCoreEvent('index:status', (status: IndexStatus) => {
          if (status.state === 'building' && this.question?.state === 'waiting') {
            this.question = { ...this.question, progress: { done: status.done, total: status.total } };
          } else {
            wake?.();
          }
        });
        await new Promise<void>((resolve) => {
          const timer = setTimeout(() => done(), INDEX_POLL_MS);
          const done = () => {
            clearTimeout(timer);
            wake = null;
            resolve();
          };
          wake = done;
          if (cancelled) done();
        });
        if (cancelled) return null;
      }
    } finally {
      stop?.();
    }
  }

  /** Вопрос «Обновить ссылки?» (решение 109); окно ожидания, если было, становится вопросом. */
  #ask(op: RelinkOp, preview: RelinkPreview): Promise<RenameAnswer> {
    return new Promise((resolve) => {
      this.question = this.#question(op, 'ask', { mode: 'ask', notes: preview.notes });
      this.#answer = (answer, dontAsk) => {
        if (answer === 'update' && dontAsk) this.host.setRenameLinks?.('update');
        resolve(answer);
      };
    });
  }

  #question(
    op: RelinkOp,
    state: RenameQuestion['state'],
    fields: { mode: RenameLinks; notes?: number; progress?: IndexProgress | null },
  ): RenameQuestion {
    const kind = this.entry(op.path)?.kind ?? 'note';
    const name = kind === 'folder' ? (op.path.at(-1) ?? '') : stem(op.path.at(-1) ?? '');
    return {
      state,
      op: op.kind,
      kind,
      name,
      ...changeText(kind, op.path, targetOf(op), op.kind),
      notes: fields.notes ?? 0,
      progress: fields.progress ?? null,
      mode: fields.mode,
    };
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
        const { from, to, links } = this.#lastRename;
        this.#lastRename = { from, to: [...change.to, ...to.slice(change.from.length)], links };
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
