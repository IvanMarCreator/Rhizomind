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
/// Единственное дополнение к приведению регистра — `ı` (U+0131) → `i`: NTFS
/// по своей таблице верхнего регистра считает `ı.md` и `I.md` одним файлом,
/// а приведение Unicode — разными. Других имён, которые NTFS считает одним,
/// а этот ключ разными, нет (проверено перебором символов при ревью 1.1);
/// обратное возможно (`ß` = `ss`) — правило ядра строже.
///
/// Приведение регистра и нормализация в редких случаях не перестановочны
/// (комбинируемые знаки вроде U+0345), поэтому шаг повторяется, пока ключ не
/// перестанет меняться: ключ ключа — тот же ключ. На обычных именах это один
/// проход; число повторов ограничено.
pub fn key(name: &str) -> NameKey {
    let mut current: String = name.nfc().collect();
    for _ in 0..4 {
        let folded = CaseMapper::new().fold_string(&current).replace('ı', "i");
        let next: String = folded.nfc().collect();
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

/// Порядок имён ядра — «по пути в естественном порядке» (С3.2, С3.4): им
/// ядро выбирает заметку при равной дате создания. Это дешёвое приближение
/// `Intl.Collator('ru', {numeric: true})`, а не сортировка дерева: дерево
/// сортирует интерфейс (папки первыми, `Intl.Collator`), и его порядок может
/// отличаться.
///
/// Уровни, как у Collator:
/// 1. буквы по ключу имени, «ё» = «е»; числа по значению («Заметка 2» перед
///    «Заметка 10»); пробелы и знаки перед цифрами, цифры перед буквами,
///    кириллица перед латиницей, латиница перед другими письменностями;
/// 2. «ё» после «е», больше ведущих нулей — позже;
/// 3. строчные перед заглавными;
/// 4. исходная строка — чтобы порядок был полным.
///
/// Порядок — сравнение ключей [`SortKey`], поэтому он транзитивен.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    sort_key(a).cmp(&sort_key(b))
}

/// Ключ сортировки имени для [`natural_cmp`]; считается один раз и
/// сравнивается сколько угодно раз.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SortKey {
    primary: Vec<Primary>,
    secondary: Vec<u8>,
    tertiary: Vec<u8>,
    raw: String,
}

/// Первичный вес: порядок вариантов — порядок групп.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Primary {
    /// Пробелы, знаки препинания, символы.
    Symbol(char),
    /// Число: длина без ведущих нулей, затем цифры.
    Number(usize, String),
    Cyrillic(char),
    Latin(char),
    Other(char),
}

/// Ключ сортировки имени.
pub fn sort_key(name: &str) -> SortKey {
    sort_key_with(name, &key(name))
}

/// Ключ сортировки по готовому ключу имени (без повторного приведения).
fn sort_key_with(raw: &str, key: &NameKey) -> SortKey {
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut chars = key.as_str().chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut digits = String::from(c);
            while let Some(d) = chars.next_if(char::is_ascii_digit) {
                digits.push(d);
            }
            let value = digits.trim_start_matches('0');
            let zeros = digits.len() - value.len();
            primary.push(Primary::Number(value.len(), value.to_owned()));
            secondary.push(u8::try_from(zeros).unwrap_or(u8::MAX));
            continue;
        }
        let (base, accent) = if c == 'ё' { ('е', 1) } else { (c, 0) };
        primary.push(match base {
            '\u{400}'..='\u{52f}' => Primary::Cyrillic(base),
            c if c.is_alphabetic() && c < '\u{250}' => Primary::Latin(base),
            c if c.is_alphanumeric() => Primary::Other(base),
            c => Primary::Symbol(c),
        });
        secondary.push(accent);
    }
    SortKey {
        primary,
        secondary,
        tertiary: raw.chars().map(|c| u8::from(c.is_uppercase())).collect(),
        raw: raw.to_owned(),
    }
}

/// Путь с заранее посчитанными ключами компонентов — для частых сравнений
/// без повторного приведения (разрешение ссылок, поиск).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedPath {
    keys: Vec<NameKey>,
    sort: Vec<SortKey>,
}

impl KeyedPath {
    /// Ключи компонентов пути; каждый ключ считается один раз.
    pub fn new(components: &[String]) -> Self {
        let keys: Vec<NameKey> = components.iter().map(|c| key(c)).collect();
        let sort = components
            .iter()
            .zip(&keys)
            .map(|(raw, key)| sort_key_with(raw, key))
            .collect();
        Self { keys, sort }
    }

    /// Ключи компонентов.
    pub fn keys(&self) -> &[NameKey] {
        &self.keys
    }

    /// Ключ сортировки последнего компонента (имени).
    pub fn name_sort_key(&self) -> Option<&SortKey> {
        self.sort.last()
    }
}

/// Порядок путей ядра: по компонентам, каждый — как [`natural_cmp`];
/// путь-начало другого пути — раньше.
impl Ord for KeyedPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort.cmp(&other.sort)
    }
}

impl PartialOrd for KeyedPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
    fn unlinkable_names_are_allowed_with_warning() {
        // С3.2 (решение владельца 25.10.2026): `#`, `[`, `]`, пробел в начале.
        for name in ["#Идеи.md", "C#.md", "[Черновик].md", "a]b.md", " Пробел.md", "Папка #1", "[архив]"] {
            assert_eq!(check(name), None, "{name:?}");
            assert_eq!(warning(name), Some(NameWarning::Unlinkable), "{name:?}");
        }
        for name in ["Встреча.md", "C++.md", "Папка 1", "схема.png", "Итоги.md.md", "Пробел внутри.md"] {
            assert_eq!(warning(name), None, "{name:?}");
        }
        assert!(is_linkable("Встреча"));
        assert!(!is_linkable(" Встреча"));
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
        let alphabet = [
            "е", "ё", "Е", "Ё", "ж", "и", "й", "a", "A", "1", "01", "2", "10", " ", "-", "ß", "ss",
        ];
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
                assert_eq!(
                    ord == Equal,
                    words[i] == words[j],
                    "{:?} {:?}",
                    words[i],
                    words[j]
                );
                if i < j {
                    assert_ne!(
                        ord, Greater,
                        "{:?} > {:?} после сортировки",
                        words[i], words[j]
                    );
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
