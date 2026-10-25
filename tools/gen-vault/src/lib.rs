//! Генератор тестового хранилища Rhizomind (задача 1.6 плана 0.1).
//!
//! Создаёт хранилище из N заметок для тестов ядра и нагрузочных тестов:
//! вложенные папки, русские и английские имена, вики-ссылки всех видов,
//! дубликаты имён, вложения и образцы кодировок. Одинаковое зерно даёт
//! байт в байт одинаковое хранилище. Служебную папку `.rhizomind/` не создаёт.

mod binary;
mod layout;
mod links;
mod manifest;
mod names;
mod plan;
mod rng;
mod shortest;
mod text;
mod words;

use names::name_key;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Наибольший размер файла «больше лимита», МБ.
pub const MAX_BIG_MB: u64 = 1024;

/// Пауза между раундами записи дубликатов: больше шага таймера ФС
/// (NTFS обновляет время с шагом около 16 мс).
const ROUND_PAUSE: Duration = Duration::from_millis(60);

/// Скрытые файл и папка (С3.2: дерево их не показывает); в число заметок не входят.
const HIDDEN: [(&str, &str); 2] = [
    (
        ".скрытая заметка.md",
        "# Скрытая заметка\n\nФайл с точкой в начале имени.\n",
    ),
    (
        ".скрытая папка/Заметка в скрытой папке.md",
        "# Заметка в скрытой папке\n\nПапка с точкой в начале имени.\n",
    ),
];

/// Создаёт хранилище в папке `out` (её нет или она пуста).
pub fn generate(out: &Path, cfg: &Config) -> io::Result<Manifest> {
    if out.exists() && fs::read_dir(out)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("папка {} не пуста", out.display()),
        ));
    }
    let min = plan::sample_count(cfg);
    if cfg.notes < min {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "--notes {}: нужно не меньше {min} (столько образцов)",
                cfg.notes
            ),
        ));
    }
    if cfg.big_mb.is_some_and(|mb| mb > MAX_BIG_MB) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("--big-mb: не больше {MAX_BIG_MB} МБ"),
        ));
    }
    let mut rng = rng::Rng::new(cfg.seed);
    let layout = plan::plan(cfg, &mut rng);

    for folder in &layout.folders {
        fs::create_dir_all(out.join(folder.join("/")))?;
    }
    // Содержимое считается последовательно (детерминированно), запись — потом.
    let mut links = links::Links::new(&layout);
    let mut counts = EolCounts::default();
    let mut rounds: Vec<Vec<(PathBuf, Vec<u8>)>> = Vec::new();
    let mut files = Vec::new();
    for idx in 0..layout.notes.len() {
        let path = out.join(layout.note_path(idx));
        if layout.notes[idx].special == layout::Special::Big {
            // Большой файл пишется потоком, без сборки в памяти: LF, без BOM.
            counts.big_bytes = text::write_big_file(&path, cfg.big_mb.unwrap_or(0))?;
            counts.lf += 1;
            continue;
        }
        let bytes = text::note_bytes(&mut links, idx, &mut rng);
        counts.add(&bytes);
        let group = &layout.by_key[&name_key(&layout.notes[idx].name)];
        if group.len() > 1 {
            let rank = group.iter().position(|&i| i == idx).unwrap();
            if rounds.len() <= rank {
                rounds.resize_with(rank + 1, Vec::new);
            }
            rounds[rank].push((path, bytes));
        } else {
            files.push((path, bytes));
        }
    }
    for idx in 0..layout.attachments.len() {
        let bytes = match layout.attachments[idx].kind {
            layout::AttachmentKind::Png => binary::png(&mut rng),
            layout::AttachmentKind::Pdf => binary::pdf(&mut rng),
            layout::AttachmentKind::Txt => binary::txt(),
            layout::AttachmentKind::Csv => binary::csv(&mut rng),
            layout::AttachmentKind::Zip => binary::zip(),
        };
        files.push((out.join(layout.attachment_path(idx)), bytes));
    }
    for (path, text) in HIDDEN {
        let path = out.join(path);
        fs::create_dir_all(path.parent().unwrap())?;
        files.push((path, text.as_bytes().to_vec()));
    }
    // Заметки с повторяющимися именами — раундами: сначала первые заметки всех
    // групп, пауза больше шага таймера ФС, затем вторые и т. д. Так дата
    // создания у `created_first` строго меньше, чем у остальных заметок группы:
    // на неё ведёт ссылка без пути (С3.4).
    for (n, round) in rounds.iter().enumerate() {
        if n > 0 {
            std::thread::sleep(ROUND_PAUSE);
        }
        write_parallel(round)?;
    }
    write_parallel(&files)?;

    Ok(build_manifest(&layout, cfg, &links, counts))
}

/// Запись файлов в несколько потоков: на Windows создание файла дорогое
/// (антивирус проверяет каждый), а порядок этих файлов не важен.
fn write_parallel(files: &[(PathBuf, Vec<u8>)]) -> io::Result<()> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 8);
    let chunk = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .try_for_each(|(path, bytes)| fs::write(path, bytes))
                })
            })
            .collect();
        handles
            .into_iter()
            .try_for_each(|h| h.join().expect("поток записи упал"))
    })
}

/// Подсчёт переносов строк и BOM по записанным заметкам.
#[derive(Default)]
struct EolCounts {
    lf: usize,
    crlf: usize,
    mixed: usize,
    bom: usize,
    no_final_newline: usize,
    big_bytes: u64,
}

impl EolCounts {
    fn add(&mut self, bytes: &[u8]) {
        let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();
        let lf = bytes.iter().filter(|&&c| c == b'\n').count() - crlf;
        match (crlf > 0, lf > 0) {
            (false, true) => self.lf += 1,
            (true, false) => self.crlf += 1,
            (true, true) => self.mixed += 1,
            (false, false) => {}
        }
        self.bom += bytes.starts_with(b"\xEF\xBB\xBF") as usize;
        self.no_final_newline += !bytes.ends_with(b"\n") as usize;
    }
}

fn build_manifest(
    l: &layout::Layout,
    cfg: &Config,
    links: &links::Links,
    eol: EolCounts,
) -> Manifest {
    let paths = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Manifest {
        format_version: FORMAT_VERSION,
        generator: format!("gen-vault {}", env!("CARGO_PKG_VERSION")),
        seed: cfg.seed,
        notes: l.notes.len(),
        folders: l.folders.len() - 1,
        max_depth: (0..l.folders.len()).map(|f| l.depth(f)).max().unwrap_or(0),
        duplicates: duplicates(l),
        key_rule: KEY_RULE.to_string(),
        hidden: {
            let mut h: Vec<String> = HIDDEN.iter().map(|(p, _)| p.to_string()).collect();
            h.sort();
            h
        },
        attachments: Attachments {
            png: l
                .attachments
                .iter()
                .filter(|a| a.kind == layout::AttachmentKind::Png)
                .count(),
            pdf: l
                .attachments
                .iter()
                .filter(|a| a.kind == layout::AttachmentKind::Pdf)
                .count(),
            paths: (0..l.attachments.len())
                .filter(|&i| {
                    matches!(
                        l.attachments[i].kind,
                        layout::AttachmentKind::Png | layout::AttachmentKind::Pdf
                    )
                })
                .map(|i| l.attachment_path(i))
                .collect(),
            other: (0..l.attachments.len())
                .filter(|&i| {
                    !matches!(
                        l.attachments[i].kind,
                        layout::AttachmentKind::Png | layout::AttachmentKind::Pdf
                    )
                })
                .map(|i| l.attachment_path(i))
                .collect(),
        },
        encodings: Encodings {
            lf: eol.lf,
            crlf: eol.crlf,
            mixed: eol.mixed,
            bom: eol.bom,
            no_final_newline: eol.no_final_newline,
            samples: encoding_samples(l, cfg, eol.big_bytes),
        },
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
            hash_in_name: format!("Юникод/{}.md", plan::SAMPLE_HASH),
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
        .filter(|r| r.source < samples || (r.source - samples).is_multiple_of(step))
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

fn encoding_samples(l: &layout::Layout, cfg: &Config, big_bytes: u64) -> EncodingSamples {
    let p = |name: &str| format!("Кодировки/{name}.md");
    let big = cfg.big_mb.map(|mb| {
        let idx = (0..l.notes.len())
            .find(|&i| l.notes[i].special == layout::Special::Big)
            .unwrap();
        BigFile {
            path: l.note_path(idx),
            bytes: big_bytes,
            limit_mb: mb,
        }
    });
    EncodingSamples {
        lf: p("LF без BOM"),
        crlf: p("CRLF без BOM"),
        lf_bom: p("LF с BOM"),
        crlf_bom: p("CRLF с BOM"),
        mixed: p("Смешанные переносы"),
        no_final_newline: p("Без перевода строки в конце"),
        invalid_utf8: p("Невалидный UTF-8"),
        big,
    }
}
