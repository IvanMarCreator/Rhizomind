// Пункт своего контекстного меню (src/shell/ContextMenu.svelte, решение 97).
export interface MenuItem {
  id: string;
  label: string;
  /** Клавиша-подсказка справа (F2, Delete, Ctrl+C). */
  shortcut?: string | undefined;
  /** Неактивный пункт: серый, не выбирается. */
  disabled?: boolean;
  run: () => void;
}
