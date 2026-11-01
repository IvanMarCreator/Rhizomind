// Вид списка автодополнения вики-ссылок (задача 2.14, замечание КТ2):
// выбранный пункт заметно выделен — фон `accent/indigo/subtle`, как у
// выбранного пункта быстрого перехода (docs/design/tokens.md), в светлой и
// тёмной теме. Прежний `bg/hover` на `bg/popover` в тёмной теме почти не
// отличался от фона списка (#26282C на #232529). Стили — только из нашего
// статического CSS: правила, которые вставляет CodeMirror, CSP окна режет,
// здесь они убраны.
import { EditorSelection } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { waitFor } from '@testing-library/svelte';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import type { LinkSuggestion } from '../../api/index.ts';
import { createEditorState } from '../codemirror.ts';
import type { WikiHost } from './host.ts';

const css = (file: string) => readFileSync(path.join(import.meta.dirname, file), 'utf8');
const OUR_CSS = [css('../../theme/tokens.css'), css('../codemirror-base.css'), css('wikilinks.css')].join('\n');

const ITEMS: LinkSuggestion[] = [
  { insert: 'План', name: 'План', path: ['План.md'], linkable: true },
  { insert: 'Планы', name: 'Планы', path: ['Планы.md'], linkable: true },
];

const host: WikiHost = {
  resolve: async () => new Map(),
  open: () => undefined,
  suggest: async () => ITEMS,
  subscribe: () => () => undefined,
};

let view: EditorView | undefined;
const originalRects = Range.prototype.getClientRects;
const originalRect = Range.prototype.getBoundingClientRect;

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

afterAll(() => {
  Range.prototype.getClientRects = originalRects;
  Range.prototype.getBoundingClientRect = originalRect;
});

afterEach(() => {
  view?.destroy();
  view = undefined;
  delete document.documentElement.dataset.theme;
  document.head.replaceChildren();
  document.body.replaceChildren();
});

/** Значение цвета с подставленными `var(--…)` из корня документа. */
function resolved(value: string): string {
  const root = getComputedStyle(document.documentElement);
  let current = value.trim();
  for (let i = 0; i < 5; i++) {
    const match = /^var\((--[\w-]+)\)$/.exec(current);
    if (!match?.[1]) break;
    current = root.getPropertyValue(match[1]).trim();
  }
  return current.toLowerCase();
}

async function openList(): Promise<HTMLElement[]> {
  const style = document.createElement('style');
  style.textContent = OUR_CSS;
  document.head.append(style);
  const text = 'См. [[Пл';
  const state = createEditorState(text.slice(0, -1), { mode: 'live_preview', wiki: host });
  view = new EditorView({ parent: document.body, state });
  view.dispatch({
    changes: { from: text.length - 1, insert: text.slice(-1) },
    selection: EditorSelection.cursor(text.length),
    userEvent: 'input.type',
  });
  // Как CSP: правила, вставленные CodeMirror (классы вида «ͼ1»), не действуют.
  for (const element of document.querySelectorAll('style')) {
    if ((element.textContent ?? '').includes('ͼ')) element.remove();
  }
  const dom = view.dom;
  await waitFor(() => expect(dom.querySelectorAll('[role="option"]')).toHaveLength(2));
  return [...dom.querySelectorAll<HTMLElement>('[role="option"]')];
}

describe.each([
  ['light', '#e8eafb', '#ffffff'],
  ['dark', '#2a2e52', '#232529'],
] as const)('выбранный пункт списка [[ — тема %s', (theme, selectedBg, popoverBg) => {
  it('выделен фоном accent/indigo/subtle, как в быстром переходе; остальные — без фона', async () => {
    document.documentElement.dataset.theme = theme;
    const [selected, other] = await openList();
    expect(selected?.getAttribute('aria-selected')).toBe('true');
    expect(other?.getAttribute('aria-selected')).toBe('false');

    const list = view?.dom.querySelector('.cm-tooltip.cm-rz-suggest') as HTMLElement;
    expect(resolved(getComputedStyle(list).backgroundColor)).toBe(popoverBg);
    expect(resolved(getComputedStyle(selected as HTMLElement).backgroundColor)).toBe(selectedBg);
    const otherBg = resolved(getComputedStyle(other as HTMLElement).backgroundColor);
    expect(['', 'transparent', 'rgba(0, 0, 0, 0)']).toContain(otherBg);
  });
});
