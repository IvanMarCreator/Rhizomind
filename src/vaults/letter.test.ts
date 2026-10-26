import { describe, expect, it } from 'vitest';
import { vaultLetter } from './letter.ts';

describe('буква хранилища', () => {
  it('первая буква имени заглавной', () => {
    expect(vaultLetter('рабочая база')).toBe('Р');
    expect(vaultLetter('  архив')).toBe('А');
  });

  it('первый символ целиком, даже вне базовой плоскости Unicode', () => {
    expect(vaultLetter('𝔸 знаки')).toBe('𝔸');
  });

  it('пустое имя — пусто', () => {
    expect(vaultLetter('   ')).toBe('');
  });
});
