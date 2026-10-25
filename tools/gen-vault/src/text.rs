//! Текст заметок: заголовки, абзацы, списки, чекбоксы, цитаты, код и ссылки.

use crate::layout::{Eol, Special, Style};
use crate::links::Links;
use crate::rng::Rng;
use crate::words::*;

/// Байты файла заметки `idx`.
pub fn note_bytes(links: &mut Links, idx: usize, rng: &mut Rng, big_mb: Option<u64>) -> Vec<u8> {
    let note = &links.layout.notes[idx];
    let style = note.style;
    match note.special {
        Special::None => {
            let lines = bulk_lines(links, idx, rng);
            encode(&lines, style, rng)
        }
        Special::AllLinks => encode(&all_links_lines(links, idx), style, rng),
        Special::CodeNotLinks => encode(&code_lines(links, idx), style, rng),
        Special::InvalidUtf8 => invalid_utf8(),
        Special::Big => big_file(big_mb.unwrap_or(0)),
    }
}

fn bulk_lines(links: &mut Links, idx: usize, rng: &mut Rng) -> Vec<String> {
    let headings = links.layout.notes[idx].headings.clone();
    let mut lines = Vec::new();
    for heading in &headings {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("{} {heading}", "#".repeat(rng.range(1, 3))));
        lines.push(String::new());
        for block in 0..rng.range(1, 3) {
            if block > 0 {
                lines.push(String::new());
            }
            block_lines(links, idx, rng, &mut lines);
        }
    }
    lines
}

fn block_lines(links: &mut Links, idx: usize, rng: &mut Rng, lines: &mut Vec<String>) {
    let item = |links: &mut Links, rng: &mut Rng| -> String {
        if rng.chance(30) {
            format!("{} {}", sentence(rng), links.random_link(idx, rng))
        } else {
            sentence(rng)
        }
    };
    match rng.below(20) {
        0..=6 => {
            let mut p = item(links, rng);
            if rng.chance(30) {
                p = format!("{p} См. {}.", links.random_link(idx, rng));
            }
            lines.push(p);
        }
        7..=9 => {
            for _ in 0..rng.range(2, 4) {
                lines.push(format!("- {}", item(links, rng)));
            }
        }
        10 | 11 => {
            for i in 1..=rng.range(2, 4) {
                lines.push(format!("{i}. {}", item(links, rng)));
            }
        }
        12..=14 => {
            for _ in 0..rng.range(2, 4) {
                let mark = if rng.chance(40) { 'x' } else { ' ' };
                lines.push(format!("- [{mark}] {}", item(links, rng)));
            }
        }
        15 | 16 => lines.push(format!("> {}", item(links, rng))),
        17 => {
            // Блок кода: `[[...]]` внутри — не ссылка.
            let name = links.layout.notes[rng.below(links.layout.notes.len())]
                .name
                .clone();
            lines.push("```rust".to_string());
            lines.push(links.code(format!("let link = \"[[{name}]]\";")));
            lines.push("```".to_string());
        }
        18 => {
            let name = links.layout.notes[rng.below(links.layout.notes.len())]
                .name
                .clone();
            let code = links.code(format!("`[[{name}]]`"));
            lines.push(format!("Пример разметки: {code} — это код, не ссылка."));
        }
        _ => lines.push(sentence(rng)),
    }
}

/// «Все виды ссылок»: по одной ссылке каждого вида на известные образцы.
fn all_links_lines(links: &mut Links, idx: usize) -> Vec<String> {
    let mut lk = |t: &str, h: Option<&str>, a: Option<&str>| links.link(idx, t, h, a);
    vec![
        "# Цели".to_string(),
        String::new(),
        format!(
            "Простые: {} и {}.",
            lk("Ёлка", None, None),
            lk("Елка", None, None)
        ),
        format!(
            "С псевдонимом: {}.",
            lk("Заметка 10", None, Some("десятая заметка"))
        ),
        format!("На заголовок: {}.", lk("Заметка 2", Some("Итоги"), None)),
        format!(
            "На заголовок с псевдонимом: {}.",
            lk("Заметка 3", Some("Цели"), Some("цели третьей"))
        ),
        format!(
            "Кратчайший путь: {}, {}, {}.",
            lk("Альфа/Встреча", None, None),
            lk("Бета/Встреча", None, Some("встреча беты")),
            lk("Архив/2024/Отчёт", None, None)
        ),
        format!(
            "Корень и папка: {} и {}.",
            lk("/План", None, None),
            lk("Работа/План", None, None)
        ),
        format!(
            "Полный путь к уникальной: {}.",
            lk("Нумерация/Заметка 11", None, None)
        ),
        format!(
            "Другой регистр: {} и {}.",
            lk("заметка 20", None, None),
            lk("readme", None, None)
        ),
        format!("Имя в NFD, ссылка в NFC: {}.", lk("йогурт", None, None)),
        format!(
            "Без пути при дубликатах (самая старая): {} и {}.",
            lk("Встреча", None, None),
            lk("Отчёт", None, Some("отчёте"))
        ),
        format!("Глубоко: {}.", lk("Глубокая заметка", None, None)),
        format!(
            "Нет такого заголовка: {}.",
            lk("Заметка 1", Some("Нет такого заголовка"), None)
        ),
        String::new(),
        "## Итоги".to_string(),
        String::new(),
        format!(
            "Неразрешённые: {}, {}, {}, {}.",
            lk("Несуществующая заметка", None, None),
            lk("Нет такой папки/Заметка", None, None),
            lk("/Нет в корне", None, None),
            lk("Missing note", None, Some("ссылка в никуда"))
        ),
    ]
}

/// «Код и не ссылки»: `[[...]]` во всех видах кода и одна настоящая ссылка.
fn code_lines(links: &mut Links, idx: usize) -> Vec<String> {
    let real = links.link(idx, "Заметка 1", None, None);
    let mut c = |s: &str| links.code(s.to_string());
    vec![
        "# Цели".to_string(),
        String::new(),
        "Всё ниже, кроме последней строки, — код, а не ссылки.".to_string(),
        String::new(),
        "```rust".to_string(),
        c("let link = \"[[Встреча]]\";"),
        "```".to_string(),
        String::new(),
        "~~~".to_string(),
        c("[[План]] в блоке с тильдами"),
        "~~~".to_string(),
        String::new(),
        format!(
            "Код в строке: {} и {}.",
            c("`[[Заметка 2]]`"),
            c("``[[Заметка 3|x]]``")
        ),
        String::new(),
        c("    [[Отчёт]] в блоке с отступом"),
        String::new(),
        "## Итоги".to_string(),
        String::new(),
        format!("Настоящая ссылка: {real}."),
    ]
}

/// Файл с невалидным UTF-8 посередине: ядро открывает его только для чтения.
fn invalid_utf8() -> Vec<u8> {
    let mut out = "# Цели\n\nСтрока до невалидных байтов.\n"
        .as_bytes()
        .to_vec();
    out.extend_from_slice(b"\xD0\x9F\xD1\x80\xD0 \xFF\xFE \xC3\x28 \xED\xA0\x80\n");
    out.extend_from_slice("\n## Итоги\n\nСтрока после.\n".as_bytes());
    out
}

/// Заметка больше `mb` МБ: текст, заголовки, без ссылок.
fn big_file(mb: u64) -> Vec<u8> {
    let limit = (mb as usize) * 1024 * 1024;
    let mut out = "# Цели\n\n".as_bytes().to_vec();
    let line =
        "Большой файл: эта строка повторяется, чтобы превысить лимит размера для редактирования.\n";
    while out.len() <= limit {
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice("\n## Итоги\n\nКонец большого файла.\n".as_bytes());
    out
}

pub fn sentence(rng: &mut Rng) -> String {
    let words = if rng.chance(75) { WORDS_RU } else { WORDS_EN };
    let mut out = String::new();
    for i in 0..rng.range(4, 12) {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(rng.pick(words));
    }
    let mut chars = out.chars();
    let first: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    format!("{first}{}.", chars.as_str())
}

/// Байты файла: строки, переносы и BOM по стилю.
pub fn encode(lines: &[String], style: Style, rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    if style.bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    for (i, line) in lines.iter().enumerate() {
        out.extend_from_slice(line.as_bytes());
        let last = i + 1 == lines.len();
        if last && !style.final_newline {
            break;
        }
        let crlf = match style.eol {
            Eol::Lf => false,
            Eol::Crlf => true,
            // Смешанные: первые две строки разные, дальше — случайно.
            Eol::Mixed => match i {
                0 => false,
                1 => true,
                _ => rng.chance(50),
            },
        };
        out.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
    }
    out
}
