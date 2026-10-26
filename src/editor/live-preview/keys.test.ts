import { describe, expect, it } from 'vitest';
import { isEditorModeKey } from './keys.ts';

const key = (over: Partial<Parameters<typeof isEditorModeKey>[0]>) => ({
  key: 'e',
  code: 'KeyE',
  ctrlKey: true,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  isComposing: false,
  ...over,
});

describe('Ctrl+E — режим редактора', () => {
  it('латинская и русская раскладка (по физической клавише)', () => {
    expect(isEditorModeKey(key({}))).toBe(true);
    expect(isEditorModeKey(key({ key: 'у' }))).toBe(true);
  });

  it('другие модификаторы, без Ctrl и во время ввода IME — нет', () => {
    expect(isEditorModeKey(key({ ctrlKey: false }))).toBe(false);
    expect(isEditorModeKey(key({ shiftKey: true }))).toBe(false);
    expect(isEditorModeKey(key({ altKey: true }))).toBe(false);
    expect(isEditorModeKey(key({ isComposing: true }))).toBe(false);
    expect(isEditorModeKey(key({ key: 'o', code: 'KeyO' }))).toBe(false);
  });
});
