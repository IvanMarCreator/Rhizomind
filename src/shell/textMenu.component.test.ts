import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { textMenuItems, textTargetOf, type TextTarget } from './textMenu.ts';

const views: EditorView[] = [];

function editor(doc: string, options: { readOnly?: boolean; selection?: [number, number] } = {}): EditorView {
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      ...(options.selection ? { selection: { anchor: options.selection[0], head: options.selection[1] } } : {}),
      extensions: options.readOnly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : [],
    }),
  });
  views.push(view);
  return view;
}

function input(value: string, attrs = ''): HTMLInputElement {
  const host = document.createElement('div');
  host.innerHTML = `<input ${attrs} />`;
  document.body.appendChild(host);
  const element = host.querySelector('input') as HTMLInputElement;
  element.value = value;
  return element;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.innerHTML = '';
  vi.restoreAllMocks();
});

/** Цель меню; тест падает, если её нет. */
function target(node: EventTarget | null): TextTarget {
  const found = textTargetOf(node);
  if (!found) throw new Error('нет цели меню');
  return found;
}

function item(items: ReturnType<typeof textMenuItems>, id: string) {
  const found = items.find((entry) => entry.id === id);
  if (!found) throw new Error(`нет пункта ${id}`);
  return found;
}

function labels(items: ReturnType<typeof textMenuItems>) {
  return items.map((item) => `${item.label}${item.disabled ? ' (серый)' : ''} ${item.shortcut ?? ''}`.trim());
}

describe('меню правки текста (решение 97)', () => {
  it('цель — текст заметки, поле ввода, многострочное поле; не текст — нет меню', () => {
    const view = editor('текст');
    expect(textTargetOf(view.contentDOM)).not.toBeNull();
    expect(textTargetOf(view.contentDOM.firstChild)).not.toBeNull();
    expect(textTargetOf(input('a'))).not.toBeNull();
    const area = document.createElement('textarea');
    document.body.appendChild(area);
    expect(textTargetOf(area)).not.toBeNull();
    const button = document.createElement('button');
    document.body.appendChild(button);
    expect(textTargetOf(button)).toBeNull();
    expect(textTargetOf(document.body)).toBeNull();
    expect(textTargetOf(input('', 'type="checkbox"'))).toBeNull();
  });

  it('без выделения «Вырезать» и «Копировать» серые; «Вставить» серый — только Ctrl+V', () => {
    const found = target(editor('текст').contentDOM);
    expect(labels(textMenuItems(found))).toEqual([
      'Вырезать (серый) Ctrl+X',
      'Копировать (серый) Ctrl+C',
      'Вставить (серый) Ctrl+V',
      'Выделить всё Ctrl+A',
    ]);
  });

  it('с выделением «Вырезать» и «Копировать» активны', () => {
    const found = target(editor('текст', { selection: [0, 3] }).contentDOM);
    expect(labels(textMenuItems(found)).slice(0, 2)).toEqual(['Вырезать Ctrl+X', 'Копировать Ctrl+C']);
  });

  it('только чтение: «Вырезать» серый, «Копировать» активен', () => {
    const found = target(editor('текст', { readOnly: true, selection: [0, 3] }).contentDOM);
    expect(labels(textMenuItems(found)).slice(0, 2)).toEqual(['Вырезать (серый) Ctrl+X', 'Копировать Ctrl+C']);
    const field = input('abc', 'readonly');
    field.setSelectionRange(0, 2);
    expect(labels(textMenuItems(target(field))).slice(0, 2)).toEqual([
      'Вырезать (серый) Ctrl+X',
      'Копировать Ctrl+C',
    ]);
  });

  it('«Копировать» и «Вырезать» в тексте заметки: фокус в редактор, команда движка copy/cut', () => {
    const view = editor('текст', { selection: [0, 3] });
    const exec = vi.fn((command: string) => {
      // Команду движок выполняет над тем, где фокус.
      expect(view.hasFocus || document.activeElement === view.contentDOM, command).toBe(true);
      return true;
    });
    document.execCommand = exec;
    const items = textMenuItems(target(view.contentDOM));
    item(items, 'copy').run();
    item(items, 'cut').run();
    expect(exec.mock.calls.map(([command]) => command)).toEqual(['copy', 'cut']);
  });

  it('«Выделить всё» в тексте заметки выделяет весь текст', () => {
    const view = editor('первая\nвторая');
    item(textMenuItems(target(view.contentDOM)), 'select-all').run();
    expect(view.state.selection.main.from).toBe(0);
    expect(view.state.selection.main.to).toBe(view.state.doc.length);
  });

  it('поле ввода: выделение видно по selectionStart/End, «Выделить всё» выделяет значение', () => {
    const field = input('abcdef');
    field.setSelectionRange(1, 3);
    expect(item(textMenuItems(target(field)), 'copy').disabled).toBe(false);
    item(textMenuItems(target(field)), 'select-all').run();
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, 6]);
    expect(document.activeElement).toBe(field);
  });

  it('правый щелчок вне выделения ставит курсор туда, внутри выделения — оставляет выделение', () => {
    const view = editor('одно два три', { selection: [0, 4] });
    vi.spyOn(view, 'posAtCoords').mockReturnValueOnce(2).mockReturnValueOnce(9);
    const found = target(view.contentDOM);
    found.placeCaret(10, 10);
    expect([view.state.selection.main.from, view.state.selection.main.to]).toEqual([0, 4]);
    found.placeCaret(50, 10);
    expect([view.state.selection.main.from, view.state.selection.main.to]).toEqual([9, 9]);
  });
});
