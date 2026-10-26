// Ширина текста и размер шрифта заметки (настройки хранилища): переменные
// темы на <html>, их читают колонка заметки и редактор. Применяются сразу,
// без перезапуска окна.
import type { Settings } from '../api/index.ts';

/** Куда ставятся переменные: обычно `document.documentElement`. */
export interface StyleRoot {
  style: Pick<CSSStyleDeclaration, 'setProperty' | 'removeProperty'>;
}

export function applyTextSettings(
  settings: Pick<Settings, 'text_width' | 'font_size'>,
  root: StyleRoot = document.documentElement,
): void {
  // «Колонка» — ширина из tokens.css; «во всю ширину» — без ограничения.
  if (settings.text_width === 'full') root.style.setProperty('--note-column-width', 'none');
  else root.style.removeProperty('--note-column-width');
  root.style.setProperty('--note-font-size', `${settings.font_size}px`);
}
