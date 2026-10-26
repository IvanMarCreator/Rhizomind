// Клавиатура в выпадающих меню хранилищ (переключатель, меню «…» в окне
// управления): стрелки, Home/End ходят по пунктам по кругу, Esc и Tab
// закрывают меню.

/** Пункты меню, по которым ходит фокус. */
export function menuItems(menu: HTMLElement | undefined): HTMLElement[] {
  return menu ? [...menu.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]')] : [];
}

/** Фокус на пункт `index` (по кругу: -1 — последний). */
export function focusMenuItem(menu: HTMLElement | undefined, index: number): void {
  const items = menuItems(menu);
  if (items.length === 0) return;
  items[((index % items.length) + items.length) % items.length]?.focus();
}

/** Обработка клавиши в меню; `onClose` — Esc и Tab. */
export function onMenuKeydown(event: KeyboardEvent, menu: HTMLElement | undefined, onClose: () => void): void {
  const current = menuItems(menu).indexOf(document.activeElement as HTMLElement);
  switch (event.key) {
    case 'Escape':
    case 'Tab':
      event.preventDefault();
      event.stopPropagation();
      onClose();
      break;
    case 'ArrowDown':
      event.preventDefault();
      focusMenuItem(menu, current + 1);
      break;
    case 'ArrowUp':
      event.preventDefault();
      focusMenuItem(menu, current < 0 ? -1 : current - 1);
      break;
    case 'Home':
      event.preventDefault();
      focusMenuItem(menu, 0);
      break;
    case 'End':
      event.preventDefault();
      focusMenuItem(menu, -1);
      break;
  }
}

/**
 * Закрытие меню щелчком мимо него и мимо `anchor` (кнопки, открывшей меню:
 * её щелчок сам переключает меню). Возвращает отписку.
 */
export function closeOnOutsidePointer(
  menu: () => HTMLElement | undefined,
  anchor: () => HTMLElement | undefined,
  onClose: () => void,
): () => void {
  const onPointerDown = (event: PointerEvent) => {
    const target = event.target as Node;
    if (menu()?.contains(target) || anchor()?.contains(target)) return;
    onClose();
  };
  window.addEventListener('pointerdown', onPointerDown, true);
  return () => window.removeEventListener('pointerdown', onPointerDown, true);
}
