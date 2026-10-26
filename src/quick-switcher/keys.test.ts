import { describe, expect, it } from 'vitest';
import { isTabKey } from './keys.ts';

describe('Tab', () => {
  it('по key и по физической клавише (ввод через службу текста Windows: key = Process)', () => {
    expect(isTabKey({ key: 'Tab', code: 'Tab' })).toBe(true);
    expect(isTabKey({ key: 'Process', code: 'Tab' })).toBe(true);
    expect(isTabKey({ key: 'Tab', code: '' })).toBe(true);
    expect(isTabKey({ key: 'Process', code: 'KeyA' })).toBe(false);
    expect(isTabKey({ key: 'Enter', code: 'Enter' })).toBe(false);
  });
});
