// Пределы параметров команд ядра API v1 — те же, что в Rust
// (src-tauri/src/commands/limits.rs); совпадение проверяет снимок
// contracts/v1/limits.snap. Интерфейс может проверять ввод заранее, но
// окончательно решает ядро (`invalid_argument` с именем поля).
const MIB = 1024 * 1024;

export const LIMITS = {
  /** Целей в одном `links_resolve`. */
  max_link_targets: 10_000,
  /** Строка поиска, символов Unicode. */
  max_query_chars: 256,
  /** Компонент пути или имя, единиц UTF-16 (как длина строки в JS). */
  max_component_utf16: 255,
  /** Компонентов в пути. */
  max_path_components: 64,
  /** Жёсткий потолок текста `note_write`, байт UTF-8. */
  max_note_bytes: 100 * MIB,
  /** Раскладка окна в JSON, байт. */
  max_workspace_bytes: MIB,
  /** Адрес `open_external`, байт UTF-8; управляющие символы запрещены. */
  max_url_bytes: 8 * 1024,
  /** Настройка «максимальный размер файла для редактирования», байт. */
  edit_size_range: [MIB, 100 * MIB],
  /** Размер шрифта текста, пиксели. */
  font_size_range: [10, 32],
} as const;
