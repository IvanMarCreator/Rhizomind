// Меню правки текста (решение 97): своё контекстное меню Rhizomind в тексте
// заметки (CodeMirror) и полях ввода вместо меню браузерного движка (в нём
// «Обновить», «Сохранить как», «Печать» — обход решений 94/96 и риск потери
// ввода, инвариант 4).
//
// Пункты: «Вырезать», «Копировать», «Вставить», «Выделить всё». Буфер обмена
// — общий системный (решение 98). Вырезать и копировать — командой движка
// `copy`/`cut` по щелчку пользователя: так же движок делает Ctrl+C/Ctrl+X,
// читать буфер для этого не нужно. «Вставить» — текст буфера даёт команда
// ядра `clipboard_read_text`, и только по щелчку по пункту (не при открытии
// меню): разрешение движка ClipboardRead странице не выдаётся
// (docs/SECURITY.md). Вставка — обычная правка: одна отмена Ctrl+Z,
// автосохранение, не в режиме только чтения; длиннее предела размера
// редактируемого файла — уведомление, текст не вставляется.
import { selectAll } from '@codemirror/commands';
import { EditorView } from '@codemirror/view';
import type { ClipboardReadOutcome } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';
import type { MenuItem } from './menu.ts';

/** Место правки текста под правым щелчком. */
export interface TextTarget {
  readonly hasSelection: boolean;
  readonly readOnly: boolean;
  /** Вернуть фокус (и выделение) в поле или редактор. */
  focus(): void;
  /** Правый щелчок мышью вне выделения ставит курсор в точку щелчка. */
  placeCaret(x: number, y: number): void;
  selectAll(): void;
  /** Точка для меню, открытого с клавиатуры (Shift+F10, клавиша меню). */
  anchor(): { x: number; y: number };
  /**
   * Вставить текст вместо выделения как ввод пользователя; `false` — поле
   * или редактор стали только для чтения, текст не вставлен.
   */
  insertText(text: string): boolean;
}

/** Откуда «Вставить» берёт текст: окно хранилища — команда ядра, иначе пункт неактивен. */
export interface PasteSource {
  read(): Promise<ClipboardReadOutcome>;
  notify(text: string): void;
}

/** Поля, где правится текст (флажки, кнопки и ползунки — нет). */
const TEXT_INPUT_TYPES: ReadonlySet<string> = new Set(['text', 'search', 'url', 'email', 'tel', 'password', 'number']);

function inputTarget(field: HTMLInputElement | HTMLTextAreaElement): TextTarget | null {
  if (field instanceof HTMLInputElement && !TEXT_INPUT_TYPES.has(field.type)) return null;
  // У полей числа selectionStart всегда null.
  const start = field.selectionStart;
  const end = field.selectionEnd;
  return {
    hasSelection: start !== null && end !== null && start !== end,
    readOnly: field.readOnly || field.disabled,
    focus: () => field.focus(),
    placeCaret: () => undefined,
    selectAll: () => {
      field.focus();
      field.select();
    },
    anchor: () => {
      const box = field.getBoundingClientRect();
      return { x: box.left + 8, y: box.bottom };
    },
    insertText: (text) => {
      if (field.readOnly || field.disabled) return false;
      // В однострочном поле переносов нет — как при вставке Ctrl+V.
      const value = field instanceof HTMLInputElement ? text.replace(/\r?\n/g, ' ') : text.replace(/\r\n/g, '\n');
      field.focus();
      // insertText движка — ввод пользователя: событие input, отмена Ctrl+Z.
      try {
        if (document.execCommand('insertText', false, value)) return true;
      } catch {
        // Движок без команд правки — вставка ниже.
      }
      const from = field.selectionStart ?? field.value.length;
      const to = field.selectionEnd ?? from;
      field.setRangeText(value, from, to, 'end');
      field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste', data: value }));
      return true;
    },
  };
}

function editorTarget(view: EditorView): TextTarget {
  return {
    hasSelection: view.state.selection.ranges.some((range) => !range.empty),
    readOnly: view.state.readOnly,
    focus: () => view.focus(),
    placeCaret: (x, y) => {
      let pos: number | null = null;
      try {
        pos = view.posAtCoords({ x, y });
      } catch {
        // Без раскладки (скрытый редактор) — курсор остаётся.
      }
      if (pos === null) return;
      const inside = view.state.selection.ranges.some((range) => !range.empty && range.from <= pos && pos <= range.to);
      if (!inside) view.dispatch({ selection: { anchor: pos } });
    },
    selectAll: () => {
      view.focus();
      selectAll(view);
    },
    anchor: () => {
      let box: { left: number; bottom: number } | null;
      try {
        box = view.coordsAtPos(view.state.selection.main.head);
      } catch {
        box = null;
      }
      const fallback = view.contentDOM.getBoundingClientRect();
      return { x: box?.left ?? fallback.left, y: box?.bottom ?? fallback.top };
    },
    insertText: (text) => {
      // Состояние читается заново: пока шёл запрос, заметка могла стать только для чтения.
      if (view.state.readOnly) return false;
      view.focus();
      // Одна транзакция — одна отмена; правка пользователя — автосохранение.
      view.dispatch(view.state.replaceSelection(text), { userEvent: 'input.paste', scrollIntoView: true });
      return true;
    },
  };
}

/** Цель меню правки текста под элементом; `null` — там текст не правится. */
export function textTargetOf(node: EventTarget | null): TextTarget | null {
  const element = node instanceof Element ? node : node instanceof Node ? node.parentElement : null;
  if (!element) return null;
  const field = element.closest('input, textarea');
  if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) return inputTarget(field);
  const content = element.closest('.cm-content');
  const view = content instanceof HTMLElement ? EditorView.findFromDOM(content) : null;
  return view ? editorTarget(view) : null;
}

/** Команда правки движка над тем, где фокус (`copy`, `cut`). */
function edit(target: TextTarget, command: 'copy' | 'cut') {
  target.focus();
  document.execCommand(command);
}

/** «Вставить»: текст буфера от ядра — в поле или редактор. */
async function paste(target: TextTarget, source: PasteSource): Promise<void> {
  let result: ClipboardReadOutcome;
  try {
    result = await source.read();
  } catch (error) {
    source.notify(userMessage(error));
    return;
  }
  if (result.outcome === 'too_large') source.notify(t('textMenu.pasteTooLarge'));
  else if (result.outcome === 'text') target.insertText(result.text);
}

/**
 * Пункты меню для цели. Буфер обмена здесь не читается: только щелчок по
 * «Вставить» вызывает `source.read`.
 */
export function textMenuItems(target: TextTarget, source: PasteSource | null = null): MenuItem[] {
  return [
    {
      id: 'cut',
      label: t('textMenu.cut'),
      shortcut: 'Ctrl+X',
      disabled: !target.hasSelection || target.readOnly,
      run: () => edit(target, 'cut'),
    },
    {
      id: 'copy',
      label: t('textMenu.copy'),
      shortcut: 'Ctrl+C',
      disabled: !target.hasSelection,
      run: () => edit(target, 'copy'),
    },
    {
      id: 'paste',
      label: t('textMenu.paste'),
      shortcut: 'Ctrl+V',
      // Без команды ядра (окно управления) пункт неактивен; Ctrl+V работает.
      disabled: source === null || target.readOnly,
      run: () => {
        if (source) void paste(target, source);
      },
    },
    { id: 'select-all', label: t('textMenu.selectAll'), shortcut: 'Ctrl+A', run: () => target.selectAll() },
  ];
}
