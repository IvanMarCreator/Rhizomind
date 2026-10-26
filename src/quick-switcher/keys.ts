// Клавиши окна быстрого перехода. Само сочетание открытия (Ctrl+O) — в
// реестре горячих клавиш (src/settings/hotkeys.ts).
export interface KeyLike {
  key: string;
  code: string;
}

/**
 * Tab (с модификаторами или без). По `key` и по физической клавише `code`:
 * WebKitGTK у Shift+Tab присылает не «Tab» в `key` (GTK: ISO_Left_Tab), а при
 * вводе через службу текста Windows (подсказки при вводе, IME) `key` —
 * «Process»; `code` в обоих случаях «Tab».
 */
export function isTabKey(event: Pick<KeyLike, 'key' | 'code'>): boolean {
  return event.key === 'Tab' || event.code === 'Tab';
}
