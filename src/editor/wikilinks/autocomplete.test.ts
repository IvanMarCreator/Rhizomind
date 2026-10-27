// Когда открывается автодополнение вики-ссылки (С3.4).
import { EditorSelection, EditorState } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { findTrigger } from './autocomplete.ts';

/** Курсор — `¦`. */
function at(marked: string, readOnly = false) {
  const pos = marked.indexOf('¦');
  return EditorState.create({
    doc: marked.slice(0, pos) + marked.slice(pos + 1),
    selection: EditorSelection.cursor(pos),
    extensions: EditorState.readOnly.of(readOnly),
  });
}

describe('findTrigger', () => {
  it.each([
    ['сразу после [[', 'a [[¦', ''],
    ['часть имени', 'a [[Вст¦', 'Вст'],
    ['путь', '[[Проекты/Альф¦', 'Проекты/Альф'],
    ['последняя [[ строки', '[[a]] и [[b¦', 'b'],
    ['закрывающие ]] после курсора', '[[Вс¦]]', 'Вс'],
  ])('%s', (_name, text, query) => {
    expect(findTrigger(at(text))?.query).toBe(query);
  });

  it.each([
    ['без [[', 'a ¦'],
    ['ссылка закрыта', '[[a]] ¦'],
    ['псевдоним', '[[a|вст¦'],
    ['заголовок', '[[a#ит¦'],
    ['встраивание — 0.3', '![[a¦'],
    ['[[ на строке выше', '[[a\nb¦'],
    ['заметка только для чтения', '[[a¦'],
  ])('нет: %s', (name, text) => {
    expect(findTrigger(at(text, name.includes('чтения')))).toBeNull();
  });

  it('позиции: от [[ до курсора', () => {
    expect(findTrigger(at('x\nab [[Вс¦'))).toEqual({ from: 7, to: 9, query: 'Вс' });
  });
});
