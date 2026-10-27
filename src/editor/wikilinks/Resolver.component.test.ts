// Разрешение вики-ссылок видимой части (С3.4) на подставном `WikiHost`:
// пауза перед вопросом ядру, вопросы только о новых ключах, порядок ответов
// после изменения дерева хранилища (vault:changed).
import { EditorView } from '@codemirror/view';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createEditorState } from '../codemirror.ts';
import { RESOLVE_DELAY_MS } from '../live-preview/index.ts';
import type { WikiEvent, WikiHost } from './host.ts';
import { resolutionField } from './resolve.ts';

interface Pending {
  keys: readonly string[];
  answer(map: Record<string, boolean>): void;
}

function fakeHost() {
  const calls: Pending[] = [];
  const listeners = new Set<(event: WikiEvent) => void>();
  const host: WikiHost = {
    resolve: (keys) =>
      new Promise((resolve) => {
        calls.push({ keys: [...keys], answer: (map) => resolve(new Map(Object.entries(map))) });
      }),
    open: () => undefined,
    suggest: () => Promise.resolve([]),
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const emit = (event: WikiEvent) => {
    for (const listener of [...listeners]) listener(event);
  };
  return { host, calls, emit };
}

let views: EditorView[] = [];

function editor(text: string, host: WikiHost): EditorView {
  const view = new EditorView({
    parent: document.body,
    state: createEditorState(text, { mode: 'live_preview', wiki: host }),
  });
  views.push(view);
  return view;
}

function typeText(view: EditorView, text: string): void {
  const at = view.state.doc.length;
  view.dispatch({ changes: { from: at, insert: text }, selection: { anchor: at + text.length }, userEvent: 'input.type' });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  for (const view of views) view.destroy();
  views = [];
  vi.useRealTimers();
});

describe('разрешение ссылок: пауза и только новые ключи', () => {
  it(`N правок подряд — один вопрос через ${RESOLVE_DELAY_MS} мс; повторных ключей нет`, async () => {
    const { host, calls } = fakeHost();
    const view = editor('', host);
    await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS);
    expect(calls).toHaveLength(0);

    for (const char of 'Сегодня [[Бета]] и [[Гамма]]') {
      typeText(view, char);
      await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS - 50);
    }
    expect(calls).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(50);
    expect(calls.map((call) => call.keys)).toEqual([['Бета', 'Гамма']]);
    calls[0]?.answer({ Бета: true, Гамма: false });
    await vi.advanceTimersByTimeAsync(0);

    // Правки без новых ключей и повтор уже известного — ядро не спрашивается.
    typeText(view, ' текст');
    typeText(view, ' и снова [[Бета]]');
    await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS * 2);
    expect(calls).toHaveLength(1);

    // Новый ключ — вопрос только о нём.
    typeText(view, ' [[Дельта]]');
    await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS);
    expect(calls.map((call) => call.keys)).toEqual([['Бета', 'Гамма'], ['Дельта']]);
    const all = calls.flatMap((call) => call.keys);
    expect(new Set(all).size).toBe(all.length);
  });
});

describe('разрешение ссылок после vault:changed', () => {
  it('поздний ответ прежнего поколения не затирает свежий', async () => {
    const { host, calls, emit } = fakeHost();
    const view = editor('См. [[План]]\n\nконец', host);
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS);
    expect(calls.map((call) => call.keys)).toEqual([['План']]);

    // Дерево изменилось до ответа: ссылки спрашиваются заново.
    emit({ kind: 'refresh' });
    await vi.advanceTimersByTimeAsync(RESOLVE_DELAY_MS);
    expect(calls.map((call) => call.keys)).toEqual([['План'], ['План']]);

    calls[1]?.answer({ План: true });
    await vi.advanceTimersByTimeAsync(0);
    calls[0]?.answer({ План: false });
    await vi.advanceTimersByTimeAsync(0);

    expect(view.state.field(resolutionField).known.get('План')).toBe(true);
    expect(view.contentDOM.querySelector('.cm-rz-wikilink-unresolved')).toBeNull();
  });
});
