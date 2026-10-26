// Настройка CodeMirror для правила «байт в байт» (ADR 0001, С3.3): документ
// редактора хранит текст ровно таким, каким его отдало ядро.
import { undo } from '@codemirror/commands';
import type { EditorState, Transaction } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { createEditorState, editorKeymap, normalizePastedText } from './codemirror.ts';

const SAMPLES: [string, string][] = [
  ['пустой текст', ''],
  ['без перевода строки в конце', '# Заголовок\nтекст'],
  ['пустые строки и пробелы в конце строк', '\n\nа  \n\tб\t\n\n'],
  ['одиночный CR (старый Mac) остаётся символом', 'a\rb\r\nc\r'],
  ['разделители строк Unicode', 'a b c\u0085d'],
  ['NUL и управляющие символы', 'a\u0000b\u0007c\u001b'],
  ['суррогатные пары и составные символы', '👩🏽‍💻 é = é, 𝕏'],
  ['символ замены из невалидного UTF-8', 'ab�cd'],
  ['BOM в середине текста (не первый байт файла)', 'a﻿b'],
  ['длинный текст', 'строка\n'.repeat(5000)],
];

describe('документ редактора — тот же текст', () => {
  it.each(SAMPLES)('%s', (_name, text) => {
    expect(createEditorState(text).doc.toString()).toBe(text);
  });

  it('правка и отмена возвращают ровно исходный текст', () => {
    const text = 'a\rb\n c';
    let state: EditorState = createEditorState(text);
    state = state.update({ changes: { from: 1, insert: 'XYZ\n' }, userEvent: 'input.type' }).state;
    expect(state.doc.toString()).toBe('aXYZ\n\rb\n c');

    undo({
      state,
      dispatch: (tr: Transaction) => {
        state = tr.state;
      },
    });

    expect(state.doc.toString()).toBe(text);
  });
});

describe('вставка из буфера обмена', () => {
  it('переносы Windows и одиночные CR из буфера становятся \\n', () => {
    expect(normalizePastedText('a\r\nb\rc\nd')).toBe('a\nb\nc\nd');
  });
});

describe('только чтение', () => {
  it('состояние только для чтения', () => {
    expect(createEditorState('x', { readOnly: true }).readOnly).toBe(true);
    expect(createEditorState('x').readOnly).toBe(false);
  });
});

describe('клавиши', () => {
  it('Alt+← и Alt+→ остаются за историей переходов окна', () => {
    const keys = editorKeymap.map((binding) => binding.key);
    expect(keys).not.toContain('Alt-ArrowLeft');
    expect(keys).not.toContain('Alt-ArrowRight');
    expect(keys).toContain('Mod-z');
  });
});
