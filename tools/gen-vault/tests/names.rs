//! Имена в хранилище: правила Windows (С3.2), дубликаты и кратчайший
//! различающий путь (С3.4). Проверки написаны независимо от генератора.

mod common;

use common::{TempDir, read_dirs, read_tree};
use gen_vault::{Config, Manifest, generate};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

fn key(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase().nfc().collect()
}

/// Правила имени Windows из С3.2: запрещённые символы, зарезервированные
/// имена (и с расширением), точка или пробел в конце, управляющие символы.
fn windows_name_problem(name: &str) -> Option<&'static str> {
    if name.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        return Some("запрещённый символ");
    }
    if name.chars().any(char::is_control) {
        return Some("управляющий символ");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("точка или пробел в конце");
    }
    let stem = name
        .split('.')
        .next()
        .unwrap()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return Some("зарезервированное имя");
    }
    None
}

fn vault(notes: usize, seed: u64) -> (TempDir, Manifest) {
    let dir = TempDir::new("names");
    let manifest = generate(dir.path(), &Config::new(notes, seed)).unwrap();
    (dir, manifest)
}

/// Заметка: путь без `.md` по компонентам и ключи компонентов.
struct NoteRef {
    parts: Vec<String>,
    keys: Vec<String>,
}

fn notes_of(tree: &BTreeMap<String, Vec<u8>>) -> Vec<NoteRef> {
    tree.keys()
        .filter_map(|p| p.strip_suffix(".md"))
        .map(|p| {
            let parts: Vec<String> = p.split('/').map(str::to_string).collect();
            let keys = parts.iter().map(|s| key(s)).collect();
            NoteRef { parts, keys }
        })
        .collect()
}

/// Заметки, на которые указывает ссылка по правилам С3.4: `/` — путь от
/// корня, иначе — совпадение конца пути (без учёта регистра, с NFC).
fn resolve<'a>(notes: &'a [NoteRef], link: &str) -> Vec<&'a Vec<String>> {
    let (anchored, body) = match link.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, link),
    };
    let want: Vec<String> = body.split('/').map(key).collect();
    notes
        .iter()
        .filter(|n| {
            if anchored {
                n.keys == want
            } else {
                n.keys.ends_with(&want)
            }
        })
        .map(|n| &n.parts)
        .collect()
}

#[test]
fn every_name_is_valid_on_windows_and_unique_in_its_folder() {
    let (dir, manifest) = vault(1500, 7);
    let tree = read_tree(dir.path());
    let dirs = read_dirs(dir.path());

    let mut siblings: BTreeSet<(String, String)> = BTreeSet::new();
    for path in tree.keys().chain(dirs.iter()) {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        assert!(!name.starts_with('.'), "скрытое имя {path}");
        if let Some(problem) = windows_name_problem(name) {
            panic!("{path}: {problem}");
        }
        assert!(
            siblings.insert((key(parent), key(name))),
            "имя повторяется в папке без учёта регистра: {path}"
        );
    }
    assert!(!dirs.iter().any(|d| d.split('/').any(|c| c == ".rhizomind")));

    let depth = dirs.iter().map(|d| d.split('/').count()).max().unwrap();
    assert_eq!(depth, 5, "глубина папок");
    assert_eq!(manifest.max_depth, 5);
    assert_eq!(manifest.folders, dirs.len());
}

#[test]
fn name_samples_cover_numbers_case_yo_and_unicode_forms() {
    let (dir, manifest) = vault(200, 3);
    let tree = read_tree(dir.path());
    let has = |p: &str| tree.contains_key(p);

    assert!(
        has("Нумерация/Заметка 2.md") && has("Нумерация/Заметка 10.md"),
        "естественная сортировка"
    );
    assert!(
        has("Учёба/Ёлка.md") && has("Учёба/Елка.md"),
        "ё и е — разные имена"
    );
    assert!(
        has("Учёба/Идеи.md") && has("Личное/идеи.md"),
        "одно имя в разном регистре"
    );
    // Имя в разложенной форме Unicode (NFD): «Й» = «И» + U+0306.
    let nfd = "Юникод/И\u{306}огурт.md";
    assert!(has(nfd), "имя в NFD");
    assert_eq!(manifest.samples.nfd_name, nfd);
    assert!(
        tree.keys()
            .any(|p| p.chars().any(|c| c.is_ascii_alphabetic())),
        "английские имена"
    );
    assert!(tree.keys().any(|p| p.contains(' ')), "пробелы в именах");
    assert!(
        manifest
            .samples
            .natural_sort
            .contains(&"Нумерация/Заметка 10.md".to_string())
    );
}

#[test]
fn duplicates_match_manifest_and_get_shortest_unique_paths() {
    let (dir, manifest) = vault(2000, 11);
    let tree = read_tree(dir.path());
    let notes = notes_of(&tree);

    // Группы одинаковых имён по диску.
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for n in &notes {
        groups
            .entry(n.keys.last().unwrap().clone())
            .or_default()
            .insert(format!("{}.md", n.parts.join("/")));
    }
    groups.retain(|_, v| v.len() > 1);
    let from_manifest: BTreeMap<String, BTreeSet<String>> = manifest
        .duplicates
        .iter()
        .map(|g| {
            (
                g.key.clone(),
                g.notes.iter().map(|n| n.path.clone()).collect(),
            )
        })
        .collect();
    assert_eq!(groups, from_manifest);
    assert!(groups.len() > 10, "мало дубликатов: {}", groups.len());

    for group in &manifest.duplicates {
        assert!(group.notes.iter().any(|n| n.path == group.created_first));
        for n in &group.notes {
            let link = &n.shortest_link;
            let hits = resolve(&notes, link);
            assert_eq!(hits.len(), 1, "{link} указывает не на одну заметку");
            assert_eq!(format!("{}.md", hits[0].join("/")), n.path);
            // Короче нельзя: без первой папки ссылка уже неоднозначна.
            if let Some(rest) = link.strip_prefix('/') {
                assert!(
                    resolve(&notes, rest).len() > 1,
                    "{link}: хватило бы конца пути"
                );
            } else if let Some((_, shorter)) = link.split_once('/') {
                assert!(
                    resolve(&notes, shorter).len() > 1,
                    "{link}: есть путь короче"
                );
            }
        }
    }

    // Примеры из С3.4 — известные значения.
    let link_of = |path: &str| -> String {
        manifest
            .duplicates
            .iter()
            .flat_map(|g| &g.notes)
            .find(|n| n.path == path)
            .unwrap()
            .shortest_link
            .clone()
    };
    assert_eq!(link_of("Проекты/Альфа/Встреча.md"), "Альфа/Встреча");
    assert_eq!(link_of("Проекты/Бета/Встреча.md"), "Бета/Встреча");
    assert_eq!(link_of("План.md"), "/План");
    assert_eq!(link_of("Работа/План.md"), "Работа/План");
    assert_eq!(link_of("Архив/2024/Отчёт.md"), "Архив/2024/Отчёт");
    assert_eq!(link_of("Учёба/Идеи.md"), "Учёба/Идеи");
}
