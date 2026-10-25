//! Генератор тестового хранилища Rhizomind (задача 1.6 плана 0.1).
//!
//! Создаёт хранилище из N заметок для тестов ядра и нагрузочных тестов:
//! вложенные папки, русские и английские имена, вики-ссылки всех видов,
//! дубликаты имён, вложения и образцы кодировок. Одинаковое зерно даёт
//! байт в байт одинаковое хранилище. Служебную папку `.rhizomind/` не создаёт.

mod layout;
mod manifest;
mod names;
mod plan;
mod rng;
mod shortest;
mod text;
mod words;

use std::fs;
use std::io;
use std::path::Path;

pub use names::{is_valid_windows_name, name_key};

/// Параметры генерации.
#[derive(Debug, Clone)]
pub struct Config {
    /// Число заметок (`.md`), включая образцы.
    pub notes: usize,
    pub seed: u64,
    /// Размер файла «больше лимита» в МБ; `None` — файл не создаётся.
    pub big_mb: Option<u64>,
}

impl Config {
    pub fn new(notes: usize, seed: u64) -> Self {
        Config {
            notes,
            seed,
            big_mb: None,
        }
    }
}

pub use manifest::*;

/// Создаёт хранилище в папке `out` (её нет или она пуста).
pub fn generate(out: &Path, cfg: &Config) -> io::Result<Manifest> {
    if out.exists() && fs::read_dir(out)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("папка {} не пуста", out.display()),
        ));
    }
    let mut rng = rng::Rng::new(cfg.seed);
    let layout = plan::plan(cfg, &mut rng);

    for folder in &layout.folders {
        fs::create_dir_all(out.join(folder.join("/")))?;
    }
    for idx in 0..layout.notes.len() {
        let lines = text::bulk_lines(&layout, idx, &mut rng);
        let bytes = text::encode(&lines, layout.notes[idx].style, &mut rng);
        fs::write(out.join(layout.note_path(idx)), bytes)?;
    }

    Ok(build_manifest(&layout, cfg))
}

fn build_manifest(l: &layout::Layout, cfg: &Config) -> Manifest {
    let paths = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Manifest {
        format_version: FORMAT_VERSION,
        generator: format!("gen-vault {}", env!("CARGO_PKG_VERSION")),
        seed: cfg.seed,
        notes: l.notes.len(),
        folders: l.folders.len() - 1,
        max_depth: (0..l.folders.len()).map(|f| l.depth(f)).max().unwrap_or(0),
        duplicates: duplicates(l),
        samples: Samples {
            natural_sort: [1, 2, 3, 10, 11, 20, 100]
                .iter()
                .map(|n| format!("Нумерация/Заметка {n}.md"))
                .collect(),
            yo_e: paths(&["Учёба/Ёлка.md", "Учёба/Елка.md"]),
            case_variants: paths(&["Учёба/Идеи.md", "Личное/идеи.md"]),
            nfd_name: format!("Юникод/{}.md", plan::SAMPLE_NFD),
            root_and_folder: paths(&["План.md", "Работа/План.md"]),
            deepest: format!("{}/Глубокая заметка.md", plan::DEEP_FOLDERS.join("/")),
        },
    }
}

fn duplicates(l: &layout::Layout) -> Vec<DuplicateGroup> {
    let mut groups: Vec<(&String, &Vec<usize>)> =
        l.by_key.iter().filter(|(_, v)| v.len() > 1).collect();
    groups.sort();
    groups
        .into_iter()
        .map(|(key, members)| DuplicateGroup {
            key: key.clone(),
            notes: members
                .iter()
                .map(|&i| DuplicateNote {
                    path: l.note_path(i),
                    shortest_link: shortest::shortest_link(l, i),
                })
                .collect(),
            created_first: l.note_path(*members.iter().min().unwrap()),
        })
        .collect()
}
