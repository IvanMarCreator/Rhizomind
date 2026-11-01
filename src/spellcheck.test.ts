// Проверка орфографии движка выключена во всех полях ввода интерфейса (аудит
// 3.3, З3; docs/SECURITY.md): каждый <input>, <textarea> и элемент с
// contenteditable в разметке .svelte несёт spellcheck="false". Редактор
// заметки (CodeMirror) ставит атрибут сам — его проверяет Editor.component.test.ts.
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { parse } from 'svelte/compiler';
import { describe, expect, it } from 'vitest';

const SRC = import.meta.dirname;

function svelteFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return svelteFiles(full);
    return entry.name.endsWith('.svelte') ? [full] : [];
  });
}

interface Attribute {
  type: string;
  name?: string;
  value?: unknown;
}

interface Element {
  type: 'RegularElement';
  name: string;
  attributes: Attribute[];
  start: number;
}

/** Все обычные элементы разметки (в блоках {#if}, {#each}, компонентах и т. д.). */
function elements(node: unknown, found: Element[] = []): Element[] {
  if (Array.isArray(node)) {
    for (const item of node) elements(item, found);
  } else if (node !== null && typeof node === 'object') {
    const record = node as Record<string, unknown>;
    if (record.type === 'RegularElement') found.push(record as unknown as Element);
    for (const [key, value] of Object.entries(record)) {
      // Скрипты и стили компонента — не разметка.
      if (key === 'instance' || key === 'module' || key === 'css') continue;
      elements(value, found);
    }
  }
  return found;
}

function staticValue(attribute: Attribute | undefined): string | undefined {
  if (!attribute || !Array.isArray(attribute.value)) return undefined;
  const parts = attribute.value as { type: string; data?: string }[];
  return parts.every((part) => part.type === 'Text') ? parts.map((part) => part.data).join('') : undefined;
}

function needsSpellcheckOff(element: Element): boolean {
  if (element.name === 'input' || element.name === 'textarea') return true;
  const editable = element.attributes.find((a) => a.type === 'Attribute' && a.name === 'contenteditable');
  return editable !== undefined && staticValue(editable) !== 'false';
}

describe('проверка орфографии в полях ввода', () => {
  const files = svelteFiles(SRC);

  it('находит компоненты с полями ввода', () => {
    const withFields = files.filter((file) =>
      elements(parse(readFileSync(file, 'utf8'), { modern: true }).fragment).some(needsSpellcheckOff),
    );
    // Переименование, быстрый переход, создание хранилища, поиск горячих клавиш,
    // фильтр папок, числовые поля настроек.
    expect(withFields.length).toBeGreaterThanOrEqual(6);
  });

  it('каждое поле ввода — со spellcheck="false"', () => {
    const missing: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, 'utf8');
      for (const element of elements(parse(source, { modern: true }).fragment)) {
        if (!needsSpellcheckOff(element)) continue;
        const spellcheck = element.attributes.find((a) => a.type === 'Attribute' && a.name === 'spellcheck');
        if (staticValue(spellcheck) !== 'false') {
          const line = source.slice(0, element.start).split('\n').length;
          missing.push(`${path.relative(SRC, file)}:${line} <${element.name}>`);
        }
      }
    }
    expect(missing).toEqual([]);
  });
});
