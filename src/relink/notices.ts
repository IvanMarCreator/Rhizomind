// Уведомления об итоге обновления ссылок (0.2-5.1; С2-3.5 «Где ссылки не
// могут быть обновлены», С2-5.4; решения 115, 121; макеты У-1, У-3).
// Правило контракта: уведомление — только по событию `links:updated`, поле
// `relink` в ответе `entry_rename`/`entry_move` не показывается.
// - «Ссылки обновлены в N заметках» — с крестиком;
// - «Не удалось обновить ссылки в N заметках» — «Показать» (список заметок с
//   причиной, щелчок открывает заметку) и «Повторить» (только `busy`/`io`,
//   по `retry_token`); висит до закрытия (решение 121);
// - ход переписывания (`links:progress`) — одно уведомление без крестика.
// Состояние — хранилище Svelte (`subscribe`): зона уведомлений окна
// (src/shell/Notifications.svelte) показывает его рядом с обычными.
import type {
  LinksProgressPayload,
  LinksUpdateReason,
  LinksUpdatedPayload,
  RelinkFailure,
  RelinkFailureReason,
  RelinkToken,
  VaultPath,
} from '../api/types.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';
import { noteName } from '../editor/sessions.ts';

export interface RelinkNoticesHost {
  /** «Повторить»: `relink_answer` с `retry_token` и `apply: true`; итог — новым `links:updated`. */
  retry(token: RelinkToken): Promise<unknown>;
  /** Обычное уведомление окна (ошибка повтора). */
  notify(text: string): void;
  /** Предел размера для редактирования (настройка хранилища) — причина `too_large`. */
  readonly limitBytes: number;
}

export type RelinkNoticeTone = 'success' | 'warning' | 'progress';

export interface RelinkNotice {
  id: number;
  tone: RelinkNoticeTone;
  text: string;
  /** Вторая строка, приглушённая. */
  detail: string | null;
  closable: boolean;
  /** Кнопка «Показать»: список заметок, где ссылки не обновлены. */
  canShow: boolean;
  /** Кнопка «Повторить» (решение 115). */
  canRetry: boolean;
  reason: LinksUpdateReason | null;
  failed: readonly RelinkFailure[];
}

export interface RelinkNoticesState {
  notices: readonly RelinkNotice[];
  /** Уведомление, чей список «Не удалось обновить ссылки» открыт. */
  shown: RelinkNotice | null;
}

/** Строка списка «Не удалось обновить ссылки» (макет У-3). */
export interface FailureRow {
  path: VaultPath;
  name: string;
  /** Папка заметки через « / » или «корень хранилища». */
  folder: string;
  label: string;
  /** «будет повторено» — `busy`/`io`; «не повторяется» — `too_large`/`not_utf8`. */
  retried: boolean;
}

/** Повтор помогает только занятым и недоступным заметкам (решение 115). */
export function isRetriable(reason: RelinkFailureReason): boolean {
  return reason === 'busy' || reason === 'io';
}

/** «1 заметке», «5 заметках», «21 заметке» — предложный падеж после «в». */
export function notesIn(count: number): string {
  const one = count % 10 === 1 && count % 100 !== 11;
  return t(one ? 'relink.notesIn.one' : 'relink.notesIn.many', { count });
}

const MIB = 1024 * 1024;
const number = new Intl.NumberFormat('ru', { maximumFractionDigits: 1 });

function failureLabel(reason: RelinkFailureReason, limitBytes: number): string {
  switch (reason) {
    case 'too_large':
      return t('relink.failure.too_large', { size: t('units.megabytes', { value: number.format(limitBytes / MIB) }) });
    case 'not_utf8':
      return t('relink.failure.not_utf8');
    case 'busy':
      return t('relink.failure.busy');
    case 'io':
      return t('relink.failure.io');
  }
}

export function failureRows(failed: readonly RelinkFailure[], limitBytes: number): FailureRow[] {
  return failed.map(({ path, reason }) => ({
    path,
    name: noteName(path),
    folder: path.length > 1 ? path.slice(0, -1).join(' / ') : t('relink.failed.vaultRoot'),
    label: failureLabel(reason, limitBytes),
    retried: isRetriable(reason),
  }));
}

function successText(reason: LinksUpdateReason, updated: number): string {
  if (reason === 'resumed') return t('relink.resumed');
  if (reason === 'duplicate') return t('relink.clarified', { notes: notesIn(updated) });
  return t('relink.updated', { notes: notesIn(updated) });
}

type Subscriber = (state: RelinkNoticesState) => void;

export class RelinkNotices {
  readonly #host: RelinkNoticesHost;
  #state: RelinkNoticesState = { notices: [], shown: null };
  /** Знаки повтора — только здесь, не в состоянии для показа; одноразовые. */
  readonly #retryTokens = new Map<number, RelinkToken>();
  readonly #subscribers = new Set<Subscriber>();
  #nextId = 1;

  constructor(host: RelinkNoticesHost) {
    this.#host = host;
  }

  get state(): RelinkNoticesState {
    return this.#state;
  }

  get limitBytes(): number {
    return this.#host.limitBytes;
  }

  /** Контракт хранилища Svelte: сразу текущее состояние, потом каждое изменение. */
  subscribe(run: Subscriber): () => void {
    this.#subscribers.add(run);
    run(this.#state);
    return () => this.#subscribers.delete(run);
  }

  /** Событие `links:updated`. */
  updated({ reason, updated, failed, retry_token }: LinksUpdatedPayload): void {
    const added: RelinkNotice[] = [];
    if (updated > 0) {
      added.push(this.#notice({ tone: 'success', text: successText(reason, updated), reason }));
    }
    if (failed.length > 0) {
      const canRetry = retry_token !== null && failed.some((failure) => isRetriable(failure.reason));
      const done = reason === 'duplicate' ? null : t('relink.failed.renameDone');
      const detail = canRetry ? done : [done, t('relink.failed.noRetry')].filter(Boolean).join(' ');
      const notice = this.#notice({
        tone: 'warning',
        text: t('relink.failed', { notes: notesIn(failed.length) }),
        detail,
        canShow: true,
        canRetry,
        reason,
        failed,
      });
      if (canRetry && retry_token !== null) this.#retryTokens.set(notice.id, retry_token);
      added.push(notice);
    }
    // Итог пришёл — ход переписывания больше не нужен.
    this.#set({ ...this.#state, notices: [...this.#withoutProgress(), ...added] });
  }

  /** Событие `links:progress`: одно уведомление о ходе, обновляется на месте. */
  progress({ done, total }: LinksProgressPayload): void {
    const current = this.#state.notices.find((notice) => notice.tone === 'progress');
    if (total > 0 && done >= total) {
      if (current) this.#set({ ...this.#state, notices: this.#withoutProgress() });
      return;
    }
    const text = total > 0 ? t('relink.progress', { done, total }) : t('relink.progressUnknown');
    if (current) {
      const next = { ...current, text };
      this.#set({ ...this.#state, notices: this.#state.notices.map((notice) => (notice === current ? next : notice)) });
      return;
    }
    const notice = this.#notice({ tone: 'progress', text, closable: false });
    this.#set({ ...this.#state, notices: [...this.#state.notices, notice] });
  }

  dismiss(id: number): void {
    this.#retryTokens.delete(id);
    this.#set({
      notices: this.#state.notices.filter((notice) => notice.id !== id),
      shown: this.#state.shown?.id === id ? null : this.#state.shown,
    });
  }

  /** «Показать»: открыть список заметок, где ссылки не обновлены. */
  show(id: number): void {
    const notice = this.#state.notices.find((item) => item.id === id && item.canShow);
    if (notice) this.#set({ ...this.#state, shown: notice });
  }

  hide(): void {
    if (this.#state.shown) this.#set({ ...this.#state, shown: null });
  }

  /**
   * «Повторить» (решение 115): знак одноразовый — уведомление уходит сразу,
   * итог повтора придёт новым `links:updated` (`reason: retry`).
   */
  async retry(id: number): Promise<void> {
    const token = this.#retryTokens.get(id);
    if (token === undefined) return;
    this.dismiss(id);
    try {
      await this.#host.retry(token);
    } catch (error) {
      this.#host.notify(t('relink.retryFailed', { message: userMessage(error) }));
    }
  }

  #withoutProgress(): RelinkNotice[] {
    return this.#state.notices.filter((notice) => notice.tone !== 'progress');
  }

  #notice(fields: Partial<RelinkNotice> & Pick<RelinkNotice, 'tone' | 'text'>): RelinkNotice {
    return {
      id: this.#nextId++,
      detail: null,
      closable: true,
      canShow: false,
      canRetry: false,
      reason: null,
      failed: [],
      ...fields,
    };
  }

  #set(state: RelinkNoticesState): void {
    this.#state = state;
    for (const run of this.#subscribers) run(state);
  }
}
