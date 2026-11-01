// Замер «нажатие → кадр на экране» внутри окна (задача 3.2).
//
// Время считает сам движок окна, а не тест: задержки WebDriver в замер не
// попадают. Для каждого нажатия (кроме Ctrl/Shift/Alt/Meta) запоминается
// `event.timeStamp` события keydown — момент, когда движок получил нажатие.
// Ответ — первое изменение DOM в своей области после нажатия (список
// быстрого перехода, текст редактора); затем ждём кадр: requestAnimationFrame
// и следующую задачу после него (MessageChannel) — к этому моменту кадр с
// изменением отрисован. Задержка = момент кадра − timeStamp нажатия; в неё
// входят обработка ввода, запрос к ядру, обновление Svelte/CodeMirror и раскладка.
//
// Функции, передаваемые в browser.execute, выполняются в окне и должны быть
// самодостаточными (CSP окна запрещает eval, поэтому условия — по имени режима).

/**
 * Где ждать ответа:
 * - `options` — в окне появились пункты списка (открытие быстрого перехода);
 * - `listbox` — изменился список быстрого перехода (ответ на ввод запроса);
 * - `editor` — изменился текст редактора (ввод символа);
 * - `editor-text` — в тексте редактора появилась строка `arg` (открытие заметки).
 */
export type ProbeMode = 'options' | 'listbox' | 'editor' | 'editor-text';

export interface ProbeRecord {
  key: string;
  down: number;
  changed: number | null;
  paint: number | null;
}

/** Включает замер в окне (ставит обработчики при первом вызове) и сбрасывает записи. */
export async function armProbe(browser: WebdriverIO.Browser, mode: ProbeMode, arg = ''): Promise<void> {
  await browser.execute(
    (mode, arg) => {
      type Rec = { key: string; down: number; changed: number | null; paint: number | null };
      type Probe = { mode: string; arg: string; records: Rec[]; longTasks: number[]; eventMax: number };
      const w = window as unknown as { __rzPerfProbe?: Probe };
      if (!w.__rzPerfProbe) {
        const probe: Probe = { mode: '', arg: '', records: [], longTasks: [], eventMax: 0 };
        w.__rzPerfProbe = probe;
        const modifiers = new Set(['Control', 'Shift', 'Alt', 'Meta']);
        // Ввод без keydown (символ вне раскладки драйвер может передать сразу
        // вводом текста) — по beforeinput, чтобы нажатие не потерялось.
        let keySinceInput = false;
        window.addEventListener(
          'keydown',
          (event) => {
            if (probe.mode === '' || modifiers.has(event.key)) return;
            keySinceInput = true;
            probe.records.push({ key: event.key, down: event.timeStamp, changed: null, paint: null });
          },
          true,
        );
        window.addEventListener(
          'beforeinput',
          (event) => {
            if (probe.mode !== '' && !keySinceInput) {
              probe.records.push({ key: event.data ?? '', down: event.timeStamp, changed: null, paint: null });
            }
            keySinceInput = false;
          },
          true,
        );
        const relevant = (mutations: MutationRecord[]): boolean => {
          const inside = (selector: string) =>
            mutations.some((m) => {
              const node = m.target instanceof Element ? m.target : m.target.parentElement;
              return Boolean(node?.closest(selector));
            });
          switch (probe.mode) {
            case 'options':
              return document.querySelector('[role="option"]') !== null;
            case 'listbox':
              return inside('[role="listbox"]');
            case 'editor':
              return inside('.cm-content');
            case 'editor-text':
              return (document.querySelector('.cm-content')?.textContent ?? '').includes(probe.arg);
            default:
              return false;
          }
        };
        new MutationObserver((mutations) => {
          const waiting = probe.records.filter((r) => r.changed === null);
          if (waiting.length === 0 || !relevant(mutations)) return;
          const at = performance.now();
          for (const r of waiting) r.changed = at;
          requestAnimationFrame(() => {
            const channel = new MessageChannel();
            channel.port1.onmessage = () => {
              const painted = performance.now();
              for (const r of waiting) r.paint = painted;
            };
            channel.port2.postMessage(null);
          });
        }).observe(document.documentElement, { subtree: true, childList: true, characterData: true });
        // Справочно (только Chromium/WebView2): длинные задачи и Event Timing.
        try {
          new PerformanceObserver((list) => {
            if (probe.mode !== '') for (const e of list.getEntries()) probe.longTasks.push(e.duration);
          }).observe({ type: 'longtask' });
        } catch {
          /* движок без Long Tasks API */
        }
        try {
          new PerformanceObserver((list) => {
            if (probe.mode === '') return;
            for (const e of list.getEntries()) probe.eventMax = Math.max(probe.eventMax, e.duration);
          }).observe({ type: 'event', durationThreshold: 16 } as PerformanceObserverInit);
        } catch {
          /* движок без Event Timing API */
        }
      }
      const probe = w.__rzPerfProbe;
      probe.mode = mode;
      probe.arg = arg;
      probe.records = [];
      probe.longTasks = [];
      probe.eventMax = 0;
      performance.setResourceTimingBufferSize(100_000);
      performance.clearResourceTimings();
    },
    mode,
    arg,
  );
}

export interface ProbeExtras {
  /** Длинные задачи (Long Tasks API, только Chromium), мс. */
  longTasks: number[];
  /** Наибольшая длительность события по Event Timing (0 — нет данных). */
  eventMax: number;
  /** Длительности запросов к ядру по Resource Timing (команда в адресе `ipc`), мс. */
  ipc: { command: string; ms: number }[];
}

/** Ждёт, пока у `count` нажатий с начала замера будет кадр с ответом; задержки «нажатие → кадр», мс. */
export async function collectProbe(browser: WebdriverIO.Browser, count: number, timeoutMs = 10_000): Promise<number[]> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const records = await browser.execute(
      () => (window as unknown as { __rzPerfProbe?: { records: ProbeRecord[] } }).__rzPerfProbe?.records ?? null,
    );
    if (!records) throw new Error('замер не включён в этом окне');
    const done = records.filter((r) => r.paint !== null);
    if (records.length >= count && done.length === records.length) return records.map((r) => (r.paint ?? Number.NaN) - r.down);
    if (Date.now() > deadline) {
      throw new Error(
        `нет ответа за ${timeoutMs} мс: нажатий ${records.length} из ${count}, с кадром ${done.length}; ${JSON.stringify(records)}`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

/** Выключает замер; справочные данные за время замера. */
export async function finishProbe(browser: WebdriverIO.Browser): Promise<ProbeExtras> {
  return browser.execute(() => {
    const probe = (window as unknown as { __rzPerfProbe: { mode: string; longTasks: number[]; eventMax: number } }).__rzPerfProbe;
    probe.mode = '';
    const ipc = performance
      .getEntriesByType('resource')
      .filter((e) => /^(ipc:|https?:\/\/ipc\.localhost)/.test(e.name))
      .map((e) => ({
        command: decodeURIComponent(e.name.replace(/^.*\/\/[^/]+\//, '')),
        ms: (e as PerformanceResourceTiming).responseEnd - e.startTime,
      }));
    return { longTasks: probe.longTasks, eventMax: probe.eventMax, ipc };
  });
}
