//! Генератор тестового хранилища Rhizomind (задача 1.6 плана 0.1).
//!
//! Создаёт хранилище из N заметок для тестов ядра и нагрузочных тестов:
//! вложенные папки, русские и английские имена, вики-ссылки всех видов,
//! дубликаты имён, вложения и образцы кодировок. Одинаковое зерно даёт
//! байт в байт одинаковое хранилище. Служебную папку `.rhizomind/` не создаёт.

mod layout;
mod links;
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
    let mut links = links::Links::new(&layout);
    for idx in 0..layout.notes.len() {
        let bytes = text::note_bytes(&mut links, idx, &mut rng, cfg.big_mb);
        fs::write(out.join(layout.note_path(idx)), bytes)?;
    }

    Ok(build_manifest(&layout, cfg, &links))
}

fn build_manifest(l: &layout::Layout, cfg: &Config, links: &links::Links) -> Manifest {
    let paths = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Manifest {
        format_version: FORMAT_VERSION,
        generator: format!("gen-vault {}", env!("CARGO_PKG_VERSION")),
        seed: cfg.seed,
        notes: l.notes.len(),
        folders: l.folders.len() - 1,
        max_depth: (0..l.folders.len()).map(|f| l.depth(f)).max().unwrap_or(0),
        duplicates: duplicates(l),
        links: link_stats(links),
        expected_links: expected_links(l, links),
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

fn link_stats(links: &links::Links) -> LinkStats {
    let mut s = LinkStats {
        total: 0,
        plain: 0,
        alias: 0,
        heading: 0,
        path: 0,
        root: 0,
        unresolved: 0,
        ambiguous_bare: 0,
        case_variant: 0,
        in_code: links.in_code,
    };
    for r in &links.records {
        let root = r.target_text.starts_with('/');
        let path = !root && r.target_text.contains('/');
        s.total += 1;
        s.root += root as usize;
        s.path += path as usize;
        s.heading += r.heading.is_some() as usize;
        s.alias += r.alias as usize;
        s.plain += (!root && !path && r.heading.is_none() && !r.alias) as usize;
        s.unresolved += (r.target == links::Target::Unresolved) as usize;
        s.ambiguous_bare += matches!(r.target, links::Target::Oldest(_)) as usize;
        s.case_variant += r.case_variant as usize;
    }
    s
}

/// Все ссылки образцов и ссылки примерно 300 массовых заметок.
fn expected_links(l: &layout::Layout, links: &links::Links) -> Vec<ExpectedLink> {
    let samples = l.samples;
    let step = ((l.notes.len() - samples) / 300).max(1);
    links
        .records
        .iter()
        .filter(|r| r.source < samples || (r.source - samples) % step == 0)
        .map(|r| {
            let (resolution, target, candidates) = match &r.target {
                links::Target::Unique(t) => (Resolution::Unique, Some(*t), Vec::new()),
                links::Target::Oldest(c) => (
                    Resolution::Oldest,
                    c.iter().min().copied(),
                    c.iter().map(|&i| l.note_path(i)).collect(),
                ),
                links::Target::Unresolved => (Resolution::Unresolved, None, Vec::new()),
            };
            let heading_exists = match (&r.heading, target) {
                (Some(h), Some(t)) => Some(l.notes[t].headings.contains(h)),
                _ => None,
            };
            ExpectedLink {
                source: l.note_path(r.source),
                link: r.raw.clone(),
                resolution,
                target: target.map(|t| l.note_path(t)),
                candidates,
                heading: r.heading.clone(),
                heading_exists,
            }
        })
        .collect()
}
