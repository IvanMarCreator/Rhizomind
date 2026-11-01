// Парная квадратная скобка (решение владельца 95, задача 2.18) — в обоих
// режимах редактора:
// - `[` вставляет `[]`, курсор между скобками; второй `[` в пустой паре даёт
//   `[[]]` и открывает список подсказок вики-ссылок (вставка — ввод
//   `input.type`, как у набранного символа). Пара — только перед концом
//   строки, пробелом, закрывающей скобкой или знаком препинания (как
//   closeBrackets в CodeMirror и Obsidian): перед словом `[` — одна, чтобы
//   можно было взять в скобки уже написанный текст; после `\` — тоже одна
//   (экранированная скобка — текст);
// - `[` при выделении оборачивает его в `[…]`, выделение остаётся внутри;
// - `]` перед `]`, вставленной парой, переводит курсор за неё (так же для
//   `]]` вики-ссылки и `[текст]` ссылки Markdown); чужая `]` не
//   перескакивается. Вставленные скобки помнятся, пока курсор на их строке;
// - Backspace между пустыми `[]` удаляет обе; в `[[]]` — внутреннюю пару
//   (второй Backspace — внешнюю), как в Obsidian;
// - пробел в пустой паре сразу после маркера пункта (`- `, `* `, `+ `, `1. `,
//   `1) `, в том числе в цитате) даёт готовую задачу `- [ ] ` с курсором после
//   неё — можно сразу писать текст задачи; в остальных местах пробел обычный;
// - в коде (в строке и в блоке), в блоке свойств и во время набора через IME
//   правила не действуют.
// Каждая правка — одна транзакция: Ctrl+Z отменяет пару одним шагом.
// Своё небольшое расширение вместо closeBrackets из @codemirror/autocomplete:
// без новой зависимости (решение 75).
import {
  EditorSelection,
  Prec,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
  type TransactionSpec,
} from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { inCodeAt } from './live-preview/index.ts';

/** Перед чем `[` вставляет пару: конец строки, пробел, закрывающая скобка, знак препинания. */
const PAIR_BEFORE = /^(?:$|[\s)\]}.,;:!?])/;

/** Строка до `[` — маркер пункта списка (возможно, в цитате) и пробелы после него. */
const LIST_MARKER = /^(?:[ \t]{0,3}> ?)*[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]+$/;

/** Позиции (в новом тексте) `]`, вставленных парой. */
const addClosing = StateEffect.define<readonly number[]>();

/**
 * Вставленные парой `]` — позиции символов. Скобка забывается, если её
 * удалили или заменили, и когда курсор ушёл с её строки.
 */
const closingField = StateField.define<readonly number[]>({
  create: () => [],
  update(value, tr) {
    let next: number[] = [];
    for (const pos of value) {
      if (!tr.docChanged) {
        next.push(pos);
        continue;
      }
      // Удалена или заменена сама скобка — забыть; вставка рядом её не задевает.
      let touched = false;
      tr.changes.iterChangedRanges((fromA, toA) => {
        if (fromA <= pos && toA > pos) touched = true;
      });
      if (!touched) next.push(tr.changes.mapPos(pos, 1));
    }
    for (const effect of tr.effects) if (effect.is(addClosing)) next.push(...effect.value);
    if (next.length === 0) return value.length === 0 ? value : [];
    const line = tr.state.doc.lineAt(tr.state.selection.main.head);
    next = next.filter((pos) => pos >= line.from && pos < line.to && tr.state.doc.sliceString(pos, pos + 1) === ']');
    return next;
  },
});

function isClosing(state: EditorState, pos: number): boolean {
  return state.field(closingField, false)?.includes(pos) ?? false;
}

/** Ввод `[`: пара, обёртка выделения или `null` — обычный ввод символа. */
export function openBracket(state: EditorState): TransactionSpec | null {
  if (state.readOnly) return null;
  for (const range of state.selection.ranges) {
    if (inCodeAt(state, range.from)) return null;
    if (!range.empty) continue;
    const line = state.doc.lineAt(range.head);
    const after = line.text.slice(range.head - line.from);
    const before = range.head > line.from ? line.text[range.head - line.from - 1] : '';
    if (!PAIR_BEFORE.test(after) || before === '\\') return null;
  }
  const spec = state.changeByRange((range) => {
    if (range.empty) {
      return { changes: { from: range.head, insert: '[]' }, range: EditorSelection.cursor(range.head + 1) };
    }
    return {
      changes: [
        { from: range.from, insert: '[' },
        { from: range.to, insert: ']' },
      ],
      range: EditorSelection.range(range.anchor + 1, range.head + 1),
    };
  });
  // Позиции `]` — в новом тексте: по курсорам итогового выделения.
  const positions = spec.selection.ranges.filter((range) => range.empty).map((range) => range.head);
  return { ...spec, effects: addClosing.of(positions), userEvent: 'input.type', scrollIntoView: true };
}

/** Ввод `]` перед `]`, вставленной парой: курсор за неё. `null` — обычный ввод. */
export function closeBracket(state: EditorState): TransactionSpec | null {
  if (state.readOnly) return null;
  const { ranges } = state.selection;
  if (!ranges.every((range) => range.empty && isClosing(state, range.head))) return null;
  return {
    selection: EditorSelection.create(
      ranges.map((range) => EditorSelection.cursor(range.head + 1)),
      state.selection.mainIndex,
    ),
    userEvent: 'input.type',
    scrollIntoView: true,
  };
}

/** Backspace между пустыми `[]`: удалить обе. `null` — обычный Backspace. */
export function deletePair(state: EditorState): TransactionSpec | null {
  if (state.readOnly) return null;
  const { ranges } = state.selection;
  const ok = ranges.every(
    (range) =>
      range.empty &&
      state.doc.sliceString(range.head - 1, range.head + 1) === '[]' &&
      !inCodeAt(state, range.head - 1),
  );
  if (!ok) return null;
  return {
    ...state.changeByRange((range) => ({
      changes: { from: range.head - 1, to: range.head + 1 },
      range: EditorSelection.cursor(range.head - 1),
    })),
    userEvent: 'delete.backward',
    scrollIntoView: true,
  };
}

/** Пробел в пустой `[]` после маркера пункта: задача `- [ ] `, курсор после неё. */
export function taskSpace(state: EditorState): TransactionSpec | null {
  if (state.readOnly || state.selection.ranges.length !== 1) return null;
  const range = state.selection.main;
  if (!range.empty) return null;
  const { head } = range;
  if (state.doc.sliceString(head - 1, head + 1) !== '[]') return null;
  const line = state.doc.lineAt(head);
  if (!LIST_MARKER.test(line.text.slice(0, head - 1 - line.from))) return null;
  if (inCodeAt(state, head - 1)) return null;
  const next = state.doc.sliceString(head + 1, head + 2);
  const spaced = next === ' ' || next === '\t';
  const changes = spaced ? [{ from: head, insert: ' ' }] : [{ from: head, insert: ' ' }, { from: head + 1, insert: ' ' }];
  return { changes, selection: EditorSelection.cursor(head + 3), userEvent: 'input.type', scrollIntoView: true };
}

/** Идёт набор через IME (в том числе только что начатый): не вмешиваться. */
const composing = (view: EditorView): boolean => view.composing || view.compositionStarted;

const RULES: Record<string, (state: EditorState) => TransactionSpec | null> = {
  '[': openBracket,
  ']': closeBracket,
  ' ': taskSpace,
};

/** Расширение: ввод `[`, `]`, пробела и Backspace (важнее сочетаний по умолчанию). */
export function bracketPairs(): Extension {
  return [
    closingField,
    EditorView.inputHandler.of((view, from, to, text) => {
      const rule = RULES[text];
      const { main } = view.state.selection;
      if (!rule || composing(view) || from !== main.from || to !== main.to) return false;
      const spec = rule(view.state);
      if (!spec) return false;
      view.dispatch(spec);
      return true;
    }),
    Prec.high(
      keymap.of([
        {
          key: 'Backspace',
          run: (view) => {
            if (composing(view)) return false;
            const spec = deletePair(view.state);
            if (!spec) return false;
            view.dispatch(spec);
            return true;
          },
        },
      ]),
    ),
  ];
}
