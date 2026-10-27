// Ключ имени для сравнения без учёта регистра — то же правило, что у ядра
// (`names::key`, src-tauri/src/names/mod.rs): NFC → полное приведение
// регистра Unicode → NFC, повтор до устойчивого результата; `ı` → `i`.
// Окончательно имена сравнивает ядро; интерфейсу ключ нужен, чтобы не
// создавать папку, которую ядро сочтёт уже существующей (решение 84).
//
// В JavaScript нет полного приведения регистра (`fold`); его заменяет
// «в верхний регистр, затем в нижний» без учёта языка: так `ß` = `SS`,
// `ﬁ` = `FI`, `ς` = `σ`, `ſ` = `s`. Остаток — `ẞ` (заглавная эсцет), у
// которой нижний регистр `ß`: она приводится к `ss` отдельно.

/** Ключ имени: два имени совпадают для ядра, если совпадают ключи. */
export function nameKey(name: string): string {
  let current = name.normalize('NFC');
  for (let i = 0; i < 4; i++) {
    const folded = current.toUpperCase().toLowerCase().replaceAll('ß', 'ss').replaceAll('ı', 'i');
    const next = folded.normalize('NFC');
    if (next === current) break;
    current = next;
  }
  return current;
}
