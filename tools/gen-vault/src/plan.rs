//! Построение раскладки: особые образцы и массовые заметки.

use crate::Config;
use crate::layout::{Eol, Layout, MAX_DEPTH, Special, Style};
use crate::names::name_key;
use crate::rng::Rng;
use crate::words::*;

pub fn plan(cfg: &Config, rng: &mut Rng) -> Layout {
    let mut l = Layout::new();
    let open_from = l.folders.len();
    let bulk_from = l.notes.len();

    // Папки для массовых заметок: корень и всё, что создано после образцов.
    let target_folders = open_from + (cfg.notes / 25).max(6);
    while l.folders.len() < target_folders {
        let parent = random_open_folder(&l, rng, open_from, true);
        let name = folder_name(rng);
        l.add_folder(parent, &name);
    }

    while l.notes.len() < cfg.notes {
        add_bulk_note(&mut l, rng, open_from, bulk_from);
    }
    l
}

/// Корень или папка не из образцов; `for_child` — с местом для вложенной папки.
fn random_open_folder(l: &Layout, rng: &mut Rng, open_from: usize, for_child: bool) -> usize {
    loop {
        let idx = if rng.chance(10) || l.folders.len() == open_from {
            0
        } else {
            rng.range(open_from, l.folders.len() - 1)
        };
        if !for_child || l.depth(idx) < MAX_DEPTH {
            return idx;
        }
    }
}

fn folder_name(rng: &mut Rng) -> String {
    match rng.below(10) {
        0..=5 => {
            let base = *rng.pick(FOLDERS_RU);
            if rng.chance(20) { format!("{base} {}", rng.range(2, 9)) } else { base.to_string() }
        }
        6 | 7 => rng.pick(FOLDERS_EN).to_string(),
        8 => format!("{}", rng.range(2019, 2026)),
        _ => format!("Неделя {}", rng.range(1, 52)),
    }
}

fn bulk_name(rng: &mut Rng) -> String {
    let base = match rng.below(10) {
        0..=3 => format!("{} {}", rng.pick(NOTE_NOUNS_RU), rng.range(1, 999)),
        4 | 5 => format!("{} {} {}", rng.pick(NOTE_NOUNS_RU), rng.pick(TOPICS_RU), rng.range(1, 99)),
        6 => format!("{} {}", rng.pick(NOTE_NOUNS_EN), rng.range(1, 999)),
        7 => format!("{} {} {}", rng.pick(NOTE_NOUNS_EN), rng.pick(TOPICS_EN), rng.range(1, 99)),
        _ => format!(
            "{:04}-{:02}-{:02} {}",
            rng.range(2023, 2026),
            rng.range(1, 12),
            rng.range(1, 28),
            rng.pick(NOTE_NOUNS_RU)
        ),
    };
    match rng.below(100) {
        0..=4 => base.to_lowercase(),
        5 | 6 => base.to_uppercase(),
        _ => base,
    }
}

fn bulk_style(rng: &mut Rng) -> Style {
    let eol = match rng.below(100) {
        0..=87 => Eol::Lf,
        88..=97 => Eol::Crlf,
        _ => Eol::Mixed,
    };
    Style { eol, bom: rng.chance(6), final_newline: !rng.chance(1) }
}

fn random_headings(rng: &mut Rng) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for _ in 0..rng.range(1, 3) {
        let h = rng.pick(HEADINGS).to_string();
        if !out.contains(&h) {
            out.push(h);
        }
    }
    out
}

fn add_bulk_note(l: &mut Layout, rng: &mut Rng, open_from: usize, bulk_from: usize) {
    let folder = random_open_folder(l, rng, open_from, false);
    let style = bulk_style(rng);
    let headings = random_headings(rng);
    let name = unique_bulk_name(l, rng, folder);
    l.add_note(folder, &name, headings, style, Special::None);
    let _ = bulk_from;
}

/// Имя, которого нет ни у одной заметки хранилища и которое свободно в папке.
fn unique_bulk_name(l: &Layout, rng: &mut Rng, folder: usize) -> String {
    let mut name = bulk_name(rng);
    let mut extra = 2;
    while l.by_key.contains_key(&name_key(&name)) || !l.note_name_free(folder, &name) {
        name = if extra < 6 { bulk_name(rng) } else { format!("{name} {extra}") };
        extra += 1;
    }
    name
}
