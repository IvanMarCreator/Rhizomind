import type { PerfVault } from './vault.ts';

const MANAGER = '[data-testid="vault-manager"]';

export interface TreeOpen {
  /** Щелчок → дерево увидел тест (часы теста), мс. */
  total: number;
  /** Щелчок → начало загрузки страницы нового окна (часы окна). */
  navigation: number;
  /** Щелчок → DOMContentLoaded нового окна: скрипт интерфейса загружен и выполнен; `null` — нет данных. */
  domReady: number | null;
  /** Начало и конец запроса `tree_list` от щелчка; `null` — движок не даёт Resource Timing для ipc. */
  treeList: { start: number; end: number } | null;
  /** Щелчок → дерево в DOM (часы окна). */
  rendered: number;
}

/** Щелчок в окне управления и переход в новое окно. Не ждёт дерево или индекс. */
export async function enterFromManager(browser: WebdriverIO.Browser): Promise<number> {
  await browser.$(MANAGER).waitForDisplayed({ timeout: 60_000 });
  const manager = await browser.getWindowHandle();
  const button = browser.$('button[aria-label="Открыть папку как хранилище"]');
  await button.waitForClickable({ timeout: 10_000 });
  const clickAt = Date.now();
  await button.click();
  const deadline = clickAt + 60_000;
  for (;;) {
    if (Date.now() > deadline) throw new Error('окно хранилища не появилось за 60 с');
    const other = (await browser.getWindowHandles()).find((handle) => handle !== manager);
    if (!other) continue;
    try {
      await browser.switchToWindow(other);
      return clickAt;
    } catch {
      // Окно ещё создаётся.
    }
  }
}

/** Щелчок в окне управления → все записи корня в дереве нового окна. */
export async function openFromManager(browser: WebdriverIO.Browser, vault: PerfVault): Promise<TreeOpen> {
  const clickAt = await enterFromManager(browser);
  for (;;) {
    if (Date.now() > clickAt + 60_000) throw new Error(`дерево не показано за 60 с (записей корня ${vault.rootEntries})`);
    try {
      const state = await browser.execute(() => {
        const nav = performance.timeOrigin;
        const ipc = performance
          .getEntriesByType('resource')
          .filter((e) => /tree_list/.test(e.name))
          .map((e) => ({ start: nav + e.startTime, end: nav + (e as PerformanceResourceTiming).responseEnd }));
        const page = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
        return {
          items: document.querySelectorAll('[role="treeitem"]').length,
          nav,
          domReady: page && page.domContentLoadedEventEnd > 0 ? nav + page.domContentLoadedEventEnd : null,
          now: nav + performance.now(),
          treeList: ipc[0] ?? null,
        };
      });
      if (state.items >= vault.rootEntries) {
        const seen = Date.now();
        return {
          total: seen - clickAt,
          navigation: state.nav - clickAt,
          domReady: state.domReady === null ? null : state.domReady - clickAt,
          treeList: state.treeList ? { start: state.treeList.start - clickAt, end: state.treeList.end - clickAt } : null,
          rendered: state.now - clickAt,
        };
      }
    } catch {
      // Окно ещё создаётся (WebDriver видит его раньше, чем страницу).
    }
  }
}

export async function indexReady(browser: WebdriverIO.Browser): Promise<number> {
  let elapsed = 0;
  await browser.waitUntil(async () => {
    const result = await browser.execute(async () => {
      const api = (window as unknown as { __TAURI_INTERNALS__?: { invoke(cmd: string): Promise<{ state: string }> } }).__TAURI_INTERNALS__;
      const status = await api?.invoke('index_status');
      return { state: status?.state, elapsed: performance.now() };
    });
    elapsed = result.elapsed;
    return result.state === 'ready';
  }, { timeout: 180_000, interval: 25, timeoutMsg: 'индекс не готов' });
  // performance.now() — от начала навигации окна. Опрос даёт верхнюю оценку.
  return elapsed;
}
