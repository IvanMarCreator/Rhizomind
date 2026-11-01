// Подмена ядра для тестов интерфейса (Vitest, компонентные тесты): без окна
// Tauri, на @tauri-apps/api/mocks. В приложение не импортируется.
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type {
  Backlinks,
  CommandName,
  CommandRequest,
  CommandResponse,
  CoreError,
  CoreEvents,
  EventName,
  IndexReply,
  IndexStatus,
  RelinkPreview,
  RelinkToken,
  TagSummary,
  VaultPath,
} from './types.ts';

/** Подменный обработчик команды: получает запрос, возвращает ответ или бросает CoreError. */
export type CommandMock<K extends CommandName> = (
  request: CommandRequest<K>,
) => CommandResponse<K> | Promise<CommandResponse<K>>;

export type CoreMocks = { [K in CommandName]?: CommandMock<K> };

/** Вызов, дошедший до подменного ядра: имя команды и аргументы `invoke`. */
export type RecordedCall = [command: string, args: unknown];

const notImplemented: CoreError = { code: 'not_implemented', message_key: 'errors.not_implemented' };

type Invoke = (command: string, args?: unknown, options?: unknown) => Promise<unknown>;

declare global {
  interface Window {
    /** Мост Tauri; в тестах его ставит @tauri-apps/api/mocks. */
    __TAURI_INTERNALS__: { invoke: Invoke };
  }
}

/**
 * Подменяет ядро: команды из `mocks` отвечают подменой, остальные — ошибкой
 * `not_implemented`, как заглушки ядра. Окно — `main`; события подменяются
 * тоже (`emitCoreEvent`). Возвращает журнал всех вызовов моста, включая
 * подписку на события. После теста — `clearMocks()`.
 */
export function mockCore(mocks: CoreMocks): RecordedCall[] {
  const calls: RecordedCall[] = [];
  mockWindows('main');
  mockIPC(
    (command, args) => {
      const mock = (mocks as Record<string, ((request: unknown) => unknown) | undefined>)[command];
      if (!mock) throw notImplemented;
      const request = (args as { request?: unknown } | undefined)?.request ?? null;
      return mock(request);
    },
    { shouldMockEvents: true },
  );
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  internals.invoke = (command, args, options) => {
    calls.push([command, args ?? {}]);
    return invoke(command, args, options);
  };
  return calls;
}

/** Отправляет событие «от ядра» подписчикам интерфейса (после `mockCore`). */
export function emitCoreEvent<K extends EventName>(event: K, payload: CoreEvents[K]): Promise<void> {
  return emit(event, payload);
}

// ---- Подмена индекса и обновления ссылок (0.2) ------------------------------
// Интерфейс 0.2 (правая панель, обратные ссылки, теги, окно вопроса) строится
// на подмене, пока ядро не готово (план 0.2, этап 2). Подмена отвечает так же,
// как ядро: пока индекс строится — `index_building`, неизвестная заметка или
// тег — пустой список, не ошибка.

/** Знак плана для тестов (вид — как у ядра: 32 шестнадцатеричные цифры). */
export const TEST_RELINK_TOKEN: RelinkToken = 'f'.repeat(32);

/** Данные подменного индекса. */
export interface IndexFake {
  /** По умолчанию — готов (`ready`); `building` — все команды индекса отвечают `index_building`. */
  status?: IndexStatus;
  /** Обратные ссылки по пути заметки (компоненты через `/`). */
  backlinks?: Record<string, Backlinks>;
  tags?: TagSummary[];
  /** Заметки по ключу тега. */
  tagNotes?: Record<string, VaultPath[]>;
}

/** Подмена команд индекса (`index_status`, `backlinks_list`, `tags_list`, `tag_notes`) для `mockCore`. */
export function indexMocks(fake: IndexFake = {}): CoreMocks {
  const status: IndexStatus = fake.status ?? { state: 'ready', done: 0, total: 0 };
  const reply = <T extends object>(data: T): IndexReply<T> =>
    status.state === 'ready'
      ? { status: 'ready', ...data }
      : { status: 'index_building', done: status.done, total: status.total };
  return {
    index_status: () => status,
    backlinks_list: ({ path }) => reply(fake.backlinks?.[path.join('/')] ?? { notes_total: 0, truncated: false, notes: [] }),
    tags_list: () => reply({ tags: fake.tags ?? [] }),
    tag_notes: ({ key }) => {
      const notes = fake.tagNotes?.[key] ?? [];
      return reply({ notes_total: notes.length, notes });
    },
  };
}

/**
 * Подмена `relink_preview`, `relink_answer` и `links_flush_done` для `mockCore`.
 * `preview` — ответ предпросмотра (по умолчанию: переписывать нечего);
 * ответы окна видны в журнале вызовов `mockCore`.
 */
export function relinkMocks(
  preview: IndexReply<RelinkPreview> = { status: 'ready', notes: 0, links: 0, token: TEST_RELINK_TOKEN },
): CoreMocks {
  return {
    relink_preview: () => preview,
    relink_answer: () => null,
    links_flush_done: () => null,
  };
}
