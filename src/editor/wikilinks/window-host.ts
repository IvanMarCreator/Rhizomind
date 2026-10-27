// `WikiHost` окна на командах ядра (С3.4): разрешение (`links_resolve`),
// подсказки (`links_suggest`), переход и создание заметки по ссылке
// (`note_create`, `folder_create`, `tree_list`). Вики-ссылка открывается
// только как заметка хранилища — через `openNote` окна (история переходов),
// никогда через `open_external`.
import { core, type VaultPath } from '../../api/index.ts';
import { LIMITS } from '../../api/limits.ts';
import { userMessage } from '../../i18n/errors.ts';
import { t } from '../../i18n/index.ts';
import type { WikiEvent, WikiHost } from './host.ts';
import { followWikilink, type FollowDeps } from './open.ts';

export interface WindowWikiOptions {
  /** Открытая заметка — откуда ведут ссылки; `null` — заметки нет. */
  from: () => VaultPath | null;
  /** Открыть заметку в окне (новый шаг истории). */
  openNote: (path: VaultPath) => void;
  notify: (text: string) => void;
}

const coreDeps: FollowDeps = {
  resolve: async (from, target) => (await core.linksResolve({ from, targets: [target] })).results[0]?.path ?? null,
  treeList: async () => (await core.treeList()).entries,
  folderCreate: async (dir, name) => (await core.folderCreate({ dir, name })).path,
  noteCreate: async (dir, name) => (await core.noteCreate({ dir, name })).path,
};

const samePath = (a: VaultPath, b: VaultPath) => a.length === b.length && a.every((part, i) => part === b[i]);

export class WindowWikiHost implements WikiHost {
  readonly #options: WindowWikiOptions;
  readonly #deps: FollowDeps;
  readonly #listeners = new Set<(event: WikiEvent) => void>();
  /** Переход (заголовок или начало), когда откроется заметка `path`. */
  #pending: { path: VaultPath; heading: string | null } | null = null;

  constructor(options: WindowWikiOptions, deps: FollowDeps = coreDeps) {
    this.#options = options;
    this.#deps = deps;
  }

  async resolve(keys: readonly string[]): Promise<ReadonlyMap<string, boolean>> {
    const from = this.#options.from();
    const answers = new Map<string, boolean>();
    if (!from) return answers;
    for (let i = 0; i < keys.length; i += LIMITS.max_link_targets) {
      const targets = keys.slice(i, i + LIMITS.max_link_targets);
      const { results } = await core.linksResolve({ from, targets });
      results.forEach((result, k) => answers.set(targets[k] ?? result.target, result.path !== null));
    }
    return answers;
  }

  async suggest(query: string) {
    const from = this.#options.from();
    if (!from) return [];
    return (await core.linksSuggest({ from, query })).items;
  }

  open(inner: string): void {
    void this.follow(inner);
  }

  /** Переход по ссылке; промис — для тестов. */
  async follow(inner: string): Promise<void> {
    const from = this.#options.from();
    if (!from) return;
    try {
      const result = await followWikilink(inner, from, this.#deps);
      if (!result) return;
      if (samePath(result.path, from)) {
        this.#emit({ kind: 'arrive', heading: result.heading });
        return;
      }
      this.#pending = { path: result.path, heading: result.heading };
      this.#options.openNote(result.path);
    } catch (error) {
      this.#options.notify(t('editor.wikilink.openFailed', { message: userMessage(error) }));
    }
  }

  /**
   * Окно показывает заметку `path` (или ничего). Ждущий переход к другой
   * заметке сбрасывается: переход не завершился (сразу открыли другую,
   * Ctrl+O, «назад»), и при случайном открытии той заметки позже он не
   * сработает.
   */
  shown(path: VaultPath | null): void {
    if (this.#pending && (!path || !samePath(this.#pending.path, path))) this.#pending = null;
  }

  /** Дерево хранилища изменилось (`vault:changed`): разрешение ссылок — заново. */
  refresh(): void {
    this.#emit({ kind: 'refresh' });
  }

  subscribe(listener: (event: WikiEvent) => void): () => void {
    this.#listeners.add(listener);
    // Ждущий переход — всем подписчикам нового редактора, когда он создан.
    queueMicrotask(() => {
      const from = this.#options.from();
      const pending = this.#pending;
      if (!pending || !from) return;
      // Ждущий переход — только для заметки из ссылки; открылась другая — сброс.
      this.#pending = null;
      if (!samePath(pending.path, from)) return;
      this.#emit({ kind: 'arrive', heading: pending.heading });
    });
    return () => this.#listeners.delete(listener);
  }

  #emit(event: WikiEvent): void {
    for (const listener of [...this.#listeners]) listener(event);
  }
}
