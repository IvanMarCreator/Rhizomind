import { undo, history } from '@codemirror/commands';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ClipboardReadOutcome } from '../api/index.ts';
import { textMenuItems, textTargetOf, type PasteSource, type TextTarget } from './textMenu.ts';

const views: EditorView[] = [];

function editor(doc: string, options: { readOnly?: boolean; selection?: [number, number] } = {}): EditorView {
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      ...(options.selection ? { selection: { anchor: options.selection[0], head: options.selection[1] } } : {}),
      extensions: options.readOnly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : [history()],
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
  Reflect.deleteProperty(document, 'execCommand');
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

  it('без выделения «Вырезать» и «Копировать» серые; без команды ядра «Вставить» серый', () => {
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
    // В jsdom команд правки нет: подставная, снимается после теста.
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

describe('«Вставить» — текст буфера от ядра по щелчку (решение 98)', () => {
  function source(result: ClipboardReadOutcome | Error) {
    const notices: string[] = [];
    const read = vi.fn(() => (result instanceof Error ? Promise.reject(result) : Promise.resolve(result)));
    const paste: PasteSource = { read, notify: (text) => notices.push(text) };
    return { paste, read, notices };
  }

  async function settle() {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  it('построение меню буфер не читает; щелчок по «Вставить» — один вызов', async () => {
    const view = editor('ab', { selection: [1, 1] });
    const { paste, read } = source({ outcome: 'text', text: 'X' });
    const items = textMenuItems(target(view.contentDOM), paste);
    expect(read).not.toHaveBeenCalled();
    expect(item(items, 'paste').disabled).toBe(false);

    item(items, 'paste').run();
    await settle();

    expect(read).toHaveBeenCalledTimes(1);
    expect(view.state.doc.toString()).toBe('aXb');
  });

  it('в редакторе — вместо выделения, одной правкой: Ctrl+Z возвращает текст', async () => {
    const view = editor('одно два', { selection: [5, 8] });
    const { paste } = source({ outcome: 'text', text: 'три\nчетыре' });
    item(textMenuItems(target(view.contentDOM), paste), 'paste').run();
    await settle();
    expect(view.state.doc.toString()).toBe('одно три\nчетыре');

    undo(view);
    expect(view.state.doc.toString()).toBe('одно два');
  });

  it('только чтение: пункт серый; стала только для чтения за время запроса — не вставляется', async () => {
    const { paste } = source({ outcome: 'text', text: 'X' });
    const locked = editor('ab', { readOnly: true });
    expect(item(textMenuItems(target(locked.contentDOM), paste), 'paste').disabled).toBe(true);

    const view = editor('ab');
    const menu = textMenuItems(target(view.contentDOM), paste);
    view.dispatch({ effects: [] });
    view.setState(EditorState.create({ doc: 'ab', extensions: [EditorState.readOnly.of(true)] }));
    item(menu, 'paste').run();
    await settle();
    expect(view.state.doc.toString()).toBe('ab');
  });

  it('в поле: вместо выделения, переносы строк — пробелы', async () => {
    const field = input('abcd');
    field.setSelectionRange(1, 3);
    const { paste } = source({ outcome: 'text', text: 'x\ny' });
    let inputs = 0;
    field.addEventListener('input', () => (inputs += 1));

    item(textMenuItems(target(field), paste), 'paste').run();
    await settle();

    expect(field.value).toBe('ax yd');
    expect(inputs).toBe(1);
  });

  it('длиннее предела — уведомление, текст не вставляется; пусто и не в фокусе — ничего', async () => {
    const view = editor('ab');
    const big = source({ outcome: 'too_large', limit_bytes: 10 });
    item(textMenuItems(target(view.contentDOM), big.paste), 'paste').run();
    await settle();
    expect(view.state.doc.toString()).toBe('ab');
    expect(big.notices).toEqual([
      'Текст в буфере обмена слишком большой для вставки: больше «Максимального размера файла для редактирования» (Настройки → Файлы и ссылки).',
    ]);

    for (const result of [{ outcome: 'empty' }, { outcome: 'not_focused' }] as const) {
      const quiet = source(result);
      item(textMenuItems(target(view.contentDOM), quiet.paste), 'paste').run();
      await settle();
      expect(quiet.notices).toEqual([]);
    }
    expect(view.state.doc.toString()).toBe('ab');
  });

  it('ошибка ядра — уведомление текстом словаря', async () => {
    const view = editor('ab');
    const failing = source(Object.assign(new Error('x'), { code: 'io_failed', message_key: 'errors.io_failed' }));
    item(textMenuItems(target(view.contentDOM), failing.paste), 'paste').run();
    await settle();
    expect(failing.notices).toHaveLength(1);
    expect(view.state.doc.toString()).toBe('ab');
  });
});
