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
import { Compartment, EditorState, Transaction, type Extension } from '@codemirror/state';
import { EditorView, highlightSpecialChars, keymap, type KeyBinding } from '@codemirror/view';
import type { EditorMode } from '../api/index.ts';
import { t } from '../i18n/index.ts';
import { hugeLines } from './huge-lines.ts';
import { blockTracking, livePreview, sourceLinks } from './live-preview/index.ts';
import { wikilinkCompletion } from './wikilinks/autocomplete.ts';
import { wikiHost, type WikiHost } from './wikilinks/host.ts';
import { revealPlugin } from './wikilinks/reveal.ts';
import './live-preview/live-preview.css';
import './wikilinks/wikilinks.css';

/** Переносы из буфера обмена и перетаскивания — в `\n`, как в остальном тексте. */
export function normalizePastedText(text: string): string {
  return text.replace(/\r\n?/g, '\n');
}

/**
 * Сочетания редактора. Команды окна из реестра горячих клавиш (Alt+←/→ и
 * др.) важнее их (решение 80): окно перехватывает такие нажатия раньше
 * редактора (Shell.svelte, фаза перехвата); освобождённое сочетание снова
 * работает здесь.
 */
export const editorKeymap: readonly KeyBinding[] = [...defaultKeymap, ...historyKeymap];

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

const isHighSurrogate = (code: number) => code >= 0xd800 && code <= 0xdbff;
const isLowSurrogate = (code: number) => code >= 0xdc00 && code <= 0xdfff;

/**
 * Текст пришёл с диска (заметку изменили снаружи, задача 2.9): документ
 * заменяется одной правкой — только отличающаяся середина (общие начало и
 * конец не трогаются), поэтому курсор и выделение переносятся по правке и
 * остаются на своём месте, если их часть текста не менялась (С3.6). Правка
 * не попадает в историю отмены и не считается вводом пользователя
 * (`Transaction.remote`): сессия уже знает этот текст.
 */
export function replaceDocument(view: EditorView, text: string): void {
  const doc = view.state.doc.toString();
  if (doc === text) return;
  const max = Math.min(doc.length, text.length);
  let start = 0;
  while (start < max && doc.charCodeAt(start) === text.charCodeAt(start)) start += 1;
  let end = 0;
  while (end < max - start && doc.charCodeAt(doc.length - 1 - end) === text.charCodeAt(text.length - 1 - end)) end += 1;
  // Граница правки не разрезает суррогатную пару (эмодзи и др.).
  if (start > 0 && isHighSurrogate(doc.charCodeAt(start - 1))) start -= 1;
  if (end > 0 && isLowSurrogate(doc.charCodeAt(doc.length - end))) end -= 1;
  view.dispatch({
    changes: { from: start, to: doc.length - end, insert: text.slice(start, text.length - end) },
    annotations: [Transaction.addToHistory.of(false), Transaction.remote.of(true)],
  });
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
    // Блоки кода — в обоих режимах: автодополнение вики-ссылок не открывается в коде (2.6а).
    blockTracking(),
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
    hugeLines,
    EditorView.clipboardInputFilter.of(normalizePastedText),
    EditorView.contentAttributes.of({
      spellcheck: 'false',
      autocorrect: 'off',
      autocapitalize: 'off',
      translate: 'no',
      'aria-label': t('editor.label'),
    }),
    EditorView.updateListener.of((update) => {
      // Текст с диска (replaceDocument) — не правка пользователя.
      const fromDisk = update.transactions.every((tr) => !tr.docChanged || tr.annotation(Transaction.remote) === true);
      if (update.docChanged && !fromDisk) onChange?.(update.state.doc.toString());
    }),
  ];
}

export function createEditorState(text: string, options: EditorOptions = {}): EditorState {
  return EditorState.create({ doc: text, extensions: editorExtensions(options) });
}
