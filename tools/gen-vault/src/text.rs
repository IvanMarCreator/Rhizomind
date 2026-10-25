//! Текст заметок: заголовки, абзацы, списки, чекбоксы, код.

use crate::layout::{Eol, Layout, Style};
use crate::rng::Rng;
use crate::words::*;

/// Строки обычной заметки (без переносов).
pub fn bulk_lines(l: &Layout, idx: usize, rng: &mut Rng) -> Vec<String> {
    let note = &l.notes[idx];
    let mut lines = Vec::new();
    for heading in &note.headings {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("{} {heading}", "#".repeat(rng.range(1, 3))));
        lines.push(String::new());
        for block in 0..rng.range(1, 3) {
            if block > 0 {
                lines.push(String::new());
            }
            match rng.below(10) {
                0..=3 => lines.push(sentence(rng)),
                4 | 5 => {
                    for _ in 0..rng.range(2, 4) {
                        lines.push(format!("- {}", sentence(rng)));
                    }
                }
                6 => {
                    for i in 1..=rng.range(2, 4) {
                        lines.push(format!("{i}. {}", sentence(rng)));
                    }
                }
                7 | 8 => {
                    for _ in 0..rng.range(2, 4) {
                        let mark = if rng.chance(40) { 'x' } else { ' ' };
                        lines.push(format!("- [{mark}] {}", sentence(rng)));
                    }
                }
                _ => lines.push(format!("> {}", sentence(rng))),
            }
        }
    }
    lines
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
    let first: String = chars.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
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
