// Набор расширений CodeMirror 6 для текста заметки (ADR 0001).
//
// Правило «байт в байт»: разделитель строк документа — только `\n`
// (`EditorState.lineSeparator`). Без него CodeMirror превращал бы одиночный
// `\r` и `\r\n` в перенос, и первая же правка меняла бы байты, которых
// пользователь не трогал. `\r\n` в тексте от ядра не бывает: ядро приводит
// его к `\n` и восстанавливает при записи (src-tauri/src/vault/text.rs).
//
// Проверка орфографии выключена (С3.3): проверка браузерного движка может
// обращаться к сети. Режим (живой предпросмотр или исходный текст, задача 2.5)
// переключается на ходу и только меняет оформление: документ тот же.
// Вики-ссылки (задача 2.6): оформление — в живом предпросмотре, переход по
// щелчку, автодополнение после `[[` и переход к заголовку — в обоих режимах;
// связь с окном — `WikiHost` (src/editor/wikilinks/host.ts).
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { Compartment, EditorState, type Extension } from '@codemirror/state';
import { EditorView, highlightSpecialChars, keymap, type KeyBinding } from '@codemirror/view';
import type { EditorMode } from '../api/index.ts';
import { t } from '../i18n/index.ts';
import { livePreview, sourceLinks } from './live-preview/index.ts';
import { wikilinkCompletion } from './wikilinks/autocomplete.ts';
import { wikiHost, type WikiHost } from './wikilinks/host.ts';
import { revealPlugin } from './wikilinks/reveal.ts';
import './wikilinks/wikilinks.css';

/** Переносы из буфера обмена и перетаскивания — в `\n`, как в остальном тексте. */
export function normalizePastedText(text: string): string {
  return text.replace(/\r\n?/g, '\n');
}

/** Alt+← и Alt+→ — история переходов окна (src/core/history), не редактор. */
const WINDOW_KEYS = new Set(['Alt-ArrowLeft', 'Alt-ArrowRight']);

export const editorKeymap: readonly KeyBinding[] = [
  ...defaultKeymap.filter((binding) => !binding.key || !WINDOW_KEYS.has(binding.key)),
  ...historyKeymap,
];

/** Только чтение переключается на ходу: заметку удалили, пока она открыта. */
const lock = new Compartment();

function lockExtension(readOnly: boolean): Extension {
  return [EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)];
}

/** Запретить или разрешить правку в открытом редакторе. */
export function setReadOnly(view: EditorView, readOnly: boolean): void {
  if (view.state.readOnly === readOnly) return;
  view.dispatch({ effects: lock.reconfigure(lockExtension(readOnly)) });
}

/** Режим редактора: оформление живого предпросмотра или исходный текст. */
const modeSlot = new Compartment();

function modeExtension(mode: EditorMode, onOpenLink: ((url: string) => void) | undefined): Extension {
  return mode === 'live_preview' ? livePreview({ onOpenLink }) : sourceLinks({ onOpenLink });
}

/** Переключить режим в открытом редакторе. Текст и выделение не меняются. */
export function setMode(view: EditorView, mode: EditorMode, onOpenLink?: (url: string) => void): void {
  view.dispatch({ effects: modeSlot.reconfigure(modeExtension(mode, onOpenLink)) });
}

export interface EditorOptions {
  readOnly?: boolean;
  /** Режим редактора; по умолчанию — исходный текст. */
  mode?: EditorMode;
  /** Щелчок по внешней ссылке: открыть через ядро. */
  onOpenLink?: (url: string) => void;
  /** Вики-ссылки: разрешение, переход, подсказки (без него — только оформление). */
  wiki?: WikiHost;
  /** Документ изменился правкой пользователя. */
  onChange?: (text: string) => void;
}

export function editorExtensions({ readOnly = false, mode = 'source', onOpenLink, wiki, onChange }: EditorOptions = {}): Extension[] {
  return [
    wiki ? wikiHost.of(wiki) : [],
    revealPlugin,
    wikilinkCompletion(),
    EditorState.lineSeparator.of('\n'),
    lock.of(lockExtension(readOnly)),
    modeSlot.of(modeExtension(mode, onOpenLink)),
    history(),
    keymap.of(editorKeymap),
    // Одиночный `\r`, NUL и другие невидимые символы видны значком.
    highlightSpecialChars(),
    EditorView.lineWrapping,
    EditorView.clipboardInputFilter.of(normalizePastedText),
    EditorView.contentAttributes.of({
      spellcheck: 'false',
      autocorrect: 'off',
      autocapitalize: 'off',
      translate: 'no',
      'aria-label': t('editor.label'),
    }),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onChange?.(update.state.doc.toString());
    }),
  ];
}

export function createEditorState(text: string, options: EditorOptions = {}): EditorState {
  return EditorState.create({ doc: text, extensions: editorExtensions(options) });
}
