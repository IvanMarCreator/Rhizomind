// Внешние изменения файлов в окне хранилища (С3.6, задача 2.9, инвариант 4).
//
// Ядро следит за папкой хранилища и сообщает окну (src-tauri/src/events/):
// - `note:changed` — заметка изменилась снаружи (с версией с диска);
// - `note:renamed` — заметка переименована или перемещена снаружи;
// - `note:deleted` — заметка удалена снаружи или переименована в не-заметку
//   (`a.md` → `a.txt`, решение 90: уход из заметок — как удаление);
// - `vault:changed` с `rescan: true` — была полная сверка с диском (переполнение
//   буфера ОС, хранилище снова доступно): отдельного `note:changed` могло не быть.
// Свои записи окна ядро событиями не считает (журнал своих записей,
// src-tauri/src/own_writes.rs); если событие о своём действии всё же пришло
// (наблюдатель опередил ответ команды), оно применяется без вреда: версия
// совпала — ничего; переименование уже применено — ничего; удаление идёт в
// этом окне — без уведомления «удалена на диске».
//
// Здесь — только маршрут событий; что делать с текстом, решает редактор
// (src/editor/sessions.ts, session.ts): без ввода — текст с диска, с вводом —
// обе версии (ядро), удаление — заметка закрывается, ввод отбрасывается
// (решение владельца: внешнее действие — действие пользователя).
import { onCoreEvent, type FileVersion, type TreeChange, type Unsubscribe, type VaultPath } from '../api/index.ts';
import { t } from '../i18n/index.ts';

/** Что маршрут событий ждёт от окна (оболочка — src/shell/Shell.svelte). */
export interface ExternalChangesHost {
  /** Заметка, показанная в окне; `null` — заметки нет. */
  readonly currentPath: VaultPath | null;
  /** Заметку изменили снаружи: редактор сверяет её с диском. */
  changed(path: VaultPath, version: FileVersion): void;
  /** Заметку переименовали или переместили: редактор, история и раскладка следуют за путём. */
  renamed(from: VaultPath, to: VaultPath): void;
  /** Заметку удалили: она закрывается, ввод отбрасывается, шаги истории уходят. */
  removed(path: VaultPath): void;
  /** Была полная сверка с диском: открытые заметки сверяются с диском. */
  rescanned(): void;
  /** Удаление `path` идёт в этом окне (дерево файлов): это не удаление «на диске». */
  deletingHere(path: VaultPath): boolean;
  notify(text: string): void;
}

/** `path` — это `prefix` или лежит внутри него. */
function within(path: VaultPath, prefix: VaultPath): boolean {
  return prefix.length <= path.length && prefix.every((part, i) => part === path[i]);
}

/** Имя заметки для уведомлений: последний компонент без `.md`. */
function noteName(path: VaultPath): string {
  return (path.at(-1) ?? '').replace(/\.md$/i, '');
}

const isNote = (path: VaultPath) => /\.md$/i.test(path.at(-1) ?? '');
const keyOf = (path: VaultPath) => JSON.stringify(path);

export class ExternalChanges {
  readonly #host: ExternalChangesHost;
  /**
   * Заметки, переименованные снаружи в не-заметку (`a.md` → `a.txt`), из
   * последней пачки `vault:changed`: ядро присылает за ней `note:deleted`
   * (решение 90), уведомление называет новое имя файла.
   */
  #leftNotes = new Map<string, VaultPath>();

  constructor(host: ExternalChangesHost) {
    this.#host = host;
  }

  /** Подписка на события ядра своего окна. Возвращает отписку. */
  start(): () => void {
    let disposed = false;
    const stops: Unsubscribe[] = [];
    const subscribe = (subscription: Promise<Unsubscribe>) => {
      subscription
        .then((stop) => {
          if (disposed) stop();
          else stops.push(stop);
        })
        .catch(() => undefined);
    };
    const live =
      <A extends unknown[]>(handler: (...args: A) => void) =>
      (...args: A) => {
        if (!disposed) handler(...args);
      };
    subscribe(onCoreEvent('note:changed', live(({ path, version }) => this.changed(path, version))));
    subscribe(onCoreEvent('note:renamed', live(({ from, to }) => this.renamed(from, to))));
    subscribe(onCoreEvent('note:deleted', live(({ path }) => this.deleted(path))));
    subscribe(
      onCoreEvent(
        'vault:changed',
        live(({ rescan, changes }) => {
          this.treeChanged(changes);
          if (rescan) this.#host.rescanned();
        }),
      ),
    );
    return () => {
      disposed = true;
      for (const stop of stops.splice(0)) stop();
    };
  }

  /** Пачка изменений дерева: запомнить переименования заметок в не-заметки. */
  treeChanged(changes: readonly TreeChange[]): void {
    this.#leftNotes = new Map(
      changes.flatMap((change) =>
        change.kind === 'renamed' && isNote(change.from) && !isNote(change.to)
          ? [[keyOf(change.from), change.to] as const]
          : [],
      ),
    );
  }

  changed(path: VaultPath, version: FileVersion): void {
    this.#host.changed(path, version);
  }

  renamed(from: VaultPath, to: VaultPath): void {
    this.#host.renamed(from, to);
  }

  /**
   * Заметку удалили снаружи: открытая закрывается с уведомлением «удалена на
   * диске» (С3.6). Удаление, которое идёт в этом окне, — без уведомления.
   */
  deleted(path: VaultPath): void {
    const current = this.#host.currentPath;
    const shown = current !== null && within(current, path);
    if (shown && !this.#host.deletingHere(path)) {
      const renamedTo = this.#leftNotes.get(keyOf(path));
      this.#host.notify(
        renamedTo
          ? t('editor.noLongerNote', { name: noteName(current), file: renamedTo.at(-1) ?? '' })
          : t('editor.deletedOnDisk', { name: noteName(current) }),
      );
    }
    this.#host.removed(path);
  }
}
