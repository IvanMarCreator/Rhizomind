// Базовые стили CodeMirror без вставки стилей (находки Н7 и Н3,
// docs/reviews/2026-10-26-task-3.1.md). CodeMirror вставляет свою базовую тему
// элементом <style> (пакет style-mod), а CSP окна (`style-src 'self'`,
// src-tauri/tauri.conf.json) его запрещает — на WebView2 и на WebKitGTK.
// Здесь CSP имитируется: элемент <style> от CodeMirror убирается, остаётся
// только наш статический CSS (codemirror-base.css). jsdom не считает
// раскладку, поэтому видимый сдвиг проверяет сценарный тест
// (tests/e2e/live-preview.test.ts); здесь — вычисленные стили, от которых он
// зависит.
import { EditorView } from '@codemirror/view';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createEditorState } from './codemirror.ts';

const baseCss = readFileSync(path.join(import.meta.dirname, 'codemirror-base.css'), 'utf8');

let view: EditorView | undefined;

function current(): EditorView {
  if (!view) throw new Error('редактор не создан');
  return view;
}

beforeEach(() => {
  // Рамка фокуса браузерного движка у contenteditable (её и видно на снимках Н3).
  const engine = document.createElement('style');
  engine.textContent = '[contenteditable]:focus { outline: auto 1px; }';
  document.head.append(engine);
  const own = document.createElement('style');
  own.dataset.test = 'codemirror-base';
  own.textContent = baseCss;
  document.head.append(own);
  view = new EditorView({ parent: document.body, state: createEditorState('- Пункт\n  - Вложенный пункт\n') });
  // Как CSP: правила, вставленные CodeMirror (классы вида «ͼ1»), не действуют.
  for (const style of document.querySelectorAll('style')) {
    if ((style.textContent ?? '').includes('ͼ')) style.remove();
  }
});

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.head.replaceChildren();
  document.body.replaceChildren();
});

describe('базовые стили CodeMirror — статический CSS, без вставки стилей', () => {
  it('вставленных CodeMirror правил нет: проверяется только наш CSS', () => {
    const inserted = [...document.querySelectorAll('style')].filter((style) => (style.textContent ?? '').includes('ͼ'));
    expect(inserted).toHaveLength(0);
  });

  it('пробелы в начале строки и подряд сохраняются (Н7)', () => {
    const style = getComputedStyle(current().contentDOM);
    expect(['pre-wrap', 'break-spaces']).toContain(style.whiteSpace);
  });

  it('раскладка редактора как в базовой теме: колонка, строки — блоки', () => {
    expect(getComputedStyle(current().dom).display).toBe('flex');
    expect(getComputedStyle(current().scrollDOM).display).toBe('flex');
    expect(getComputedStyle(current().contentDOM).display).toBe('block');
    const line = current().contentDOM.querySelector('.cm-line');
    expect(line && getComputedStyle(line).display).toBe('block');
  });

  it('рамки фокуса вокруг всей области текста нет (Н3)', () => {
    current().focus();
    expect(document.activeElement).toBe(current().contentDOM);
    expect(current().contentDOM.matches('[contenteditable]:focus')).toBe(true);
    // jsdom не раскладывает сокращённое свойство outline — сравнивается оно само.
    expect(getComputedStyle(current().contentDOM).getPropertyValue('outline')).toBe('none');
  });
});
