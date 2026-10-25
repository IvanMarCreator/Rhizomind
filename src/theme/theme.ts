// Режим темы (С3.8): «Как в системе» (по умолчанию), «Светлая», «Тёмная».
// Тема окна — атрибут data-theme на <html>; значения переменных — tokens.css.
import type { Theme } from '../api/index.ts';

export type AppliedTheme = 'light' | 'dark';

/** Тёмная ли тема системы и подписка на её смену (`matchMedia`). */
export interface SystemScheme {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: () => void): void;
  removeEventListener(type: 'change', listener: () => void): void;
}

/** Куда ставится тема: обычно `document.documentElement`. */
export interface ThemeRoot {
  dataset: DOMStringMap;
}

export const DEFAULT_THEME: Theme = 'system';

export function resolveTheme(mode: Theme, systemDark: boolean): AppliedTheme {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

/** Тема системы; без `matchMedia` (jsdom в тестах) — светлая, без слежения. */
export function systemScheme(): SystemScheme {
  if (typeof window.matchMedia !== 'function') {
    return { matches: false, addEventListener: () => {}, removeEventListener: () => {} };
  }
  return window.matchMedia('(prefers-color-scheme: dark)');
}

/**
 * Ставит тему окна. В режиме «Как в системе» следит за сменой темы системы;
 * возвращает отписку (вызвать перед сменой режима).
 */
export function applyTheme(
  mode: Theme,
  root: ThemeRoot = document.documentElement,
  scheme: SystemScheme = systemScheme(),
): () => void {
  const update = () => {
    root.dataset.theme = resolveTheme(mode, scheme.matches);
  };
  update();
  if (mode !== 'system') return () => {};
  scheme.addEventListener('change', update);
  return () => scheme.removeEventListener('change', update);
}
