// Горячая клавиша быстрого перехода: Ctrl+O (С3.5). По физической клавише
// (`code`), чтобы работала и в русской раскладке (там `key` — «щ»).
export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  isComposing: boolean;
}

export function isQuickSwitcherKey(event: KeyLike): boolean {
  if (event.isComposing || !event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return false;
  return event.code === 'KeyO' || event.key.toLowerCase() === 'o';
}

/**
 * Tab (с модификаторами или без). По `key` и по физической клавише `code`:
 * когда ввод идёт через службу текста Windows (подсказки при вводе, IME),
 * движок присылает `key` = «Process», а `code` остаётся «Tab».
 */
export function isTabKey(event: Pick<KeyLike, 'key' | 'code'>): boolean {
  return event.key === 'Tab' || event.code === 'Tab';
}
