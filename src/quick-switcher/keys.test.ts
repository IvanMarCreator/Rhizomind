import { describe, expect, it } from 'vitest';
import { isQuickSwitcherKey } from './keys.ts';

const key = (init: {
  key: string;
  code?: string;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  metaKey?: boolean;
  isComposing?: boolean;
}) => ({
  key: init.key,
  code: init.code ?? '',
  ctrlKey: init.ctrlKey ?? false,
  altKey: init.altKey ?? false,
  shiftKey: init.shiftKey ?? false,
  metaKey: init.metaKey ?? false,
  isComposing: init.isComposing ?? false,
});

describe('Ctrl+O', () => {
  it('Ctrl+O в английской и русской раскладке', () => {
    expect(isQuickSwitcherKey(key({ key: 'o', code: 'KeyO', ctrlKey: true }))).toBe(true);
    expect(isQuickSwitcherKey(key({ key: 'щ', code: 'KeyO', ctrlKey: true }))).toBe(true);
    expect(isQuickSwitcherKey(key({ key: 'O', ctrlKey: true }))).toBe(true);
  });

  it('без Ctrl или с другими модификаторами — нет', () => {
    expect(isQuickSwitcherKey(key({ key: 'o', code: 'KeyO' }))).toBe(false);
    expect(isQuickSwitcherKey(key({ key: 'O', code: 'KeyO', ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isQuickSwitcherKey(key({ key: 'o', code: 'KeyO', ctrlKey: true, altKey: true }))).toBe(false);
    expect(isQuickSwitcherKey(key({ key: 'o', code: 'KeyO', ctrlKey: true, metaKey: true }))).toBe(false);
    expect(isQuickSwitcherKey(key({ key: 'p', code: 'KeyP', ctrlKey: true }))).toBe(false);
  });

  it('во время набора через IME — нет', () => {
    expect(isQuickSwitcherKey(key({ key: 'o', code: 'KeyO', ctrlKey: true, isComposing: true }))).toBe(false);
  });
});
