// Вид «Обратные ссылки» (0.2-2.2, С2-3.2): что показывает вид для открытой
// заметки. Данные — команда ядра `backlinks_list` (ответ `IndexReply`: готово
// или «индекс строится»); перезапрос — по `index:changed` и по `index:status`
// с `ready`, пока вид ждёт индекс. Ответ на прежний запрос (заметку сменили,
// пока ядро отвечало) не показывается.
import { core, type BacklinkLine, type BacklinkNote, type Backlinks, type IndexReply, type IndexStatus, type VaultPath } from '../api/index.ts';
import { nameKey } from '../core/names/index.ts';
import type { LinkPosition } from '../editor/wikilinks/host.ts';
import { userMessage } from '../i18n/errors.ts';

export type { LinkPosition };

export type BacklinksState =
  /** Заметка не открыта. */
  | { kind: 'none' }
  /** Первый ответ ещё не пришёл. */
  | { kind: 'loading' }
  /** Индекс строится; `percent` — `null`, пока объём неизвестен. */
  | { kind: 'building'; percent: number | null }
  | { kind: 'ready'; data: Backlinks }
  /** Ошибка ядра: текст для пользователя. */
  | { kind: 'failed'; message: string };

/** Источник обратных ссылок; по умолчанию — команда ядра. */
export type BacklinksSource = (path: VaultPath) => Promise<IndexReply<Backlinks>>;

const coreSource: BacklinksSource = (path) => core.backlinksList({ path });

const samePath = (a: VaultPath | null, b: VaultPath | null) =>
  a === b || (a !== null && b !== null && a.length === b.length && a.every((part, i) => part === b[i]));

/** Доля сборки индекса в процентах, вниз до целого; объём неизвестен — `null`. */
export function buildPercent(done: number, total: number): number | null {
  if (total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((done * 100) / total)));
}

export class BacklinksView {
  state = $state.raw<BacklinksState>({ kind: 'none' });

  readonly #source: BacklinksSource;
  #path: VaultPath | null = null;
  /** `show` уже вызывали. */
  #started = false;
  /** Номер последнего запроса: ответы на прежние не показываются. */
  #seq = 0;

  constructor(source: BacklinksSource = coreSource) {
    this.#source = source;
  }

  /** Число в заголовке вида: всего ссылающихся заметок; `null` — ещё неизвестно. */
  get count(): number | null {
    switch (this.state.kind) {
      case 'none':
        return 0;
      case 'ready':
        return this.state.data.notes_total;
      default:
        return null;
    }
  }

  /** Вид показывает заметку `path` (или ничего). */
  show(path: VaultPath | null): void {
    // Не читает реактивное состояние: вызывается из эффекта компонента.
    if (this.#started && samePath(path, this.#path)) return;
    this.#started = true;
    this.#path = path;
    if (!path) {
      this.#seq++;
      this.state = { kind: 'none' };
      return;
    }
    this.state = { kind: 'loading' };
    void this.#fetch();
  }

  /** `index:changed`: обратные ссылки могли измениться — перезапрос (на экране — прежний список). */
  refresh(): void {
    if (this.#path) void this.#fetch();
  }

  /**
   * `index:status`: ход сборки, пока вид ждёт индекс; готов — перезапрос. Ждёт
   * и первый ответ (`loading`): он мог уйти до готовности и вернуться с
   * «строится» — тогда ответ на перезапрос его заменит.
   */
  indexStatus(status: IndexStatus): void {
    const kind = this.state.kind;
    if (kind !== 'building' && kind !== 'loading') return;
    if (status.state === 'ready') this.refresh();
    else if (kind === 'building') this.state = { kind: 'building', percent: buildPercent(status.done, status.total) };
  }

  async #fetch(): Promise<void> {
    const path = this.#path;
    if (!path) return;
    const seq = ++this.#seq;
    try {
      const reply = await this.#source(path);
      if (seq !== this.#seq) return;
      if (reply.status === 'index_building') {
        this.state = { kind: 'building', percent: buildPercent(reply.done, reply.total) };
      } else {
        const { notes_total, truncated, notes } = reply;
        this.state = { kind: 'ready', data: { notes_total, truncated, notes } };
      }
    } catch (error) {
      if (seq === this.#seq) this.state = { kind: 'failed', message: userMessage(error) };
    }
  }
}

/** Папка заметки для показа: компоненты через « / »; заметка в корне — пустая строка. */
export function folderOf(path: VaultPath): string {
  return path.slice(0, -1).join(' / ');
}

/**
 * Ключи имён, которые повторяются в списке (без учёта регистра, как имена у
 * ядра): у таких заметок рядом с именем — папка.
 */
export function repeatedNames(notes: readonly BacklinkNote[]): (key: string) => boolean {
  const counts: Record<string, number> = Object.create(null) as Record<string, number>;
  for (const note of notes) {
    const key = nameKey(note.name);
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return (key) => (counts[key] ?? 0) > 1;
}

/** Строка обратной ссылки: текст до ссылки, ссылка, после — только текст, не HTML. */
export function splitLine(line: BacklinkLine): { before: string; link: string; after: string } {
  const length = line.snippet.length;
  const from = Math.min(Math.max(line.from_utf16, 0), length);
  const to = Math.min(Math.max(line.to_utf16, from), length);
  return { before: line.snippet.slice(0, from), link: line.snippet.slice(from, to), after: line.snippet.slice(to) };
}

/** Куда ставить курсор по строке обратной ссылки. */
export function positionOf(line: BacklinkLine): LinkPosition {
  return { line: line.line, column: line.column_utf16 };
}
