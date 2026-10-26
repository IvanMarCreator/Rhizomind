//! Назначения горячих клавиш в настройках хранилища (решение владельца 72).
//!
//! Реестр команд интерфейса и их сочетания по умолчанию живут в интерфейсе
//! (`src/settings/hotkeys.ts`); ядро хранит только назначения пользователя
//! поверх них — `{ "<команда>": ["Ctrl+KeyO", …] }` — и проверяет формат:
//! имя команды, число команд и сочетаний, запись сочетания. Пустой список —
//! у команды нет сочетаний; команды нет в объекте — сочетания по умолчанию.
//!
//! Сочетание — модификаторы в порядке `Ctrl`, `Alt`, `Shift` и физическая
//! клавиша (`KeyboardEvent.code`, поэтому работает в любой раскладке) через
//! `+`: `Ctrl+Shift+KeyO`, `Alt+ArrowLeft`, `F2`. Без `Ctrl` и `Alt` — только
//! функциональные клавиши F1–F12 и `Delete`: иначе сочетание
//! мешало бы вводу текста. Системные и общие сочетания правки
//! ([`HOTKEY_RESERVED`]) не назначаются. Те же списки — в `src/api/limits.ts`
//! и в снимке `contracts/v1/limits.snap`.

use std::collections::BTreeMap;

/// Назначения пользователя: команда → её сочетания.
pub type HotkeyOverrides = BTreeMap<String, Vec<String>>;

/// Команд с назначениями — не больше.
pub const MAX_HOTKEY_COMMANDS: usize = 64;
/// Сочетаний у одной команды — не больше.
pub const MAX_HOTKEY_CHORDS: usize = 4;
/// Имя команды, байт (`[a-z0-9._-]`).
pub const MAX_HOTKEY_COMMAND_BYTES: usize = 64;

/// Модификаторы в порядке записи.
pub const HOTKEY_MODIFIERS: &[&str] = &["Ctrl", "Alt", "Shift"];

/// Физические клавиши, которые можно назначить (`KeyboardEvent.code`).
/// Esc (отмена записи) и Tab (переход фокуса) не назначаются.
pub const HOTKEY_KEYS: &[&str] = &[
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Insert",
    "Delete",
    "Backspace",
    "Enter",
    "Space",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Comma",
    "Period",
    "Slash",
    "Backquote",
];

/// Клавиши, которые можно назначить без `Ctrl` и `Alt`.
pub const HOTKEY_BARE_KEYS: &[&str] = &[
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Delete",
];

/// Системные сочетания Windows и общие сочетания правки текста: не
/// переназначаются.
pub const HOTKEY_RESERVED: &[&str] = &[
    "Alt+F4",
    "Alt+Space",
    "Ctrl+Alt+Delete",
    "Shift+F10",
    "Ctrl+KeyA",
    "Ctrl+KeyC",
    "Ctrl+KeyV",
    "Ctrl+KeyX",
    "Ctrl+KeyY",
    "Ctrl+KeyZ",
    "Ctrl+Shift+KeyZ",
];

/// Имя команды: 1–64 байта из `a-z`, `0-9`, `.`, `_`, `-`.
pub fn command_is_valid(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= MAX_HOTKEY_COMMAND_BYTES
        && command.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// Сочетание записано по правилам модуля и его можно назначить.
pub fn chord_is_valid(chord: &str) -> bool {
    let mut parts: Vec<&str> = chord.split('+').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    if !HOTKEY_KEYS.contains(&key) {
        return false;
    }
    // Модификаторы — без повторов и в порядке HOTKEY_MODIFIERS.
    let mut next = 0;
    for modifier in &parts {
        match HOTKEY_MODIFIERS[next..].iter().position(|m| m == modifier) {
            Some(offset) => next += offset + 1,
            None => return false,
        }
    }
    let with_command_modifier = parts.iter().any(|m| matches!(*m, "Ctrl" | "Alt"));
    (with_command_modifier || HOTKEY_BARE_KEYS.contains(&key)) && !HOTKEY_RESERVED.contains(&chord)
}

/// Назначения команды: не больше [`MAX_HOTKEY_CHORDS`], каждое допустимо,
/// без повторов.
fn chords_are_valid(chords: &[String]) -> bool {
    chords.len() <= MAX_HOTKEY_CHORDS
        && chords.iter().all(|c| chord_is_valid(c))
        && chords
            .iter()
            .enumerate()
            .all(|(i, c)| !chords[..i].contains(c))
}

/// Все назначения допустимы (проверка `settings_write`).
pub fn overrides_are_valid(overrides: &HotkeyOverrides) -> bool {
    overrides.len() <= MAX_HOTKEY_COMMANDS
        && overrides
            .iter()
            .all(|(command, chords)| command_is_valid(command) && chords_are_valid(chords))
}

/// Терпимое чтение файла: недопустимая команда пропускается целиком (у неё
/// остаются сочетания по умолчанию); больше [`MAX_HOTKEY_COMMANDS`] — первые
/// по порядку имён.
pub fn sanitize(overrides: HotkeyOverrides) -> HotkeyOverrides {
    overrides
        .into_iter()
        .filter(|(command, chords)| command_is_valid(command) && chords_are_valid(chords))
        .take(MAX_HOTKEY_COMMANDS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_with_modifiers_in_order_and_known_key() {
        for chord in [
            "Ctrl+KeyO",
            "Ctrl+Shift+KeyO",
            "Ctrl+Alt+Shift+Digit1",
            "Alt+ArrowLeft",
            "F2",
            "Shift+F3",
            "Delete",
            "Ctrl+Comma",
        ] {
            assert!(chord_is_valid(chord), "{chord}");
        }
    }

    #[test]
    fn rejects_bad_order_repeats_unknown_keys_and_letters_without_command_modifier() {
        for chord in [
            "",
            "Ctrl",
            "Ctrl+",
            "Shift+Ctrl+KeyO",
            "Ctrl+Ctrl+KeyO",
            "Meta+KeyO",
            "ctrl+KeyO",
            "Ctrl+o",
            "Ctrl+Escape",
            "Ctrl+Tab",
            "KeyO",
            "Shift+KeyO",
            "ArrowLeft",
            "Enter",
            "Ctrl+KeyO ",
            "Insert",
            "Backspace",
        ] {
            assert!(!chord_is_valid(chord), "{chord:?}");
        }
    }

    #[test]
    fn system_and_editing_chords_are_reserved() {
        for chord in HOTKEY_RESERVED {
            assert!(!chord_is_valid(chord), "{chord}");
        }
    }

    #[test]
    fn command_names() {
        assert!(command_is_valid("quick_switcher.open"));
        assert!(command_is_valid("tree.undo-rename"));
        assert!(!command_is_valid(""));
        assert!(!command_is_valid("Quick"));
        assert!(!command_is_valid("быстрый"));
        assert!(!command_is_valid(&"a".repeat(65)));
        assert!(command_is_valid(&"a".repeat(64)));
    }

    fn overrides(entries: &[(&str, &[&str])]) -> HotkeyOverrides {
        entries
            .iter()
            .map(|(c, chords)| {
                (
                    c.to_string(),
                    chords.iter().map(|s| s.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn overrides_limits() {
        assert!(overrides_are_valid(&overrides(&[
            ("quick_switcher.open", &["Ctrl+KeyP", "Ctrl+KeyO"]),
            ("note.new", &[]),
        ])));
        assert!(!overrides_are_valid(&overrides(&[(
            "a",
            &["F1", "F2", "F3", "F4", "F5"]
        )])));
        assert!(!overrides_are_valid(&overrides(&[("a", &["F1", "F1"])])));
        assert!(!overrides_are_valid(&overrides(&[("A", &["F1"])])));
        let many: HotkeyOverrides = (0..65).map(|i| (format!("c{i}"), vec![])).collect();
        assert!(!overrides_are_valid(&many));
    }

    #[test]
    fn sanitize_drops_only_bad_commands() {
        let read = sanitize(overrides(&[
            ("good", &["Ctrl+KeyP"]),
            ("bad", &["Alt+F4"]),
            ("Bad", &["F1"]),
        ]));
        assert_eq!(read, overrides(&[("good", &["Ctrl+KeyP"])]));
        let many: HotkeyOverrides = (0..70).map(|i| (format!("c{i:02}"), vec![])).collect();
        assert_eq!(sanitize(many).len(), MAX_HOTKEY_COMMANDS);
    }
}
