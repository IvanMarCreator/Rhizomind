//! Теги заметок (С2-3.3, решение 113): в тексте и в блоке свойств, все формы
//! ключей `tags`/`tag`, кириллица, разный регистр одного тега, `-` и `/` в имени
//! (тег плоский: `работа/отчёт` и `работа` — разные теги).

use crate::rng::Rng;
use crate::text::sentence;
use unicode_normalization::UnicodeNormalization;

/// Словарь тегов; ключи имён (NFC → нижний регистр → NFC) попарно различны.
pub const TAGS: &[&str] = &[
    "проект",
    "идея",
    "работа",
    "работа/отчёт",
    "работа/план",
    "учёба",
    "личное",
    "встреча",
    "книги",
    "книги/фантастика",
    "чтение",
    "рецепт",
    "здоровье",
    "спорт",
    "финансы",
    "путешествия",
    "вопрос",
    "срочно",
    "важное",
    "на-потом",
    "черновик_v2",
    "Колонния_людей",
    "ёлка",
    "елка",
    "архив/2024",
    "идеи/новые",
    "ui-дизайн",
    "2026-план",
    "q3-цели",
    "x86",
    // «за́мок»: о + комбинируемое ударение U+0301 (составной формы нет).
    "замо\u{301}к",
    "café",
    "1с-бухгалтерия",
    "ИБ",
    "Rhizomind",
    "todo",
    "inbox",
    "project",
    "idea",
    "reading",
    "draft",
    "review",
    "research/ml",
    "meeting-notes",
    "life/health",
    "rust",
];

/// Значения в блоке свойств, которые не теги (С2-3.3: пропускаются).
const NOT_TAG_VALUES: &[&str] = &["2026", "\"два слова\""];

/// Ловушки в тексте: похоже на тег, но не тег (С2-3.3).
const TRAP: &str =
    "Задача #123 из трекера, год #2026, адрес https://example.org/#раздел, \\#не_тег и язык C#.";

/// Теги одной заметки: строки блока свойств (без `---`), строка с тегами в тексте и ловушка.
pub struct NoteTags {
    pub front: Vec<String>,
    pub text_line: Option<String>,
    pub trap: Option<String>,
}

/// Учёт тегов по хранилищу: каждое вхождение (заметка, написание) в порядке файла.
#[derive(Default)]
pub struct TagLog {
    pub occurrences: Vec<(usize, String)>,
    multi_form: usize,
    single_form: usize,
    notes: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Front,
    Text,
    Both,
}

impl TagLog {
    /// 2–5 разных тегов заметки `idx`; блок свойств — у двух третей заметок.
    pub fn note(&mut self, idx: usize, rng: &mut Rng) -> NoteTags {
        self.notes += 1;
        let n = rng.range(2, 5);
        let mut picked: Vec<&str> = Vec::with_capacity(n);
        while picked.len() < n {
            let t = *rng.pick(TAGS);
            if !picked.contains(&t) {
                picked.push(t);
            }
        }
        let has_front = rng.chance(65);
        let mut places: Vec<Place> = picked
            .iter()
            .map(|_| match (has_front, rng.below(100)) {
                (false, _) => Place::Text,
                (true, 0..=59) => Place::Front,
                (true, 60..=84) => Place::Text,
                (true, _) => Place::Both,
            })
            .collect();
        if has_front && !places.contains(&Place::Front) && !places.contains(&Place::Both) {
            places[0] = Place::Front;
        }
        let front: Vec<String> = picked
            .iter()
            .zip(&places)
            .filter(|(_, p)| **p != Place::Text)
            .map(|(t, _)| spelling(t, rng))
            .collect();
        let text: Vec<String> = picked
            .iter()
            .zip(&places)
            .filter(|(_, p)| **p != Place::Front)
            .map(|(t, _)| spelling(t, rng))
            .collect();

        let front_lines = if front.is_empty() {
            Vec::new()
        } else {
            self.front_lines(&front, rng)
        };
        for t in front.iter().chain(&text) {
            self.occurrences.push((idx, t.clone()));
        }
        NoteTags {
            front: front_lines,
            text_line: (!text.is_empty()).then(|| text_line(&text, rng)),
            trap: self.notes.is_multiple_of(20).then(|| TRAP.to_string()),
        }
    }

    /// Строки блока свойств: другие ключи и теги в одной из форм С2-3.3
    /// (формы идут по кругу, чтобы в любом хранилище были все).
    fn front_lines(&mut self, tags: &[String], rng: &mut Rng) -> Vec<String> {
        let mut out = Vec::new();
        if rng.chance(50) {
            out.push(format!(
                "created: {:04}-{:02}-{:02}",
                rng.range(2023, 2026),
                rng.range(1, 12),
                rng.range(1, 28)
            ));
        }
        if rng.chance(15) {
            // `#` в значении другого ключа — не тег.
            out.push("title: \"Черновик #заметки\"".to_string());
        }
        let skip = rng.chance(15).then(|| *rng.pick(NOT_TAG_VALUES));
        let list = |v: &[String]| v.join(", ");
        let block = |key: &str, items: Vec<String>| {
            let mut lines = vec![format!("{key}:")];
            lines.extend(items.into_iter().map(|i| format!("  - {i}")));
            lines
        };
        let quoted: Vec<String> = tags
            .iter()
            .enumerate()
            .map(|(i, t)| match i % 3 {
                0 => format!("\"#{t}\""),
                1 => format!("\"{t}\""),
                _ => t.clone(),
            })
            .collect();
        let with_skip = |mut v: Vec<String>| {
            if let Some(s) = skip {
                let at = v.len() / 2;
                v.insert(at, s.to_string());
            }
            v
        };
        if tags.len() == 1 {
            let t = &tags[0];
            self.single_form += 1;
            out.push(match self.single_form % 3 {
                0 => format!("tags: {t}"),
                1 => format!("tag: \"#{t}\""),
                _ => format!("Tags: {t}"),
            });
        } else {
            self.multi_form += 1;
            match self.multi_form % 7 {
                0 => out.push(format!("tags: [{}]", list(&with_skip(tags.to_vec())))),
                1 => out.push(format!("tags: {}", list(tags))),
                2 => out.extend(block("tags", with_skip(tags.to_vec()))),
                3 => out.push(format!("tags: [{}]", list(&quoted))),
                4 => {
                    let items = tags
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            if i % 2 == 0 {
                                format!("\"#{t}\"")
                            } else {
                                format!("'{t}'")
                            }
                        })
                        .collect();
                    out.extend(block("Tags", items));
                }
                5 => out.push(format!("TAG: [{}]", list(tags))),
                _ => out.push(format!("tag: {}", list(tags))),
            }
        }
        if rng.chance(40) {
            out.push(format!("status: {}", rng.pick(&["черновик", "готово"])));
        }
        if rng.chance(15) {
            // Список под другим ключом — не теги.
            out.extend(block(
                "aliases",
                vec![format!("Синоним {}", rng.range(1, 99))],
            ));
        }
        out
    }
}

/// Написание тега: как в словаре, строчными, с прописной или прописными;
/// `café` — иногда в разложенной форме (NFD).
fn spelling(base: &str, rng: &mut Rng) -> String {
    let s = match rng.below(100) {
        0..=9 => base.to_lowercase(),
        10..=19 => {
            let mut c = base.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
        20..=24 => base.to_uppercase(),
        _ => base.to_string(),
    };
    if base == "café" && rng.chance(40) {
        s.nfd().collect()
    } else {
        s
    }
}

/// Строка текста с тегами: перед `#` — начало строки, пробел или `( « " '`.
fn text_line(tags: &[String], rng: &mut Rng) -> String {
    let items: Vec<String> = tags
        .iter()
        .map(|t| match rng.below(6) {
            0 => format!("(#{t})"),
            1 => format!("«#{t}»"),
            2 => format!("\"#{t}\""),
            3 => format!("'#{t}'"),
            4 => format!("#{t},"),
            _ => format!("#{t}"),
        })
        .collect();
    let items = items.join(" ");
    match rng.below(4) {
        0 | 1 => format!("Метки: {items}"),
        2 => format!("{items} — {}", sentence(rng)),
        _ => format!("{} {items}", sentence(rng)),
    }
}
