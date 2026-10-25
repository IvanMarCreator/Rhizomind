//! Правила имён и ключ имени — интерфейс модуля; полная реализация и тесты
//! по С3.2 и Unicode — задача 1.1.
//!
//! Одно правило для всего ядра: проверка имён при вводе (С3.2), совпадение
//! имён в папке, разрешение вики-ссылок и поиск сравнивают имена по
//! [`NameKey`]. Модуль чистый: без диска и состояния, функции не падают.
//!
//! Сейчас здесь **предварительная рабочая реализация**, а не заглушка с
//! ошибкой: её вызывают команды и их тесты (`vault_create`, `entry_rename`),
//! а `todo!()` уронил бы их. Задача 1.1 заменяет [`key`] на NFC → полное
//! приведение регистра (`icu_casemap`) → NFC и доводит [`check`] до С3.2.

use crate::commands::{NameProblem, limits};

/// Ключ имени. Два имени совпадают для ядра, если совпадают их ключи
/// (`Заметка.md` и `заметка.MD`, составная и разложенная «й»).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameKey(String);

impl NameKey {
    /// Ключ строкой — для индексов и сравнения.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Ключ имени: NFC → полное приведение регистра → NFC.
///
/// Предварительно (до задачи 1.1): только `to_lowercase`, без нормализации
/// Unicode и без полного приведения регистра (ß ≠ SS).
pub fn key(name: &str) -> NameKey {
    NameKey(name.to_lowercase())
}

/// Расширение заметки.
pub const NOTE_EXTENSION: &str = "md";

/// Заметка ли это по имени: расширение `.md` без учёта регистра.
pub fn is_note(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case(NOTE_EXTENSION))
}

/// Символы, запрещённые в именах Windows (кроме управляющих).
const FORBIDDEN: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Зарезервированные имена устройств Windows (без учёта регистра, с любым
/// расширением: `nul.md`, `COM1.txt`). `COM¹`–`COM³` и `LPT¹`–`LPT³` Windows
/// тоже считает устройствами.
fn is_reserved(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = base.to_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|rest| {
            matches!(
                rest,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn ends_with_dot_or_space(text: &str) -> bool {
    text.ends_with('.') || text.ends_with(' ')
}

/// Правила имён Windows для одного имени файла или папки (без соседей):
/// пустое, запрещённые и управляющие символы, зарезервированные имена,
/// точка или пробел в конце — у заметки (`*.md`) и в конце основы имени
/// (`Итоги..md`, `План .md`), длина. `None` — имя подходит. Совпадение с
/// соседями (`duplicate`) проверяет `vault` по [`key`].
pub fn check(name: &str) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Empty);
    }
    if name
        .chars()
        .any(|c| c.is_control() || FORBIDDEN.contains(&c))
    {
        return Some(NameProblem::ForbiddenChars);
    }
    if is_reserved(name) {
        return Some(NameProblem::ReservedName);
    }
    if ends_with_dot_or_space(name) {
        return Some(NameProblem::TrailingDotOrSpace);
    }
    if is_note(name) {
        let stem = &name[..name.len() - NOTE_EXTENSION.len() - 1];
        if ends_with_dot_or_space(stem) {
            return Some(NameProblem::TrailingDotOrSpace);
        }
    }
    if !limits::component_fits(name) {
        return Some(NameProblem::TooLong);
    }
    None
}

/// Переименование не меняет вид элемента: заметка остаётся `.md`, другой
/// файл не становится заметкой. Нарушение — [`NameProblem::NoteExtension`].
pub fn check_rename_keeps_kind(old_name: &str, new_name: &str) -> Option<NameProblem> {
    (is_note(old_name) != is_note(new_name)).then_some(NameProblem::NoteExtension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_windows_rules() {
        assert_eq!(check(""), Some(NameProblem::Empty));
        assert_eq!(check("a:b.md"), Some(NameProblem::ForbiddenChars));
        assert_eq!(check("a\tb"), Some(NameProblem::ForbiddenChars));
        assert_eq!(check("CON"), Some(NameProblem::ReservedName));
        assert_eq!(check("nul.md"), Some(NameProblem::ReservedName));
        assert_eq!(check("com1.txt"), Some(NameProblem::ReservedName));
        assert_eq!(check("Заметка."), Some(NameProblem::TrailingDotOrSpace));
        assert_eq!(check("Заметка "), Some(NameProblem::TrailingDotOrSpace));
        assert_eq!(check(&"я".repeat(256)), Some(NameProblem::TooLong));
        assert_eq!(check("Встреча 2.md"), None);
        assert_eq!(check("CONSOLE.md"), None);
    }

    #[test]
    fn note_stem_must_not_end_with_dot_or_space() {
        assert_eq!(check("Итоги..md"), Some(NameProblem::TrailingDotOrSpace));
        assert_eq!(check("План .md"), Some(NameProblem::TrailingDotOrSpace));
        assert_eq!(check("v1.2.md"), None);
    }

    #[test]
    fn note_detection_by_extension() {
        assert!(is_note("Встреча.md"));
        assert!(is_note("Встреча.MD"));
        assert!(!is_note(".md"));
        assert!(!is_note("схема.png"));
        assert!(!is_note("md"));
    }

    #[test]
    fn rename_keeps_note_extension() {
        assert_eq!(check_rename_keeps_kind("a.md", "b.md"), None);
        assert_eq!(check_rename_keeps_kind("a.md", "a.MD"), None);
        assert_eq!(
            check_rename_keeps_kind("a.md", "a.txt"),
            Some(NameProblem::NoteExtension)
        );
        assert_eq!(
            check_rename_keeps_kind("a.txt", "a.md"),
            Some(NameProblem::NoteExtension)
        );
        assert_eq!(check_rename_keeps_kind("Папка", "Папка 2"), None);
    }

    #[test]
    fn key_ignores_case() {
        assert_eq!(key("Заметка.MD"), key("заметка.md"));
    }

    #[test]
    fn key_uses_full_case_folding() {
        // Полное приведение регистра: ß = SS, ς = σ, ﬁ = fi.
        assert_eq!(key("Straße.md"), key("STRASSE.md"));
        assert_eq!(key("ΌΣΟΣ"), key("όσος"));
        assert_eq!(key("ﬁle"), key("FILE"));
        assert_eq!(key("ВСТРЕЧА"), key("встреча"));
    }

    #[test]
    fn key_normalizes_unicode() {
        // «й» составная (U+0439) и разложенная (и + U+0306).
        assert_eq!(key("Мой план"), key("Мои\u{306} план"));
        // «Й» заглавная разложенная против строчной составной.
        assert_eq!(key("Йод"), key("И\u{306}од"));
        assert_eq!(key("Café"), key("CAFE\u{301}"));
        assert_eq!(key("Мои\u{306}").as_str(), "мой");
    }

    #[test]
    fn key_keeps_yo_and_e_apart() {
        assert_ne!(key("Ёлка"), key("Елка"));
        assert_eq!(key("ЁЛКА"), key("ёлка"));
    }

    #[test]
    fn reserved_names_with_superscript_digits_and_zero() {
        for name in [
            "COM¹", "com²", "Com³.md", "LPT¹", "lpt².txt", "LPT³", "COM0", "lpt0.md", "COM9",
            "LPT9.md", "AUX", "prn.md", "conin$", "CONOUT$.md", "NUL.tar.gz", "nul .md",
        ] {
            assert_eq!(check(name), Some(NameProblem::ReservedName), "{name}");
        }
        for name in ["COM10", "COM⁴", "LPT", "Конференция.md", "AUX1", "NULL.md", "com1x"] {
            assert_eq!(check(name), None, "{name}");
        }
    }

    #[test]
    fn forbidden_chars_each() {
        for c in ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0', '\u{1f}', '\u{7f}'] {
            let name = format!("План{c}2.md");
            assert_eq!(check(&name), Some(NameProblem::ForbiddenChars), "{name:?}");
        }
        assert_eq!(check("Встреча: итоги.md"), Some(NameProblem::ForbiddenChars));
    }

    #[test]
    fn trailing_dot_or_space_everywhere() {
        for name in ["Папка.", "Папка ", "Заметка.md.", "Заметка.md ", "Итоги..md", "План .md", ".", "..", "   "] {
            assert_eq!(check(name), Some(NameProblem::TrailingDotOrSpace), "{name:?}");
        }
        for name in [" Ведущий пробел.md", ".скрытая.md", "v1.2.md", "Отчёт 2024.pdf"] {
            assert_eq!(check(name), None, "{name:?}");
        }
    }

    #[test]
    fn too_long_counts_utf16_units() {
        // 255 единиц UTF-16 — можно, 256 — нет; эмодзи — две единицы.
        assert_eq!(check(&format!("{}.md", "я".repeat(252))), None);
        assert_eq!(check(&format!("{}.md", "я".repeat(253))), Some(NameProblem::TooLong));
        assert_eq!(check(&"😀".repeat(128)), Some(NameProblem::TooLong));
        assert_eq!(check(&"😀".repeat(127)), None);
    }

    #[test]
    fn require_valid_maps_problem_to_invalid_name() {
        assert!(require_valid("Встреча.md").is_ok());
        let err = require_valid("CON.md").unwrap_err();
        assert_eq!(err.name_problem, Some(NameProblem::ReservedName));
    }

    #[test]
    fn note_stem_without_extension() {
        assert_eq!(note_stem("Встреча.md"), Some("Встреча"));
        assert_eq!(note_stem("Встреча.MD"), Some("Встреча"));
        assert_eq!(note_stem("v1.2.md"), Some("v1.2"));
        assert_eq!(note_stem("схема.png"), None);
    }

    #[test]
    fn natural_order_of_names() {
        use std::cmp::Ordering::*;
        assert_eq!(natural_cmp("Заметка 2", "Заметка 10"), Less);
        assert_eq!(natural_cmp("Заметка 10", "Заметка 2"), Greater);
        assert_eq!(natural_cmp("Альфа", "бета"), Less);
        assert_eq!(natural_cmp("Елка", "Ёлка"), Less);
        assert_eq!(natural_cmp("Ёлка", "Жук"), Less);
        assert_eq!(natural_cmp("a01", "a1"), Greater);
        assert_eq!(natural_cmp("a1", "a1"), Equal);
        assert_ne!(natural_cmp("Заметка", "заметка"), Equal);
    }

    mod props {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn key_is_idempotent(name in "\\PC{0,40}") {
                let once = key(&name);
                prop_assert_eq!(key(once.as_str()), once);
            }

            #[test]
            fn key_ignores_case_of_any_name(name in "\\PC{0,40}") {
                prop_assert_eq!(key(&name.to_uppercase()), key(&name.to_lowercase()));
            }

            #[test]
            fn check_never_panics(name in "\\PC{0,300}") {
                let _ = check(&name);
            }

            #[test]
            fn natural_cmp_is_antisymmetric(a in "\\PC{0,12}", b in "\\PC{0,12}") {
                prop_assert_eq!(natural_cmp(&a, &b), natural_cmp(&b, &a).reverse());
            }
        }
    }
}
