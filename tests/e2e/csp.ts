// Нарушения CSP по стилям в окне (задача 2.10) — для сценарных тестов редактора.

/**
 * Нарушения CSP по стилям в окне (задача 2.10): слушатель ставится до открытия
 * заметки. Ожидаемые — два элемента <style>, которые вставляют библиотеки и
 * которые CSP запрещает, а их правила повторены в нашем статическом CSS:
 * базовая тема CodeMirror (пакет style-mod, классы «ͼ…») —
 * src/editor/codemirror-base.css; стиль изолированного iframe Tauri
 * (`#__tauri_isolation__`, tauri/scripts/isolation.js) — src/shell/base.css.
 * Любое другое нарушение — ошибка. Что именно лежит в элементе CodeMirror и
 * чем закрыт каждый его селектор, сверяет
 * src/editor/codemirror-styles.component.test.ts (задача 2.10а): новая тема
 * роняет тот тест, а не проходит здесь как «известная».
 */
const KNOWN = ' [известный: правила в статическом CSS]';

export async function watchCsp(browser: WebdriverIO.Browser): Promise<void> {
  await browser.execute((KNOWN) => {
    const log: string[] = [];
    (window as unknown as { rzCsp: string[] }).rzCsp = log;
    document.addEventListener('securitypolicyviolation', (event) => {
      const target = event.target;
      const text = target instanceof HTMLStyleElement ? (target.textContent ?? '') : '';
      const known = text.includes('\u037c') || text.trimStart().startsWith('#__tauri_isolation__');
      const where = target instanceof Element ? target.tagName : String(target);
      log.push(`${event.effectiveDirective} ${event.blockedURI} ${where} ${event.sourceFile}:${event.lineNumber} ${text.slice(0, 60)}${known ? KNOWN : ''}`);
    });
  }, KNOWN);
}

export async function unexpectedStyleViolations(browser: WebdriverIO.Browser): Promise<string[]> {
  const log = await browser.execute(() => (window as unknown as { rzCsp?: string[] }).rzCsp ?? ['слушатель не установлен']);
  return log.filter((entry) => !entry.endsWith(KNOWN));
}
