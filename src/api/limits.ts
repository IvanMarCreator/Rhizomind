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
  /** Полный путь к папке на диске (`vault_create.parent`), единиц UTF-16. */
  max_folder_path_utf16: 32_767,
  /** Настройка «максимальный размер файла для редактирования», байт. */
  edit_size_range: [MIB, 100 * MIB],
  /** Размер шрифта текста, пиксели. */
  font_size_range: [10, 32],
  /** Команд с назначениями горячих клавиш в настройках. */
  hotkey_max_commands: 64,
  /** Сочетаний у одной команды. */
  hotkey_max_chords: 4,
  /** Имя команды, байт (`[a-z0-9._-]`). */
  hotkey_max_command_bytes: 64,
  /** Модификаторы в порядке записи сочетания. */
  hotkey_modifiers: ['Ctrl', 'Alt', 'Shift'],
  /** Физические клавиши (`KeyboardEvent.code`), которые можно назначить. */
  hotkey_keys: [
    'KeyA', 'KeyB', 'KeyC', 'KeyD', 'KeyE', 'KeyF', 'KeyG', 'KeyH', 'KeyI', 'KeyJ', 'KeyK', 'KeyL', 'KeyM',
    'KeyN', 'KeyO', 'KeyP', 'KeyQ', 'KeyR', 'KeyS', 'KeyT', 'KeyU', 'KeyV', 'KeyW', 'KeyX', 'KeyY', 'KeyZ',
    'Digit0', 'Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5', 'Digit6', 'Digit7', 'Digit8', 'Digit9', 'F1',
    'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12', 'ArrowLeft', 'ArrowRight', 'ArrowUp',
    'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown', 'Insert', 'Delete', 'Backspace', 'Enter', 'Space',
    'Minus', 'Equal', 'BracketLeft', 'BracketRight', 'Backslash', 'Semicolon', 'Quote', 'Comma', 'Period',
    'Slash', 'Backquote',
  ],
  /** Клавиши, которые можно назначить без Ctrl и Alt. */
  hotkey_bare_keys: [
    'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12', 'Delete',
  ],
  /** Системные сочетания и сочетания правки: не назначаются. */
  hotkey_reserved: [
    'Alt+F4', 'Alt+Space', 'Ctrl+Alt+Delete', 'Shift+F10', 'Ctrl+KeyA', 'Ctrl+KeyC', 'Ctrl+KeyV', 'Ctrl+KeyX',
    'Ctrl+KeyY', 'Ctrl+KeyZ', 'Ctrl+Shift+KeyZ',
  ],
} as const;
