// Меню правки текста (решение 97): своё контекстное меню Rhizomind в тексте
// заметки (CodeMirror) и полях ввода вместо меню браузерного движка (в нём
// «Обновить», «Сохранить как», «Печать» — обход решений 94/96 и риск потери
// ввода, инвариант 4).
//
// Пункты: «Вырезать», «Копировать», «Вставить», «Выделить всё». Вырезать и
// копировать — командой движка `copy`/`cut` по щелчку пользователя: читать
// буфер обмена для этого не нужно, новых прав нет. «Вставить» — пока
// неактивный пункт с подсказкой Ctrl+V: программное чтение буфера требует
// разрешения ClipboardRead, а все разрешения страницы отклоняются
// (docs/SECURITY.md); Ctrl+V с клавиатуры — встроенная вставка движка.
import { selectAll } from '@codemirror/commands';
import { EditorView } from '@codemirror/view';
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

/** Пункты меню для цели. */
export function textMenuItems(target: TextTarget): MenuItem[] {
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
      // Чтение буфера обмена страницей запрещено (docs/SECURITY.md).
      disabled: true,
      run: () => undefined,
    },
    { id: 'select-all', label: t('textMenu.selectAll'), shortcut: 'Ctrl+A', run: () => target.selectAll() },
  ];
}
