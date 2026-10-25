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
