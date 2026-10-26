// Буква хранилища на метке в списке и переключателе (макеты Э10б, Э11а):
// первый символ имени заглавной буквой.

/** Первый символ имени (целиком, с учётом суррогатных пар) заглавной. */
export function vaultLetter(name: string): string {
  return Array.from(name.trim())[0]?.toLocaleUpperCase('ru') ?? '';
}
