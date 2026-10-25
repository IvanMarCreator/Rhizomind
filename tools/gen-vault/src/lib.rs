//! Генератор тестового хранилища Rhizomind (задача 1.6 плана 0.1).
//!
//! Создаёт хранилище из N заметок для тестов ядра и нагрузочных тестов:
//! вложенные папки, русские и английские имена, вики-ссылки всех видов,
//! дубликаты имён, вложения и образцы кодировок. Одинаковое зерно даёт
//! байт в байт одинаковое хранилище. Служебную папку `.rhizomind/` не создаёт.

mod layout;
mod names;
mod plan;
mod rng;
mod text;
mod words;

use serde::{Deserialize, Serialize};
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
        Config { notes, seed, big_mb: None }
    }
}

/// Сводка созданного хранилища.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub generator: String,
    pub seed: u64,
    pub notes: usize,
}

/// Создаёт хранилище в папке `out` (её нет или она пуста).
pub fn generate(out: &Path, cfg: &Config) -> io::Result<Manifest> {
    if out.exists() && fs::read_dir(out)?.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("папка {} не пуста", out.display())));
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

    Ok(Manifest {
        format_version: 1,
        generator: format!("gen-vault {}", env!("CARGO_PKG_VERSION")),
        seed: cfg.seed,
        notes: layout.notes.len(),
    })
}
