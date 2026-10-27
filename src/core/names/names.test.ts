// Ключ имени интерфейса совпадает по смыслу с ключом ядра (names::key):
// те же примеры, что в тестах src-tauri/src/names/mod.rs.
import { describe, expect, it } from 'vitest';
import { nameKey } from './index.ts';

describe('nameKey — как names::key ядра', () => {
  it.each([
    ['Заметка.MD', 'заметка.md'],
    ['Straße.md', 'STRASSE.md'],
    ['STRAẞE', 'strasse'],
    ['ΌΣΟΣ', 'όσος'],
    ['ﬁle', 'FILE'],
    ['ВСТРЕЧА', 'встреча'],
    ['Мой план', 'Мой план'],
    ['Йод', 'Йод'],
    ['Café', 'CAFÉ'],
    ['ЁЛКА', 'ёлка'],
    ['ı.md', 'I.md'],
  ])('%s = %s', (a, b) => {
    expect(nameKey(a)).toBe(nameKey(b));
  });

  it('ё и е различаются', () => {
    expect(nameKey('Ёлка')).not.toBe(nameKey('Елка'));
  });

  it('ключ ключа — тот же ключ', () => {
    for (const name of ['Straße', 'ΐ', 'ᾳ', 'Мой', 'İstanbul', 'ǅ', 'ŉ']) {
      expect(nameKey(nameKey(name))).toBe(nameKey(name));
    }
  });

  it('пример составной «й»', () => {
    expect(nameKey('Мой')).toBe('мой');
  });
});
