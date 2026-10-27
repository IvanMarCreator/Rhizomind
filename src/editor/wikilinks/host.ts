// Связь редактора с окном для вики-ссылок (С3.4): разрешение, переход,
// подсказки. Расширения CodeMirror берут её из состояния (`wikiHost`), так
// что смена режима (живой предпросмотр / исходный текст) её не теряет.
// Реализация на командах ядра — Editor.svelte (`links_resolve`,
// `links_suggest`, `note_create`, `folder_create`); вики-ссылка никогда не
// уходит в `open_external`.
import { Facet } from '@codemirror/state';
import type { LinkSuggestion } from '../../api/index.ts';

/** Событие окна для открытого редактора. */
export type WikiEvent =
  /** Дерево хранилища изменилось: разрешение ссылок спросить заново. */
  | { kind: 'refresh' }
  /**
   * Переход по ссылке к этой заметке (решение 86): фокус в текст, курсор на
   * заголовок (`[[Заметка#Заголовок]]`) или, без него или если его нет, в начало.
   */
  | { kind: 'arrive'; heading: string | null };

export interface WikiHost {
  /** Разрешены ли цели (ключи `targetKey`); ошибка — ответа нет. */
  resolve(keys: readonly string[]): Promise<ReadonlyMap<string, boolean>>;
  /** Щелчок по ссылке: текст между `[[` и `]]`. */
  open(inner: string): void;
  /** Подсказки автодополнения по набранному после `[[`. */
  suggest(query: string): Promise<readonly LinkSuggestion[]>;
  /**
   * Подписка редактора на события окна. Ждущий переход к этой заметке
   * (её только что открыли по ссылке) приходит сразу.
   */
  subscribe(listener: (event: WikiEvent) => void): () => void;
}

export const wikiHost = Facet.define<WikiHost, WikiHost | null>({
  combine: (values) => values[0] ?? null,
});
