// Сочетания клавиш дерева. Ctrl+Z — по физической клавише (`code`), чтобы
// работать и в русской раскладке, где `key` — «я».

export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** Ctrl+Z (Cmd+Z на macOS) без Shift и Alt. */
export function isUndo(event: KeyLike): boolean {
  return (event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.code === 'KeyZ';
}
