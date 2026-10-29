// Замена документа текстом с диска (задача 2.9, С3.6): курсор сохраняется по
// возможности, правка не попадает в отмену и не считается вводом.
import { undo } from '@codemirror/commands';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it } from 'vitest';
import { createEditorState, replaceDocument } from './codemirror.ts';

const views: EditorView[] = [];

function editor(text: string, cursor: number) {
  const edits: string[] = [];
  const view = new EditorView({
    parent: document.body,
    state: createEditorState(text, { onChange: (next) => edits.push(next) }),
  });
  view.dispatch({ selection: { anchor: cursor } });
  views.push(view);
  return { view, edits };
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

describe('replaceDocument', () => {
  it('правка до курсора сдвигает курсор вместе с текстом', () => {
    const { view } = editor('один два три', 'один два '.length);
    replaceDocument(view, 'ноль один два три');
    expect(view.state.doc.toString()).toBe('ноль один два три');
    expect(view.state.selection.main.head).toBe('ноль один два '.length);
  });

  it('правка после курсора курсор не трогает', () => {
    const { view } = editor('один два три', 3);
    replaceDocument(view, 'один два три четыре');
    expect(view.state.selection.main.head).toBe(3);
  });

  it('курсор внутри заменённого куска — на его границе, в пределах документа', () => {
    const { view } = editor('aaa XXXXXX bbb', 7);
    replaceDocument(view, 'aaa Y bbb');
    const head = view.state.selection.main.head;
    expect(head).toBeGreaterThanOrEqual(4);
    expect(head).toBeLessThanOrEqual(5);
  });

  it('эмодзи на границе правки не разрезается: текст ровно тот, что на диске', () => {
    const { view } = editor('a😀b', 0);
    replaceDocument(view, 'a😃b');
    expect(view.state.doc.toString()).toBe('a😃b');
  });

  it('не правка пользователя и не шаг отмены', () => {
    const { view, edits } = editor('старое', 0);
    replaceDocument(view, 'новое');
    expect(edits).toEqual([]);
    expect(undo(view)).toBe(false);
  });

  it('тот же текст — документ не трогается', () => {
    const { view } = editor('текст', 2);
    const before = view.state;
    replaceDocument(view, 'текст');
    expect(view.state).toBe(before);
  });
});
