// Поиск «только последний запрос» для быстрого перехода. Без задержки
// (дебаунса): первый запрос уходит сразу. Пока ядро отвечает, новые запросы
// не отправляются, а запоминается только последний; ответ на устаревший
// запрос не показывается. Так при быстром наборе в полёте не больше одного
// запроса и на экране никогда не оказывается ответ на старый текст.

export type Run<T> = (query: string) => Promise<T>;

export class LatestOnly<T> {
  readonly #run: Run<T>;
  readonly #onResult: (query: string, result: T) => void;
  readonly #onError: (query: string, error: unknown) => void;
  #busy = false;
  #pending: string | null = null;
  #disposed = false;

  constructor(
    run: Run<T>,
    onResult: (query: string, result: T) => void,
    onError: (query: string, error: unknown) => void,
  ) {
    this.#run = run;
    this.#onResult = onResult;
    this.#onError = onError;
  }

  request(query: string): void {
    if (this.#disposed) return;
    if (this.#busy) {
      this.#pending = query;
      return;
    }
    void this.#start(query);
  }

  /** Окно закрыто: ответы больше не доставляются. */
  dispose(): void {
    this.#disposed = true;
    this.#pending = null;
  }

  async #start(query: string): Promise<void> {
    this.#busy = true;
    let outcome: { ok: true; value: T } | { ok: false; error: unknown };
    try {
      outcome = { ok: true, value: await this.#run(query) };
    } catch (error) {
      outcome = { ok: false, error };
    }
    this.#busy = false;
    if (this.#disposed) return;
    const next = this.#pending;
    this.#pending = null;
    if (next !== null) {
      void this.#start(next);
      return;
    }
    if (outcome.ok) this.#onResult(query, outcome.value);
    else this.#onError(query, outcome.error);
  }
}
