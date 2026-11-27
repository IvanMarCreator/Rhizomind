import { EditorSelection, StateEffect, StateField } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { AttachmentOutcome, VaultPath } from '../api/types.ts';
import { t } from '../i18n/index.ts';

/** Вставка одного пути из «Копировать как путь» без обращения к ОС. */
export function pastedExternalPath(text: string): boolean {
  const value = text.trim().replace(/^"(.*)"$/u, '$1');
  return !/[\r\n]/u.test(value) && /^(?:[A-Za-z]:[\\/]|\\\\|\/|file:)/iu.test(value);
}

export function pasteContainsAttachment(types: readonly string[]): boolean {
  return types.some((type) => type === 'Files' || type === 'text/uri-list' || type.startsWith('image/'));
}

export function finishAttachment(result: Exclude<AttachmentOutcome, { outcome: 'choose_name' }>, insert: (text: string) => boolean, notify: (message: string) => void): void {
  switch (result.outcome) {
    case 'created':
      if (result.target && insert(`![[${result.target}]]`)) return;
      notify(result.path ? t('attachments.saved', { path: result.path.join('/') }) : t('attachments.moved'));
      return;
    case 'text': insert(result.text); return;
    case 'external_path': notify(t('attachments.external')); return;
    case 'too_large': notify(t('attachments.tooLarge')); return;
    case 'not_focused': notify(t('attachments.focus')); return;
    case 'empty': notify(t('attachments.empty')); return;
  }
}

const track = StateEffect.define<{ id: symbol; range: EditorSelection | null }>();
const positions = StateField.define<ReadonlyMap<symbol, EditorSelection>>({
  create: () => new Map(),
  update(previous, transaction) {
    if (!previous.size && !transaction.effects.some((effect) => effect.is(track))) return previous;
    const next = new Map(previous);
    if (transaction.docChanged) for (const [id, range] of next) next.set(id, range.map(transaction.changes));
    for (const effect of transaction.effects) {
      if (!effect.is(track)) continue;
      if (effect.value.range) next.set(effect.value.id, effect.value.range);
      else next.delete(effect.value.id);
    }
    return next;
  },
});

export interface AttachmentAnchor {
  note: VaultPath;
  insert(text: string): boolean;
  pasteText(text: string): void;
  release(): void;
}

export function captureAttachment(view: EditorView, note: VaultPath): AttachmentAnchor | null {
  if (!view.dom.isConnected || view.state.readOnly) return null;
  if (!view.state.field(positions, false)) view.dispatch({ effects: StateEffect.appendConfig.of(positions) });
  const id = Symbol();
  view.dispatch({ effects: track.of({ id, range: view.state.selection }) });
  return {
    note: [...note],
    insert(text) {
      if (!view.dom.isConnected || view.state.readOnly) return false;
      const range = view.state.field(positions).get(id)?.main;
      if (!range) return false;
      view.dispatch({ changes: { from: range.from, to: range.to, insert: text }, selection: EditorSelection.cursor(range.from + text.length), userEvent: 'input.paste' });
      return true;
    },
    pasteText(text) {
      if (!view.dom.isConnected || view.state.readOnly) return;
      const range = view.state.field(positions).get(id);
      if (!range) return;
      if (!view.state.selection.eq(range)) view.dispatch({ selection: range });
      const data = new DataTransfer();
      data.setData('text/plain', text);
      view.contentDOM.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    },
    release() {
      if (view.dom.isConnected) view.dispatch({ effects: track.of({ id, range: null }) });
    },
  };
}
