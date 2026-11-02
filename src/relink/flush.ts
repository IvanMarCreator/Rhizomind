// Запись ввода перед переписыванием ссылок (0.2-5.1, С2-3.5 «Открытые
// заметки», С2-5.4 шаг 2, Р7). Ядро присылает `links:flush-requested` и ждёт
// `links_flush_done` не дольше 10 с: окно записывает несохранённый ввод всех
// своих заметок тем же путём, что при закрытии окна (`flushForLinks` —
// заметки не закрываются), и отвечает. Не записалось — `saved: false`:
// ядро не переписывает ссылки, ввод остаётся в редакторе (инвариант 4).
import { core } from '../api/index.ts';
import type { LinksFlushRequestedPayload, RelinkToken } from '../api/types.ts';
import { noteName, type UnsavedNote } from '../editor/sessions.ts';
import { t } from '../i18n/index.ts';

export interface FlushHost {
  /** Записать ввод всех заметок окна; вернуть заметки, ввод которых записать не удалось. */
  flush(): Promise<readonly UnsavedNote[]>;
  /** Ответ ядру `links_flush_done`. */
  done(token: RelinkToken, saved: boolean): Promise<unknown>;
  notify(text: string): void;
}

/** Ответ ядру — команда API v1. */
export const coreFlushDone = (token: RelinkToken, saved: boolean): Promise<null> =>
  core.linksFlushDone({ token, saved });

export class LinksFlush {
  readonly #host: FlushHost;

  constructor(host: FlushHost) {
    this.#host = host;
  }

  /** Сигнал ядра `links:flush-requested`: записать ввод и ответить со своим знаком. */
  async requested({ token }: LinksFlushRequestedPayload): Promise<void> {
    // `null` — сама запись бросила исключение: какие заметки, неизвестно.
    let unsaved: readonly UnsavedNote[] | null;
    try {
      unsaved = await this.#host.flush();
    } catch {
      unsaved = null;
    }
    const saved = unsaved?.length === 0;
    if (!saved) this.#host.notify(refusalText(unsaved));
    await this.#host.done(token, saved).catch(() => undefined);
  }
}

function refusalText(unsaved: readonly UnsavedNote[] | null): string {
  if (unsaved === null || unsaved.length === 0) return t('relink.flushFailed.unknown');
  const names = unsaved.map((note) => `«${noteName(note.path)}»`).join(', ');
  return t('relink.flushFailed', { names });
}
