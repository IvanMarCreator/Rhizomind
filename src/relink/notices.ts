// Уведомления об итоге обновления ссылок (0.2-5.1; С2-3.5 «Где ссылки не
// могут быть обновлены», С2-5.4; решения 115, 121; макеты У-1, У-3).
// Правило контракта: уведомление — только по событию `links:updated`, поле
// `relink` в ответе `entry_rename`/`entry_move` не показывается.
// - «Ссылки обновлены в N заметках» — с крестиком;
// - «Не удалось обновить ссылки в N заметках» — «Показать» (список заметок с
//   причиной, щелчок открывает заметку) и «Повторить» (только `busy`/`io`,
//   по `retry_token`); висит до закрытия (решение 121);
// - ход переписывания (`links:progress`) — одно уведомление без крестика.
// Вопросы «нужен ответ» (0.2-6.3, `links:pending`; С2-3.5 «в Проводнике»,
// С2-3.6 «Массовое копирование»; решения 110, 111; макеты У-1, У-2):
// - без крестика, висят до ответа; ответ — `relink_answer` со знаком события;
// - одно уведомление на пачку: новый `links:pending` той же причины заменяет
//   прежний (ядро гасит старый знак и шлёт вопрос на всю цепочку, 0.2-6.1);
// - знак не принят (`invalid_argument`) — уведомление не закрывается молча:
//   текст ошибки и крестик; ядро пришлёт свежий вопрос или итог.
// Состояние — хранилище Svelte (`subscribe`): зона уведомлений окна
// (src/shell/Notifications.svelte) показывает его рядом с обычными.
import { CoreCallError } from '../api/core.ts';
import type {
  LinksPendingPayload,
  LinksProgressPayload,
  LinksUpdateReason,
  LinksUpdatedPayload,
  PathRename,
  PendingReason,
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
  /** Ответ на вопрос `links:pending`: `relink_answer` со знаком вопроса. */
  answer(token: RelinkToken, apply: boolean): Promise<unknown>;
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
  /** Переименования «старое → новое» из `links:updated` (заголовок списка сбоев). */
  renames: readonly PathRename[];
  renamesTotal: number;
}

/** Уведомление «нужен ответ» (`links:pending`). Знак — не здесь, а в модели. */
export interface PendingQuestion {
  id: number;
  reason: PendingReason;
  text: string;
  /** Вторая строка, приглушённая. */
  detail: string | null;
  /** Подписи кнопок: «Обновить ссылки» / «Не обновлять», «Уточнить» / «Не трогать». */
  apply: string;
  skip: string;
  /** Ответ отправлен, ждём ядро: кнопки недоступны. */
  busy: boolean;
  /** Ответ не принят — текст ошибки. */
  error: string | null;
  /** Знак больше не действует: кнопок ответа нет. */
  stale: boolean;
  /** Крестик — только у вопроса, на который ответить уже нельзя. */
  closable: boolean;
}

export interface RelinkNoticesState {
  notices: readonly RelinkNotice[];
  /** Вопросы «нужен ответ», не больше одного на причину. */
  questions: readonly PendingQuestion[];
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
  /** «будет повторено» — `busy`/`io`; «не повторяется» — `too_large`/`not_utf8`/`conflict`. */
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
    case 'conflict':
      return t('relink.failure.conflict');
  }
}

/**
 * Почему повтор не поможет: все неповторяемые заметки — `conflict` (Р20) —
 * «проверьте ссылки вручную»; иначе — заметки больше лимита или не в UTF-8.
 */
export function onlyConflicts(failed: readonly RelinkFailure[]): boolean {
  const stuck = failed.filter((failure) => !isRetriable(failure.reason));
  return stuck.length > 0 && stuck.every((failure) => failure.reason === 'conflict');
}

/** «„Встреча“ → „Планёрка“» (и «и ещё N»), или `null`, если имён нет. */
export function renamesText(renames: readonly PathRename[], total: number): string | null {
  const first = renames[0];
  if (first === undefined) return null;
  const names = { from: noteName(first.from), to: noteName(first.to) };
  const more = Math.max(total, renames.length) - 1;
  return more > 0 ? t('relink.failed.renamedMore', { ...names, count: more }) : t('relink.failed.renamed', names);
}

export function failureRows(failed: readonly RelinkFailure[], limitBytes: number): FailureRow[] {
  return failed.map(({ path, reason }) => ({
    path,
    name: noteName(path),
    folder: folderOf(path),
    label: failureLabel(reason, limitBytes),
    retried: isRetriable(reason),
  }));
}

type Plural = 'one' | 'few' | 'many';

/** Форма слова после числа: 1 заметка, 3 заметки, 12 заметок, 21 заметка. */
export function pluralForm(count: number): Plural {
  const ten = count % 10;
  const hundred = count % 100;
  if (ten === 1 && hundred !== 11) return 'one';
  if (ten >= 2 && ten <= 4 && (hundred < 12 || hundred > 14)) return 'few';
  return 'many';
}

/** «3 заметки». */
function notesCount(count: number): string {
  return t(`relink.count.${pluralForm(count)}`, { count });
}

/** «ссылаются 12 заметок». */
function referrers(count: number): string {
  return t(`relink.refs.${pluralForm(count)}`, { count });
}

/** Папка пути через « / » или «корень хранилища». */
function folderOf(path: VaultPath): string {
  return path.length > 1 ? path.slice(0, -1).join(' / ') : t('relink.failed.vaultRoot');
}

/** Одно переименование из пачки, если оно единственное. */
function single(renames: readonly PathRename[], total: number): PathRename | null {
  return Math.max(total, renames.length) === 1 ? (renames[0] ?? null) : null;
}

function pendingFields({ reason, notes, renames, renames_total }: LinksPendingPayload): Pick<PendingQuestion, 'text' | 'detail' | 'apply' | 'skip'> {
  if (reason === 'duplicate') {
    return {
      text: t('relink.pending.duplicate', { notes: notesIn(notes) }),
      detail: t('relink.pending.duplicateDetail'),
      apply: t('relink.pending.clarify'),
      skip: t('relink.pending.skip'),
    };
  }
  const buttons = { apply: t('relink.pending.update'), skip: t('relink.pending.keep') };
  const one = single(renames, renames_total);
  if (one) {
    const from = noteName(one.from);
    const to = noteName(one.to);
    const refs = referrers(notes);
    const text =
      from === to
        ? t('relink.pending.moved', { name: from, folder: folderOf(one.to), refs })
        : t('relink.pending.one', { from, to, refs });
    return { text, detail: null, ...buttons };
  }
  return {
    text: t('relink.pending.batch', { renamed: notesCount(Math.max(renames_total, renames.length)), refs: referrers(notes) }),
    detail: t('relink.pending.batchDetail'),
    ...buttons,
  };
}

function successText({ reason, updated, renames, renames_total }: LinksUpdatedPayload, undo: boolean): string {
  const notes = notesIn(updated);
  if (reason === 'resumed') return t('relink.resumed');
  if (reason === 'duplicate') return t('relink.clarified', { notes });
  if (undo) return t('relink.undone', { notes });
  if (reason === 'external_rename') {
    const one = single(renames, renames_total);
    if (one) return t('relink.externalUpdated.one', { name: noteName(one.from), notes });
    if (renames.length > 0) return t('relink.externalUpdated.many', { renamed: notesCount(Math.max(renames_total, renames.length)), notes });
  }
  return t('relink.updated', { notes });
}

const samePath = (a: VaultPath, b: VaultPath) => a.length === b.length && a.every((part, i) => part === b[i]);
const sameRename = (a: PathRename, b: PathRename) => samePath(a.from, b.from) && samePath(a.to, b.to);
const reversed = ({ from, to }: PathRename): PathRename => ({ from: to, to: from });

/** Знак не принят ядром: неизвестный, просроченный или погашенный новым вопросом. */
function isRejectedToken(error: unknown): boolean {
  return error instanceof CoreCallError && error.error.code === 'invalid_argument';
}

type Subscriber = (state: RelinkNoticesState) => void;

export class RelinkNotices {
  readonly #host: RelinkNoticesHost;
  #state: RelinkNoticesState = { notices: [], questions: [], shown: null };
  /** Знаки повтора — только здесь, не в состоянии для показа; одноразовые. */
  readonly #retryTokens = new Map<number, RelinkToken>();
  /** Знаки вопросов `links:pending` — тоже только здесь. */
  readonly #questionTokens = new Map<number, RelinkToken>();
  /** Ctrl+Z: переименование, итог которого — «Переименование отменено…» (ревью 0.2-5.2, М4). */
  #undo: PathRename | null = null;
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
  updated(payload: LinksUpdatedPayload): void {
    const { reason, updated, failed, retry_token, renames, renames_total } = payload;
    const first = renames[0];
    const undo = reason === 'rename' && this.#undo !== null && first !== undefined && sameRename(first, this.#undo);
    if (reason === 'rename') this.#undo = null;
    let kept = this.#withoutProgress();
    if (undo && first) {
      // Отмена заменяет итог исходного переименования: одно уведомление, а не два одинаковых.
      const original = reversed(first);
      kept = kept.filter((notice) => !(notice.tone === 'success' && notice.renames[0] && sameRename(notice.renames[0], original)));
    }
    const added: RelinkNotice[] = [];
    if (updated > 0) {
      added.push(this.#notice({ tone: 'success', text: successText(payload, undo), reason, renames, renamesTotal: renames_total }));
    }
    if (failed.length > 0) {
      const canRetry = retry_token !== null && failed.some((failure) => isRetriable(failure.reason));
      const done = reason === 'duplicate' ? null : t('relink.failed.renameDone');
      const noRetry = onlyConflicts(failed) ? t('relink.failed.noRetryConflict') : t('relink.failed.noRetry');
      const detail = canRetry ? done : [done, noRetry].filter(Boolean).join(' ');
      const notice = this.#notice({
        tone: 'warning',
        text: t('relink.failed', { notes: notesIn(failed.length) }),
        detail,
        canShow: true,
        canRetry,
        reason,
        failed,
        renames,
        renamesTotal: renames_total,
      });
      if (canRetry && retry_token !== null) this.#retryTokens.set(notice.id, retry_token);
      added.push(notice);
    }
    // Итог пришёл — ход переписывания больше не нужен; устаревший вопрос той же
    // причины тоже: ядро ответило итогом вместо нового вопроса.
    const questions = this.#state.questions.filter((question) => !(question.stale && question.reason === reason));
    this.#set({ ...this.#state, notices: [...kept, ...added], questions });
  }

  /**
   * Ctrl+Z переименования с обновлением ссылок: итог этого переименования
   * (`links:updated`, `reason: rename`, первое «старое → новое» совпадает)
   * подписывается «Переименование отменено, ссылки возвращены…».
   */
  undoing(rename: PathRename): void {
    this.#undo = rename;
  }

  /** Событие `links:pending`: вопрос; прежний вопрос той же причины заменяется на месте. */
  pending(payload: LinksPendingPayload): void {
    const question: PendingQuestion = {
      id: this.#nextId++,
      reason: payload.reason,
      ...pendingFields(payload),
      busy: false,
      error: null,
      stale: false,
      closable: false,
    };
    this.#questionTokens.set(question.id, payload.token);
    const previous = this.#state.questions.find((item) => item.reason === payload.reason);
    if (previous) this.#questionTokens.delete(previous.id);
    const questions = previous
      ? this.#state.questions.map((item) => (item === previous ? question : item))
      : [...this.#state.questions, question];
    this.#set({ ...this.#state, questions });
  }

  /**
   * Ответ на вопрос: `relink_answer` со знаком вопроса. Принят — уведомление
   * уходит (итог придёт `links:updated`); знак не принят — ошибка и крестик;
   * другая ошибка — ошибка, кнопки остаются.
   */
  async answer(id: number, apply: boolean): Promise<void> {
    const token = this.#questionTokens.get(id);
    const question = this.#state.questions.find((item) => item.id === id);
    if (token === undefined || !question || question.busy || question.stale) return;
    this.#updateQuestion(id, { busy: true, error: null });
    try {
      await this.#host.answer(token, apply);
    } catch (error) {
      // Вопрос мог смениться, пока шёл ответ: тогда отвечать уже на новый.
      if (!this.#questionTokens.has(id)) return;
      if (isRejectedToken(error)) {
        this.#questionTokens.delete(id);
        this.#updateQuestion(id, { busy: false, stale: true, closable: true, error: t('relink.pending.stale') });
      } else {
        this.#updateQuestion(id, { busy: false, error: t('relink.pending.failed', { message: userMessage(error) }) });
      }
      return;
    }
    this.#questionTokens.delete(id);
    this.#set({ ...this.#state, questions: this.#state.questions.filter((item) => item.id !== id) });
  }

  /** Крестик — только у вопроса, на который ответить уже нельзя. */
  dismissQuestion(id: number): void {
    const question = this.#state.questions.find((item) => item.id === id);
    if (!question?.closable) return;
    this.#set({ ...this.#state, questions: this.#state.questions.filter((item) => item !== question) });
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
      ...this.#state,
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

  #updateQuestion(id: number, patch: Partial<PendingQuestion>): void {
    this.#set({ ...this.#state, questions: this.#state.questions.map((item) => (item.id === id ? { ...item, ...patch } : item)) });
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
      renames: [],
      renamesTotal: 0,
      ...fields,
    };
  }

  #set(state: RelinkNoticesState): void {
    this.#state = state;
    for (const run of this.#subscribers) run(state);
  }
}
