//! Построение раскладки: особые образцы и массовые заметки.

use crate::Config;
use crate::layout::{AttachmentKind, Eol, Layout, MAX_DEPTH, Special, Style};
use crate::names::name_key;
use crate::rng::Rng;
use crate::words::*;

pub fn plan(cfg: &Config, rng: &mut Rng) -> Layout {
    let mut l = Layout::new();
    add_samples(&mut l, cfg);
    l.samples = l.notes.len();
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
    add_attachments(&mut l, rng, open_from, (cfg.notes / 40).max(4) + 3);
    l
}

/// Вложения: семь в «Вложения» (PNG, PDF, TXT, CSV, ZIP), остальные PNG и PDF — по случайным папкам.
fn add_attachments(l: &mut Layout, rng: &mut Rng, open_from: usize, count: usize) {
    let inbox = l.ensure_folder(&["Вложения"]);
    l.add_attachment(inbox, "Схема.png", AttachmentKind::Png);
    l.add_attachment(inbox, "diagram.png", AttachmentKind::Png);
    l.add_attachment(inbox, "Скан договора.pdf", AttachmentKind::Pdf);
    l.add_attachment(inbox, "Report.pdf", AttachmentKind::Pdf);
    l.add_attachment(inbox, "Заметки к встрече.txt", AttachmentKind::Txt);
    l.add_attachment(inbox, "данные.csv", AttachmentKind::Csv);
    l.add_attachment(inbox, "Архив.zip", AttachmentKind::Zip);
    while l.attachments.len() < count {
        let folder = random_open_folder(l, rng, open_from, false);
        let n = rng.range(1, 999);
        let (name, kind) = if l.attachments.len().is_multiple_of(2) {
            (
                format!("{} {n}.png", rng.pick(&["Схема", "Скриншот", "photo"])),
                AttachmentKind::Png,
            )
        } else {
            (
                format!("{} {n}.pdf", rng.pick(&["Скан", "Счёт", "Invoice"])),
                AttachmentKind::Pdf,
            )
        };
        l.add_attachment(folder, &name, kind);
    }
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
            if rng.chance(20) {
                format!("{base} {}", rng.range(2, 9))
            } else {
                base.to_string()
            }
        }
        6 | 7 => rng.pick(FOLDERS_EN).to_string(),
        8 => format!("{}", rng.range(2019, 2026)),
        _ => format!("Неделя {}", rng.range(1, 52)),
    }
}

fn bulk_name(rng: &mut Rng) -> String {
    let base = match rng.below(10) {
        0..=3 => format!("{} {}", rng.pick(NOTE_NOUNS_RU), rng.range(1, 999)),
        4 | 5 => format!(
            "{} {} {}",
            rng.pick(NOTE_NOUNS_RU),
            rng.pick(TOPICS_RU),
            rng.range(1, 99)
        ),
        6 => format!("{} {}", rng.pick(NOTE_NOUNS_EN), rng.range(1, 999)),
        7 => format!(
            "{} {} {}",
            rng.pick(NOTE_NOUNS_EN),
            rng.pick(TOPICS_EN),
            rng.range(1, 99)
        ),
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
    Style {
        eol,
        bom: rng.chance(6),
        final_newline: !rng.chance(1),
    }
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
    let name = duplicate_name(l, rng, folder, bulk_from)
        .unwrap_or_else(|| unique_bulk_name(l, rng, folder));
    l.add_note(folder, &name, headings, style, Special::None);
}

/// Изредка — имя уже существующей массовой заметки из другой папки (дубликат).
/// Дубликаты с заметкой в корне делают только образцы: для пары «корень и
/// глубокая папка» правило С3.4 допускает два прочтения.
fn duplicate_name(l: &Layout, rng: &mut Rng, folder: usize, bulk_from: usize) -> Option<String> {
    if folder == 0 || l.notes.len() == bulk_from || !rng.chance(4) {
        return None;
    }
    let source = &l.notes[rng.range(bulk_from, l.notes.len() - 1)];
    let group = &l.by_key[&name_key(&source.name)];
    if source.folder == 0 || group.iter().any(|&i| l.notes[i].folder == 0) {
        return None;
    }
    // Путь одной заметки не должен совпадать с концом пути другой: правило
    // кратчайшего пути для таких пар (С3.4) ждёт решения владельца.
    let keys = |f: usize| l.folders[f].iter().map(|s| name_key(s)).collect::<Vec<_>>();
    let mine = keys(folder);
    if group.iter().any(|&i| {
        let other = keys(l.notes[i].folder);
        mine.ends_with(&other) || other.ends_with(&mine)
    }) {
        return None;
    }
    let name = if rng.chance(30) {
        source.name.to_lowercase()
    } else {
        source.name.clone()
    };
    l.note_name_free(folder, &name).then_some(name)
}

/// Образцы с известными именами: дубликаты из С3.4, естественная сортировка,
/// ё/е, регистр, NFD, глубина 5, кодировки и переносы.
fn add_samples(l: &mut Layout, cfg: &Config) {
    let h = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let note = |l: &mut Layout, folder: &[&str], name: &str, style: Style, special: Special| {
        let f = l.ensure_folder(folder);
        l.add_note(f, name, h(&["Цели", "Итоги"]), style, special);
    };
    let lf = Style::LF;
    note(l, &[], SAMPLE_ALL_LINKS, lf, Special::AllLinks);
    note(l, &[], SAMPLE_CODE, lf, Special::CodeNotLinks);
    // Дубликаты: одна папка с конца, корень и папка, две папки с конца, регистр.
    note(l, &["Проекты", "Альфа"], "Встреча", lf, Special::None);
    note(l, &["Проекты", "Бета"], "Встреча", lf, Special::None);
    note(l, &[], "План", lf, Special::None);
    note(l, &["Работа"], "План", lf, Special::None);
    note(l, &["Архив", "2024"], "Отчёт", lf, Special::None);
    note(l, &["Черновики", "2024"], "Отчёт", lf, Special::None);
    note(l, &["Учёба"], "Идеи", lf, Special::None);
    note(l, &["Личное"], "идеи", lf, Special::None);
    note(l, &["Учёба"], "Ёлка", lf, Special::None);
    note(l, &["Учёба"], "Елка", lf, Special::None);
    for n in [1, 2, 3, 10, 11, 20, 100] {
        note(
            l,
            &["Нумерация"],
            &format!("Заметка {n}"),
            lf,
            Special::None,
        );
    }
    note(l, &["Юникод"], SAMPLE_NFD, lf, Special::None);
    note(l, &["Юникод"], SAMPLE_HASH, lf, Special::NotLinked);
    note(l, &["English", "Notes"], "Project Alpha", lf, Special::None);
    note(l, &["English", "Notes"], "reading list", lf, Special::None);
    note(l, &["English", "Notes"], "README", lf, Special::None);
    note(l, &DEEP_FOLDERS, "Глубокая заметка", lf, Special::None);
    // Кодировки и переносы строк.
    let enc = ["Кодировки"];
    let style = |eol, bom, final_newline| Style {
        eol,
        bom,
        final_newline,
    };
    note(
        l,
        &enc,
        "LF без BOM",
        style(Eol::Lf, false, true),
        Special::None,
    );
    note(
        l,
        &enc,
        "CRLF без BOM",
        style(Eol::Crlf, false, true),
        Special::None,
    );
    note(
        l,
        &enc,
        "LF с BOM",
        style(Eol::Lf, true, true),
        Special::None,
    );
    note(
        l,
        &enc,
        "CRLF с BOM",
        style(Eol::Crlf, true, true),
        Special::None,
    );
    note(
        l,
        &enc,
        "Смешанные переносы",
        style(Eol::Mixed, false, true),
        Special::None,
    );
    note(
        l,
        &enc,
        "Без перевода строки в конце",
        style(Eol::Lf, false, false),
        Special::None,
    );
    note(l, &enc, "Невалидный UTF-8", lf, Special::InvalidUtf8);
    if cfg.big_mb.is_some() {
        note(l, &enc, "Большой файл", lf, Special::Big);
    }
}

pub const SAMPLE_ALL_LINKS: &str = "Все виды ссылок";
pub const SAMPLE_CODE: &str = "Код и не ссылки";
/// «Йогурт» в разложенной форме: «И» + U+0306.
pub const SAMPLE_NFD: &str = "И\u{306}огурт";
/// Имя с `#`: допустимо в Windows, но вики-ссылкой на него не сослаться
/// (`#` — начало заголовка), поэтому ссылок на эту заметку нет.
pub const SAMPLE_HASH: &str = "C# и F#";
pub const DEEP_FOLDERS: [&str; 5] = [
    "Уровень 1",
    "Уровень 2",
    "Уровень 3",
    "Уровень 4",
    "Уровень 5",
];

/// Число образцов: меньше заметок генератор не создаёт.
pub fn sample_count(cfg: &Config) -> usize {
    let mut l = Layout::new();
    add_samples(&mut l, cfg);
    l.notes.len()
}

/// Имя, которого нет ни у одной заметки хранилища и которое свободно в папке.
fn unique_bulk_name(l: &Layout, rng: &mut Rng, folder: usize) -> String {
    let mut name = bulk_name(rng);
    let mut extra = 2;
    while l.by_key.contains_key(&name_key(&name)) || !l.note_name_free(folder, &name) {
        name = if extra < 6 {
            bulk_name(rng)
        } else {
            format!("{name} {extra}")
        };
        extra += 1;
    }
    name
}
