// Обратные ссылки на настоящем ядре (задачи 0.2-1.4 и 0.2-8.3, С2-3.2; КТ1,
// пп. 2–3): правая панель «Обратные ссылки» — группы по заметкам в
// естественном порядке, папка серым при повторе имени, строки с выделенной
// ссылкой (HTML из заметки — текстом), ссылка на себя не считается; щелчок по
// строке открывает заметку с курсором на ссылке, по имени — заметку сверху;
// ссылка, добавленная в файл снаружи, появляется в панели не позже 2 с.
// Снимки — на каждом шаге в светлой и тёмной теме (`backlinks-NN-…`).
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestProfile, createTestVault, removeTestVault, startApp, type Session } from './driver.ts';
import { backlinkGroups, caret, headerPath, openFromTree, panelHeading, showView, TEXT, themeShots } from './kt1.ts';

const LINE = 'Сегодня обсудили [[Узел]] и <b>планы</b>';
const HEADING_LINE = 'Ещё раз: [[Узел#Раздел|раздел узла]].';
/** Предел С2-3.2: список обновляется не позже 2 с после сохранения заметки со ссылкой. */
const REFRESH_LIMIT_MS = 2_000;

const shot = themeShots('backlinks');

describe('обратные ссылки', () => {
  let session: Session | undefined;
  let vault = '';
  let profile = '';

  beforeAll(async () => {
    vault = createTestVault('Связи', {
      'Узел.md': '# Узел\n\nСм. [[Узел]] — ссылка на себя.\n\n## Раздел\n\nТекст узла.\n',
      'Дневник.md': `# Дневник\n\n${LINE}\n\n${HEADING_LINE}\n`,
      'Проект.md': 'План связан с [[Узел]].\n',
      'Третья.md': 'Пока без ссылок.\n',
      'Без ссылок.md': 'Здесь ничего нет.\n',
    });
    // Повтор имени «Дневник» — в группе видна папка (С2-3.2).
    const archive = path.join(vault, 'Архив');
    mkdirSync(archive);
    writeFileSync(path.join(archive, 'Дневник.md'), 'Старое упоминание ![[Узел]]\n');
    profile = createTestProfile();
    session = await startApp({ vault, profile });
    await openFromTree(session.browser, 'Узел');
  });

  afterAll(async () => {
    await session?.stop().catch(() => undefined);
    if (vault) removeTestVault(vault);
    if (profile) rmSync(profile, { recursive: true, force: true });
  });

  const browser = () => {
    if (!session) throw new Error('нет сессии');
    return session.browser;
  };

  it('группы по заметкам и строки со ссылкой; ссылки на себя нет; HTML — текстом', async () => {
    const b = browser();
    await showView(b, 'Обратные ссылки');
    await b.waitUntil(async () => (await backlinkGroups(b)).length === 3, {
      timeout: 15_000,
      timeoutMsg: 'в панели не три ссылающиеся заметки',
    });
    const groups = await backlinkGroups(b);
    expect(groups.map((g) => g.name)).toEqual(['Дневник', 'Дневник', 'Проект']);
    // Папка — только у повторяющегося имени.
    expect(groups.map((g) => g.folder)).toEqual(['Архив', 'корень хранилища', '']);
    expect(groups[0]?.links).toEqual(['![[Узел]]']);
    expect(groups[1]?.lines).toEqual([LINE, HEADING_LINE]);
    expect(groups[1]?.links).toEqual(['[[Узел]]', '[[Узел#Раздел|раздел узла]]']);
    expect(groups[1]?.total).toBe('2');
    expect(groups[2]?.lines).toEqual(['План связан с [[Узел]].']);
    // Строка показана текстом: разметки из заметки в панели нет.
    expect(await b.execute(() => document.querySelector('.backlinks .lines b') === null)).toBe(true);
    expect(await panelHeading(b)).toBe('Обратные ссылки 3');
    await shot(b, 'groups');
  });

  it('щелчок по строке открывает заметку и ставит курсор на ссылку (фокус в тексте)', async () => {
    const b = browser();
    // Строки по порядку: «Архив / Дневник» (1), «Дневник» (2), где вторая — ссылка на заголовок.
    const line = (await b.$$('.backlinks .groups > .group .lines .line'))[2];
    if (!line) throw new Error('нет строки со ссылкой на заголовок');
    await line.click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Дневник', { timeout: 5_000, timeoutMsg: 'не открылся «Дневник»' });
    await b.waitUntil(async () => (await caret(b)).after.startsWith('[[Узел#Раздел'), {
      timeout: 5_000,
        timeoutMsg: 'курсор не на ссылке',
      })
      .catch(async (error: unknown) => {
        throw new Error(`${String(error)}: ${JSON.stringify(await caret(b))}`);
      });
    expect((await caret(b)).focused).toBe(true);
    await shot(b, 'line-click-cursor');
  });

  it('щелчок по имени открывает заметку сверху', async () => {
    const b = browser();
    await openFromTree(b, 'Узел');
    await b.waitUntil(async () => (await backlinkGroups(b)).length === 3, { timeout: 10_000, timeoutMsg: 'нет списка' });
    await b.$('.backlinks button.note[aria-label="Открыть «Проект»"]').click();
    await b.waitUntil(async () => (await headerPath(b)) === 'Проект', { timeout: 5_000, timeoutMsg: 'не открылся «Проект»' });
    const where = await caret(b);
    expect(where.focused).toBe(true);
    expect(where.after).toBe('План связан с [[Узел]].');
    await shot(b, 'name-click-top');
  });

  it(`ссылка, добавленная снаружи, — в панели не позже ${REFRESH_LIMIT_MS / 1000} с`, async () => {
    const b = browser();
    await openFromTree(b, 'Узел');
    await b.waitUntil(async () => (await backlinkGroups(b)).length === 3, { timeout: 10_000, timeoutMsg: 'нет списка' });
    const started = Date.now();
    writeFileSync(path.join(vault, 'Третья.md'), 'Пока без ссылок.\n\nНовая ссылка на [[Узел]] снаружи.\n');
    let elapsed = -1;
    while (Date.now() - started < 10_000) {
      if ((await backlinkGroups(b)).some((g) => g.name === 'Третья')) {
        elapsed = Date.now() - started;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    console.log(`[kt1] внешняя ссылка в панели через ${elapsed} мс`);
    expect(elapsed, 'ссылка не появилась за 10 с').toBeGreaterThanOrEqual(0);
    expect(elapsed).toBeLessThanOrEqual(REFRESH_LIMIT_MS);
    const third = (await backlinkGroups(b)).find((g) => g.name === 'Третья');
    expect(third?.lines).toEqual(['Новая ссылка на [[Узел]] снаружи.']);
    expect(await panelHeading(b)).toBe('Обратные ссылки 4');
    await shot(b, 'external-link-added');
  });

  it('заметка без обратных ссылок — «Нет обратных ссылок»', async () => {
    const b = browser();
    await openFromTree(b, 'Без ссылок');
    await b.waitUntil(() => b.execute(() => document.querySelector('.backlinks')?.textContent?.includes('Нет обратных ссылок') ?? false), {
      timeout: 5_000,
      timeoutMsg: 'нет состояния «Нет обратных ссылок»',
    });
    expect(await b.$(TEXT).isExisting()).toBe(true);
    await shot(b, 'empty');
  });
});
