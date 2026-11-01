// Сочетания браузерного движка (решение владельца 94): не назначенные командам
// Rhizomind, они ничего не делают внутри окна — ни печати (Ctrl+P), ни поиска
// движка, ни перезагрузки, ни сохранения страницы, ни средств разработчика,
// ни масштаба.
//
// Главный рубеж — настройка движка: в WebView2 выключены «ускорители
// браузера» (`AreBrowserAcceleratorKeysEnabled = false`) и масштаб
// (`IsZoomControlEnabled = false`), src-tauri/src/security/windows.rs. Нажатие
// при этом доходит до страницы, и команды из реестра горячих клавиш работают.
// Здесь — запасной рубеж для других движков (WebKitGTK) и новых сочетаний
// движка: окно гасит действие по умолчанию у сочетаний из списка, если их не
// обработали ни команда окна, ни элемент (редактор, поле) — оболочка проверяет
// на всплытии, последней (src/shell/Shell.svelte).
//
// Клавиши правки текста (решение 81), Tab, Esc, Alt+F4 и AltGr (Ctrl+Alt) в
// список не входят: их браузер не перехватывает, а странице они нужны.
import { chordFromEvent, type KeyLike } from '../settings/hotkeys.ts';

/**
 * Сочетания движка: Chromium/Edge (WebView2) и WebKitGTK. Microsoft не
 * публикует полный список ускорителей WebView2 — здесь сочетания Edge,
 * которые действуют в странице или в окне браузера. Запись — как в реестре
 * горячих клавиш: `Ctrl+Alt+Shift+<code>`.
 */
export const ENGINE_SHORTCUTS: readonly string[] = [
  // печать
  'Ctrl+KeyP',
  'Ctrl+Shift+KeyP',
  // поиск на странице
  'Ctrl+KeyF',
  'Ctrl+KeyG',
  'Ctrl+Shift+KeyG',
  'F3',
  'Shift+F3',
  // перезагрузка и остановка
  'Ctrl+KeyR',
  'Ctrl+Shift+KeyR',
  'F5',
  'Ctrl+F5',
  'Shift+F5',
  'BrowserRefresh',
  'BrowserStop',
  // исходный код, сохранение страницы, снимок страницы, загрузки, история
  'Ctrl+KeyU',
  'Ctrl+KeyS',
  'Ctrl+Shift+KeyS',
  'Ctrl+KeyJ',
  'Ctrl+KeyH',
  // средства разработчика, диспетчер задач браузера
  'F12',
  'Ctrl+Shift+KeyI',
  'Ctrl+Shift+KeyJ',
  'Ctrl+Shift+KeyC',
  'Shift+Escape',
  // масштаб
  'Ctrl+Equal',
  'Ctrl+Shift+Equal',
  'Ctrl+Minus',
  'Ctrl+Shift+Minus',
  'Ctrl+Digit0',
  'Ctrl+NumpadAdd',
  'Ctrl+NumpadSubtract',
  'Ctrl+Numpad0',
  // навигация движка (у Rhizomind «Назад/Вперёд» — команды реестра)
  'Alt+ArrowLeft',
  'Alt+ArrowRight',
  'Alt+Home',
  'BrowserBack',
  'BrowserForward',
  'BrowserHome',
  'BrowserSearch',
  'BrowserFavorites',
  // окна и вкладки
  'Ctrl+KeyN',
  'Ctrl+Shift+KeyN',
  'Ctrl+KeyT',
  'Ctrl+Shift+KeyT',
  'Ctrl+KeyW',
  'Ctrl+Shift+KeyW',
  'Ctrl+F4',
  'Ctrl+Tab',
  'Ctrl+Shift+Tab',
  'Ctrl+PageUp',
  'Ctrl+PageDown',
  'Ctrl+Shift+KeyK',
  'Ctrl+Shift+KeyA',
  'Ctrl+Digit1',
  'Ctrl+Digit2',
  'Ctrl+Digit3',
  'Ctrl+Digit4',
  'Ctrl+Digit5',
  'Ctrl+Digit6',
  'Ctrl+Digit7',
  'Ctrl+Digit8',
  'Ctrl+Digit9',
  // открыть файл, адресная строка и поиск в ней, закладки, профиль
  'Ctrl+KeyO',
  'Ctrl+KeyL',
  'Alt+KeyD',
  'F6',
  'Shift+F6',
  'Ctrl+KeyE',
  'Ctrl+KeyK',
  'Ctrl+KeyD',
  'Ctrl+Shift+KeyD',
  'Ctrl+Shift+KeyB',
  'Ctrl+Shift+KeyO',
  'Ctrl+Shift+KeyM',
  'Ctrl+KeyM',
  // меню браузера, отзыв, чтение вслух
  'Alt+KeyF',
  'Alt+KeyE',
  'F10',
  'Alt+Shift+KeyI',
  'Ctrl+Shift+KeyU',
  // режим каретки, справка, полный экран
  'F7',
  'F1',
  'F11',
];

const SHORTCUTS: ReadonlySet<string> = new Set(ENGINE_SHORTCUTS);

/** Сочетание по физической клавише, в том числе вне реестра (цифровой блок, клавиши браузера). */
function engineChord(event: KeyLike): string | null {
  if (event.isComposing) return null;
  if (!event.code) return chordFromEvent(event);
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  parts.push(event.code);
  return parts.join('+');
}

/** Нажатие — сочетание браузерного движка из списка. */
export function isEngineShortcut(event: KeyLike): boolean {
  const chord = engineChord(event);
  return chord !== null && SHORTCUTS.has(chord);
}

/**
 * Масштаб движка колесом мыши с Ctrl (и жестом щипка на сенсорной панели —
 * он приходит так же) гасится. Возвращает снятие (для тестов).
 */
export function blockEngineZoom(doc: Document): () => void {
  const onWheel = (event: WheelEvent) => {
    if (event.ctrlKey || event.metaKey) event.preventDefault();
  };
  const options = { capture: true, passive: false } as const;
  doc.addEventListener('wheel', onWheel, options);
  return () => doc.removeEventListener('wheel', onWheel, options);
}

/**
 * Меню браузерного движка (решение 97: в нём «Обновить», «Сохранить как»,
 * «Печать») не открывается нигде в окне. Главный рубеж на Windows —
 * `AreDefaultContextMenusEnabled = false` (src-tauri/src/security/windows.rs),
 * здесь — запасной и единственный на Linux. Гасится только действие по
 * умолчанию: свои меню (дерево, правка текста) событие получают.
 * Возвращает снятие (для тестов).
 */
export function blockEngineContextMenu(doc: Document): () => void {
  const onContextMenu = (event: Event) => event.preventDefault();
  doc.addEventListener('contextmenu', onContextMenu, true);
  return () => doc.removeEventListener('contextmenu', onContextMenu, true);
}
