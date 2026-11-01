import { describe, expect, it } from 'vitest';
import { LIMITS } from '../api/limits.ts';
import { checkChord, isTextEditingKey, type KeyLike } from '../settings/hotkeys.ts';
import { ENGINE_SHORTCUTS, isEngineShortcut } from './engineShortcuts.ts';

/** Нажатие по записи `Ctrl+Shift+KeyP` (key — как в латинской раскладке). */
function press(chord: string, extra: Partial<KeyLike> = {}): KeyLike {
  const parts = chord.split('+');
  const code = parts.at(-1) ?? '';
  const key = /^Key[A-Z]$/.test(code) ? code.slice(3).toLowerCase() : code;
  return {
    key,
    code,
    ctrlKey: parts.includes('Ctrl'),
    altKey: parts.includes('Alt'),
    shiftKey: parts.includes('Shift'),
    metaKey: false,
    ...extra,
  };
}

describe('сочетания браузерного движка (решение 94)', () => {
  it.each([
    // печать
    'Ctrl+KeyP',
    'Ctrl+Shift+KeyP',
    // поиск на странице
    'Ctrl+KeyF',
    'Ctrl+KeyG',
    'Ctrl+Shift+KeyG',
    'F3',
    'Shift+F3',
    // перезагрузка
    'Ctrl+KeyR',
    'Ctrl+Shift+KeyR',
    'F5',
    'Ctrl+F5',
    'Shift+F5',
    // исходный код, сохранение страницы, загрузки, история
    'Ctrl+KeyU',
    'Ctrl+KeyS',
    'Ctrl+KeyJ',
    'Ctrl+KeyH',
    // средства разработчика
    'F12',
    'Ctrl+Shift+KeyI',
    'Ctrl+Shift+KeyJ',
    'Ctrl+Shift+KeyC',
    // масштаб
    'Ctrl+Equal',
    'Ctrl+Shift+Equal',
    'Ctrl+Minus',
    'Ctrl+Digit0',
    'Ctrl+NumpadAdd',
    'Ctrl+NumpadSubtract',
    'Ctrl+Numpad0',
    // навигация движка
    'Alt+ArrowLeft',
    'Alt+ArrowRight',
    'Alt+Home',
    'BrowserBack',
    'BrowserForward',
    'BrowserRefresh',
    // окна и вкладки
    'Ctrl+KeyN',
    'Ctrl+Shift+KeyN',
    'Ctrl+KeyT',
    'Ctrl+Shift+KeyT',
    'Ctrl+KeyW',
    'Ctrl+F4',
    'Ctrl+Tab',
    'Ctrl+Shift+Tab',
    'Ctrl+Digit1',
    'Ctrl+Digit9',
    // адресная строка, закладки, меню, каретка, справка, полный экран
    'Ctrl+KeyL',
    'Alt+KeyD',
    'Ctrl+KeyD',
    'F7',
    'F1',
    'F11',
  ])('%s — сочетание движка', (chord) => {
    expect(isEngineShortcut(press(chord))).toBe(true);
  });

  it('по физической клавише: Ctrl+P в русской раскладке', () => {
    expect(isEngineShortcut(press('Ctrl+KeyP', { key: 'з' }))).toBe(true);
  });

  it('Cmd на macOS — как Ctrl', () => {
    expect(isEngineShortcut(press('KeyP', { metaKey: true }))).toBe(true);
  });

  it.each([
    // правка текста (решение 81) и системные сочетания
    'Ctrl+KeyC',
    'Ctrl+KeyV',
    'Ctrl+KeyX',
    'Ctrl+KeyZ',
    'Ctrl+KeyY',
    'Ctrl+KeyA',
    'Ctrl+Shift+KeyZ',
    'Ctrl+Shift+KeyV',
    'Ctrl+ArrowLeft',
    'Ctrl+Shift+ArrowRight',
    'Ctrl+Backspace',
    'Ctrl+Delete',
    'Ctrl+Shift+Delete',
    'Ctrl+Home',
    'Shift+Delete',
    'Shift+Insert',
    'Ctrl+Insert',
    'Tab',
    'Shift+Tab',
    'Escape',
    'Enter',
    'Alt+F4',
    'Alt+Space',
    'Shift+F10',
    'F2',
    'Delete',
    'Home',
    'PageDown',
    // AltGr в европейских раскладках приходит как Ctrl+Alt
    'Ctrl+Alt+KeyQ',
    'Ctrl+Alt+KeyE',
    // обычный ввод
    'KeyP',
    'Shift+KeyP',
    'Digit0',
  ])('%s — не сочетание движка', (chord) => {
    expect(isEngineShortcut(press(chord))).toBe(false);
  });

  it('при наборе через IME нажатие не трогается', () => {
    expect(isEngineShortcut(press('Ctrl+KeyP', { isComposing: true }))).toBe(false);
  });

  it('пустой code (служба текста Windows) — по key', () => {
    expect(isEngineShortcut(press('Ctrl+KeyP', { code: '', key: 'p' }))).toBe(true);
    expect(isEngineShortcut(press('F5', { code: '', key: 'F5' }))).toBe(true);
  });

  it('список не задевает правку текста и запрещённые для назначения сочетания', () => {
    for (const chord of ENGINE_SHORTCUTS) {
      expect(isTextEditingKey(press(chord)), chord).toBe(false);
      expect(LIMITS.hotkey_reserved, chord).not.toContain(chord);
    }
  });

  it('Ctrl+P, Ctrl+F и F12 свободны, но их можно назначить команде (решение 96)', () => {
    for (const chord of ['Ctrl+KeyP', 'Ctrl+KeyF', 'F12']) expect(checkChord(chord), chord).toBeNull();
  });
});
