// Модель вида «Теги» (0.2-2.3, С2-3.3 «Список тегов», макет П-2): теги
// хранилища из ядра (`tags_list`), порядок «по имени / по числу заметок»
// (хранится в состоянии правой панели и пишется с раскладкой), раскрытые
// теги и их заметки (`tag_notes`). Пока индекс строится — ход сборки из
// ответа `index_building` и событий `index:status`; по `index:changed`
// список и раскрытые теги перезапрашиваются (тег без заметок ядро больше
// не отдаёт — он исчезает).
//
// Модель живёт в окне дольше вида: редактор (0.2-3.1) вызывает `reveal`,
// когда вида ещё нет на экране; вид подключается (`connect`), пока виден.
import { core, CoreCallError, onCoreEvent, type TagSummary, type Unsubscribe } from '../api/index.ts';
import type { TagOrder } from '../core/layout/layout.ts';
import type { MessageKey } from '../i18n/index.ts';
import type { RightPanelModel } from '../sidebar-right/panel.svelte.ts';
import { noteRows, sortTags, tagKey, type TagNoteRow } from './tags.ts';

/** Что модели нужно от правой панели. */
export type TagsPanel = Pick<RightPanelModel, 'tagOrder' | 'setTagOrder' | 'setCount' | 'reveal'>;

export type TagsStatus =
  | { kind: 'loading' }
  /** `percent: null` — сколько всего, ядро ещё не знает (`total: 0`). */
  | { kind: 'building'; percent: number | null }
  | { kind: 'ready' }
  | { kind: 'error'; message: MessageKey };

export type TagNotesState =
  | { kind: 'loading' }
  | { kind: 'ready'; notes: TagNoteRow[]; total: number }
  | { kind: 'error'; message: MessageKey };

function percent(done: number, total: number): number | null {
  return total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : null;
}

function errorKey(error: unknown): MessageKey {
  return error instanceof CoreCallError ? (error.error.message_key as MessageKey) : 'errors.ipc_failed';
}

export class TagsModel {
  status = $state<TagsStatus>({ kind: 'loading' });
  /** Теги в порядке ядра; показ — `tags` (в выбранном порядке). */
  #tags = $state.raw<TagSummary[]>([]);
  /** Раскрытые теги по ключу, в порядке раскрытия. */
  #expanded = $state.raw<string[]>([]);
  #notes = $state.raw<Readonly<Record<string, TagNotesState>>>({});
  /** Тег, раскрытый вызовом `reveal`: вид прокручивает к нему один раз. */
  revealed = $state<string | null>(null);

  readonly #panel: TagsPanel;
  /** Поколение запроса списка: ответ старого запроса не затирает новый. */
  #generation = 0;
  /**
   * Последний запрос заметок по ключу тега. Номера сквозные для всех тегов
   * и не сбрасываются: ответ на запрос до сворачивания тега не примется
   * за ответ на запрос после повторного раскрытия.
   */
  #notesGeneration: Record<string, number> = {};
  #notesRequests = 0;

  constructor(panel: TagsPanel) {
    this.#panel = panel;
  }

  get order(): TagOrder {
    return this.#panel.tagOrder;
  }

  get tags(): TagSummary[] {
    return sortTags(this.#tags, this.order);
  }

  setOrder(order: TagOrder): void {
    this.#panel.setTagOrder(order);
  }

  isExpanded(key: string): boolean {
    return this.#expanded.includes(key);
  }

  notesOf(key: string): TagNotesState | undefined {
    return this.#notes[key];
  }

  /** Щелчок по тегу: раскрыть (и запросить заметки) или свернуть. */
  toggle(key: string): void {
    if (this.isExpanded(key)) {
      this.#expanded = this.#expanded.filter((k) => k !== key);
      this.#forgetNotes((k) => k !== key);
    } else {
      this.#expanded = [...this.#expanded, key];
      void this.#loadNotes(key);
    }
  }

  /**
   * Открыть вид «Теги» с раскрытым тегом — щелчок по тегу в редакторе
   * (0.2-3.1). `tag` — как написан в тексте (с `#` или без); раскрытый тег
   * не сворачивается. Заметки запрашиваются, когда вид подключится.
   */
  reveal(tag: string): void {
    const key = tagKey(tag);
    if (!this.isExpanded(key)) this.#expanded = [...this.#expanded, key];
    this.revealed = key;
    this.#panel.reveal('tags');
    if (this.#connected) void this.#loadNotes(key);
  }

  #connected = false;

  /**
   * Вид на экране: подписка на `index:changed` и `index:status`, первый
   * запрос. Возвращает отключение — вызвать, когда вид убран.
   */
  connect(): () => void {
    this.#connected = true;
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
    subscribe(
      onCoreEvent('index:changed', () => {
        if (!disposed) void this.refresh();
      }),
    );
    subscribe(
      onCoreEvent('index:status', (status) => {
        if (disposed) return;
        if (status.state === 'building') this.#setStatus({ kind: 'building', percent: percent(status.done, status.total) });
        else if (this.status.kind !== 'ready') void this.refresh();
      }),
    );
    void this.refresh();
    return () => {
      if (disposed) return;
      disposed = true;
      this.#connected = false;
      this.#generation++;
      for (const stop of stops.splice(0)) stop();
    };
  }

  /** Перезапросить список тегов и заметки раскрытых тегов. */
  async refresh(): Promise<void> {
    const generation = ++this.#generation;
    let reply;
    try {
      reply = await core.tagsList();
    } catch (error) {
      if (generation === this.#generation) this.#setStatus({ kind: 'error', message: errorKey(error) });
      return;
    }
    if (generation !== this.#generation) return;
    if (reply.status === 'index_building') {
      this.#setStatus({ kind: 'building', percent: percent(reply.done, reply.total) });
      return;
    }
    this.#tags = reply.tags;
    this.#setStatus({ kind: 'ready' });
    // Тег, у которого не осталось заметок, исчез из ответа — и из раскрытых
    // (в том числе раскрытый из редактора, но ещё не попавший в индекс).
    const known = (key: string) => reply.tags.some((summary) => summary.key === key);
    const kept = this.#expanded.filter(known);
    this.#forgetNotes((key) => kept.includes(key));
    this.#expanded = kept;
    await Promise.all(kept.map((key) => this.#loadNotes(key)));
  }

  /** Вид прокрутил к тегу из `reveal`. */
  revealDone(): void {
    this.revealed = null;
  }

  #setStatus(status: TagsStatus): void {
    this.status = status;
    this.#panel.setCount('tags', status.kind === 'ready' ? this.#tags.length : null);
  }

  /** Оставить заметки только тегов, для которых `keep` — `true`. */
  #forgetNotes(keep: (key: string) => boolean): void {
    this.#notes = Object.fromEntries(Object.entries(this.#notes).filter(([key]) => keep(key)));
    this.#notesGeneration = Object.fromEntries(Object.entries(this.#notesGeneration).filter(([key]) => keep(key)));
  }

  async #loadNotes(key: string): Promise<void> {
    const generation = ++this.#notesRequests;
    this.#notesGeneration = { ...this.#notesGeneration, [key]: generation };
    if (!this.#notes[key]) this.#notes = { ...this.#notes, [key]: { kind: 'loading' } };
    const current = () => this.#notesGeneration[key] === generation && this.isExpanded(key);
    try {
      const reply = await core.tagNotes({ key });
      if (!current()) return;
      if (reply.status === 'index_building') {
        this.#setStatus({ kind: 'building', percent: percent(reply.done, reply.total) });
        return;
      }
      this.#notes = { ...this.#notes, [key]: { kind: 'ready', notes: noteRows(reply.notes), total: reply.notes_total } };
    } catch (error) {
      if (current()) this.#notes = { ...this.#notes, [key]: { kind: 'error', message: errorKey(error) } };
    }
  }
}
