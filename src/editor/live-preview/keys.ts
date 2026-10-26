// Горячая клавиша режима редактора: Ctrl+E (С3.3, решение 72; сочетание
// предварительное). По физической клавише (`code`), чтобы работала и в
// русской раскладке (там `key` — «у»). Редактируемые назначения — задача 2.8б:
// команда `editor.toggle-mode`.
import type { KeyLike } from '../../quick-switcher/keys.ts';

/** Имя команды в реестре горячих клавиш (задача 2.8б). */
export const TOGGLE_MODE_COMMAND = 'editor.toggle-mode';

export function isEditorModeKey(event: KeyLike): boolean {
  if (event.isComposing || !event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return false;
  return event.code === 'KeyE' || event.key.toLowerCase() === 'e';
}
