//! Правила имён и ключ имени (спецификация 0.1, С3.2 и С3.4).
//!
//! Одно правило для всего ядра: проверка имён при вводе (С3.2), совпадение
//! имён в папке, разрешение вики-ссылок и поиск сравнивают имена по
//! [`NameKey`]. Модуль чистый: без диска и состояния, функции не падают.

use std::cmp::Ordering;

use icu_casemap::CaseMapper;
use unicode_normalization::UnicodeNormalization;

use crate::commands::{CoreError, CoreResult, NameProblem, limits};

/// Ключ имени. Два имени совпадают для ядра, если совпадают их ключи
/// (`Заметка.md` и `заметка.MD`, составная и разложенная «й», `Straße` и
/// `STRASSE`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameKey(String);

impl NameKey {
    /// Ключ строкой — для индексов и сравнения.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Ключ имени: NFC → полное приведение регистра (Unicode full case folding,
/// `icu_casemap`) → NFC.
///
/// Приведение регистра и нормализация в редких случаях не перестановочны
/// (комбинируемые знаки вроде U+0345), поэтому шаг повторяется, пока ключ не
/// перестанет меняться: ключ ключа — тот же ключ. На обычных именах это один
/// проход; число повторов ограничено.
pub fn key(name: &str) -> NameKey {
    let mut current: String = name.nfc().collect();
    for _ in 0..4 {
        let next: String = CaseMapper::new().fold_string(&current).nfc().collect();
        if next == current {
            break;
        }
        current = next;
    }
    NameKey(current)
}

/// Расширение заметки.
pub const NOTE_EXTENSION: &str = "md";

/// Заметка ли это по имени: расширение `.md` без учёта регистра.
pub fn is_note(name: &str) -> bool {
    note_stem(name).is_some()
}

/// Основа имени заметки — имя без `.md` (`Встреча.md` → `Встреча`); `None`,
/// если это не заметка.
pub fn note_stem(name: &str) -> Option<&str> {
    name.rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case(NOTE_EXTENSION))
        .map(|(stem, _)| stem)
}

/// Символы, запрещённые в именах Windows (кроме управляющих).
pub const FORBIDDEN: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Запрещённый в имени символ: из [`FORBIDDEN`] или управляющий.
pub fn is_forbidden_char(c: char) -> bool {
    c.is_control() || FORBIDDEN.contains(&c)
}

/// Зарезервированные имена устройств Windows: без учёта регистра, с любым
/// расширением и пробелами перед ним (`nul.md`, `COM1.txt`, `NUL .md`).
/// `COM0`–`COM9`, `LPT0`–`LPT9` и с надстрочными цифрами `¹ ² ³` Windows тоже
/// считает устройствами (документация Microsoft «Naming Files, Paths, and
/// Namespaces»).
fn is_reserved(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = base.to_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
        })
    })
}

fn ends_with_dot_or_space(text: &str) -> bool {
    text.ends_with('.') || text.ends_with(' ')
}

/// Правила имён Windows для одного имени файла или папки (без соседей), по
/// порядку: пустое, запрещённые и управляющие символы, зарезервированные
/// имена, точка или пробел в конце — всего имени и основы имени заметки
/// (`Итоги..md`, `План .md`), длина больше 255 единиц UTF-16. `None` — имя
/// подходит. Причина — для пояснения под полем ввода (`NameProblem::message_key`).
/// Совпадение с соседями (`duplicate`) проверяет `vault` по [`key`].
pub fn check(name: &str) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Empty);
    }
    if name.chars().any(is_forbidden_char) {
        return Some(NameProblem::ForbiddenChars);
    }
    if is_reserved(name) {
        return Some(NameProblem::ReservedName);
    }
    if ends_with_dot_or_space(name) || note_stem(name).is_some_and(ends_with_dot_or_space) {
        return Some(NameProblem::TrailingDotOrSpace);
    }
    if !limits::component_fits(name) {
        return Some(NameProblem::TooLong);
    }
    None
}

/// [`check`] для слоя команд: нарушение — ошибка `invalid_name` с причиной.
pub fn require_valid(name: &str) -> CoreResult<()> {
    check(name).map_or(Ok(()), |problem| Err(CoreError::invalid_name(problem)))
}

/// Переименование не меняет вид элемента: заметка остаётся `.md`, другой
/// файл не становится заметкой. Нарушение — [`NameProblem::NoteExtension`].
pub fn check_rename_keeps_kind(old_name: &str, new_name: &str) -> Option<NameProblem> {
    (is_note(old_name) != is_note(new_name)).then_some(NameProblem::NoteExtension)
}

/// Естественный порядок имён: числа сравниваются по значению («Заметка 2»
/// перед «Заметка 10»), текст — по ключу имени, «ё» сразу после «е». При
/// равенстве ключей — по исходной строке, чтобы порядок был полным и
/// одинаковым на всех машинах. Это приближение `Intl.Collator('ru',
/// {numeric: true})` интерфейса для решений ядра (выбор самой старой заметки
/// при равной дате, С3.4), а не сортировка дерева.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ka, kb) = (key(a), key(b));
    natural_cmp_keys(ka.as_str(), kb.as_str()).then_with(|| a.cmp(b))
}

/// Путь с заранее посчитанными ключами компонентов — для частых сравнений
/// без повторного приведения (разрешение ссылок, поиск).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedPath {
    raw: Vec<String>,
    keys: Vec<NameKey>,
}

impl KeyedPath {
    /// Ключи компонентов пути.
    pub fn new(components: &[String]) -> Self {
        Self {
            raw: components.to_vec(),
            keys: components.iter().map(|c| key(c)).collect(),
        }
    }

    /// Ключи компонентов.
    pub fn keys(&self) -> &[NameKey] {
        &self.keys
    }
}

/// Естественный порядок путей: по компонентам, каждый — как [`natural_cmp`];
/// путь-начало другого пути — раньше.
impl Ord for KeyedPath {
    fn cmp(&self, other: &Self) -> Ordering {
        for ((ka, ra), (kb, rb)) in self
            .keys
            .iter()
            .zip(&self.raw)
            .zip(other.keys.iter().zip(&other.raw))
        {
            let ord = natural_cmp_keys(ka.as_str(), kb.as_str()).then_with(|| ra.cmp(rb));
            if ord != Ordering::Equal {
                return ord;
            }
        }
        self.keys.len().cmp(&other.keys.len())
    }
}

impl PartialOrd for KeyedPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Естественный порядок двух уже приведённых ключей.
fn natural_cmp_keys(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let da = take_digits(&mut a);
                let db = take_digits(&mut b);
                let ord = compare_numbers(&da, &db);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = letter_weight(x).cmp(&letter_weight(y));
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        digits.push(c);
    }
    digits
}

/// Числа по значению (без ведущих нулей, затем по длине), при равном
/// значении — меньше ведущих нулей раньше.
fn compare_numbers(a: &str, b: &str) -> Ordering {
    let (ta, tb) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    ta.len()
        .cmp(&tb.len())
        .then_with(|| ta.cmp(tb))
        .then_with(|| a.len().cmp(&b.len()))
}

/// Вес буквы: кодовая точка, «ё» — между «е» и «ж».
fn letter_weight(c: char) -> (u32, u8) {
    match c {
        'ё' => ('е' as u32, 1),
        other => (other as u32, 0),
    }
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
            "COM¹",
            "com²",
            "Com³.md",
            "LPT¹",
            "lpt².txt",
            "LPT³",
            "COM0",
            "lpt0.md",
            "COM9",
            "LPT9.md",
            "AUX",
            "prn.md",
            "conin$",
            "CONOUT$.md",
            "NUL.tar.gz",
            "nul .md",
        ] {
            assert_eq!(check(name), Some(NameProblem::ReservedName), "{name}");
        }
        for name in [
            "COM10",
            "COM⁴",
            "LPT",
            "Конференция.md",
            "AUX1",
            "NULL.md",
            "com1x",
        ] {
            assert_eq!(check(name), None, "{name}");
        }
    }

    #[test]
    fn forbidden_chars_each() {
        for c in [
            '\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0', '\u{1f}', '\u{7f}',
        ] {
            let name = format!("План{c}2.md");
            assert_eq!(check(&name), Some(NameProblem::ForbiddenChars), "{name:?}");
        }
        assert_eq!(
            check("Встреча: итоги.md"),
            Some(NameProblem::ForbiddenChars)
        );
    }

    #[test]
    fn trailing_dot_or_space_everywhere() {
        for name in [
            "Папка.",
            "Папка ",
            "Заметка.md.",
            "Заметка.md ",
            "Итоги..md",
            "План .md",
            ".",
            "..",
            "   ",
        ] {
            assert_eq!(
                check(name),
                Some(NameProblem::TrailingDotOrSpace),
                "{name:?}"
            );
        }
        for name in [
            " Ведущий пробел.md",
            ".скрытая.md",
            "v1.2.md",
            "Отчёт 2024.pdf",
        ] {
            assert_eq!(check(name), None, "{name:?}");
        }
    }

    #[test]
    fn too_long_counts_utf16_units() {
        // 255 единиц UTF-16 — можно, 256 — нет; эмодзи — две единицы.
        assert_eq!(check(&format!("{}.md", "я".repeat(252))), None);
        assert_eq!(
            check(&format!("{}.md", "я".repeat(253))),
            Some(NameProblem::TooLong)
        );
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

    #[test]
    fn dotless_i_matches_capital_i_like_ntfs() {
        // NTFS считает `ı.md` и `I.md` одним файлом (таблица верхнего регистра).
        assert_eq!(key("ı.md"), key("I.md"));
        assert_eq!(key("ı.md"), key("i.md"));
        assert_eq!(key("Sıra"), key("SIRA"));
    }

    #[test]
    fn order_close_to_intl_collator_ru() {
        use std::cmp::Ordering::*;
        // «ё» — вторичное отличие: важно, только если остальное равно.
        assert_eq!(natural_cmp("Ёлка", "Елки"), Less);
        assert_eq!(natural_cmp("Елки", "Ёлка"), Greater);
        assert_eq!(natural_cmp("Елка", "Ёлка"), Less);
        // Кириллица перед латиницей.
        assert_eq!(natural_cmp("Яблоко", "Apple"), Less);
        assert_eq!(natural_cmp("zebra", "аист"), Greater);
        // Регистр — вторично: строчные перед заглавными при равных буквах,
        // но буква важнее регистра.
        assert_eq!(natural_cmp("заметка", "Заметка"), Less);
        assert_eq!(natural_cmp("Заметка а", "заметка б"), Less);
        assert_eq!(natural_cmp("Б", "в"), Less);
        // Знаки и пробелы перед цифрами, цифры перед буквами.
        assert_eq!(natural_cmp("-план", "1 план"), Less);
        assert_eq!(natural_cmp("1 план", "план"), Less);
        // Числа по значению; ведущие нули — вторично.
        assert_eq!(natural_cmp("Заметка 9", "Заметка 10"), Less);
        assert_eq!(natural_cmp("a01", "a1"), Greater);
        assert_eq!(natural_cmp("a01", "a2"), Less);
    }

    #[test]
    fn order_is_total_and_transitive_on_small_alphabet() {
        use std::cmp::Ordering::*;
        let alphabet = ["е", "ё", "Е", "Ё", "ж", "и", "й", "a", "A", "1", "01", "2", "10", " ", "-", "ß", "ss"];
        let mut words: Vec<String> = alphabet.iter().map(|s| s.to_string()).collect();
        for a in alphabet {
            for b in alphabet {
                words.push(format!("{a}{b}"));
            }
        }
        words.sort_by(|a, b| natural_cmp(a, b));
        for i in 0..words.len() {
            for j in 0..words.len() {
                let ord = natural_cmp(&words[i], &words[j]);
                assert_eq!(ord, natural_cmp(&words[j], &words[i]).reverse());
                assert_eq!(ord == Equal, words[i] == words[j], "{:?} {:?}", words[i], words[j]);
                if i < j {
                    assert_ne!(ord, Greater, "{:?} > {:?} после сортировки", words[i], words[j]);
                }
            }
        }
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
