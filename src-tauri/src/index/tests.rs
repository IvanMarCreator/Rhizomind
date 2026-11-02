//! Тесты индекса через его интерфейс: [`VaultIndex`] (записи, обратные
//! ссылки, теги), [`read_entry`] и [`rebuild`] на хранилище во временной
//! папке и на хранилище генератора `tools/gen-vault`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Mutex, RwLock};

use proptest::prelude::*;

use super::*;
use crate::commands::{IndexState, VaultPath};
use crate::links::Links;
use crate::markdown::parse;
use crate::vault::{NoteRef, Vault};

fn vp(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

fn entry(text: &str, created_ms: i64) -> NoteEntry {
    NoteEntry {
        size: text.len() as u64,
        modified_ms: created_ms,
        created_ms,
        parsed: parse(text),
    }
}

/// Индекс и список заметок по `(путь, текст, дата создания)`.
fn index_of(notes: &[(&str, &str, i64)]) -> (VaultIndex, Links) {
    let mut index = VaultIndex::new();
    for (path, text, created) in notes {
        index.upsert(vp(path), entry(text, *created));
    }
    let links = Links::new(
        notes
            .iter()
            .map(|(path, _, created)| NoteRef {
                path: vp(path),
                created_ms: *created,
            })
            .collect(),
    );
    (index, links)
}

/// Обратные ссылки строкой: «откуда: строки».
fn sources(index: &VaultIndex, links: &Links, note: &str) -> Vec<(String, Vec<u32>)> {
    index
        .backlinks(&vp(note), links)
        .into_iter()
        .map(|b| {
            (
                b.from.components().join("/"),
                b.sites.iter().map(|s| s.line).collect(),
            )
        })
        .collect()
}

fn tag_list(index: &VaultIndex) -> Vec<(String, String, u32)> {
    let mut tags: Vec<_> = index
        .tags()
        .into_iter()
        .map(|t| (t.key, t.display, t.notes))
        .collect();
    tags.sort();
    tags
}

#[test]
fn backlinks_of_note() {
    let text_a = "[[Б]] в начале\nтекст\nещё [[б|псевдоним]] и [[Другое]]";
    let (index, links) = index_of(&[
        ("А.md", text_a, 1),
        ("Б.md", "текст", 2),
        ("Папка/В.md", "\n[[Б]]", 3),
        ("Г.md", "[[Другое]] [[Нет такой]]", 4),
    ]);
    assert_eq!(
        sources(&index, &links, "Б.md"),
        vec![("А.md".into(), vec![1, 3]), ("Папка/В.md".into(), vec![2])]
    );
    // Положение — вся ссылка в байтах текста.
    let a = &index.backlinks(&vp("Б.md"), &links)[0];
    assert_eq!(&text_a[a.sites[0].span.clone()], "[[Б]]");
    assert_eq!(&text_a[a.sites[1].span.clone()], "[[б|псевдоним]]");
    assert!(sources(&index, &links, "Г.md").is_empty());
    assert!(sources(&index, &links, "Нет.md").is_empty());
}

#[test]
fn backlinks_follow_link_resolution() {
    // Две «Встречи»: `[[Встреча]]` ведёт к самой старой, путь — к своей.
    let (index, links) = index_of(&[
        ("Альфа/Встреча.md", "", 10),
        ("Бета/Встреча.md", "", 5),
        ("А.md", "[[Встреча]]", 1),
        ("Б.md", "[[Альфа/Встреча]]", 1),
        ("В.md", "[[/Бета/Встреча]]", 1),
    ]);
    assert_eq!(
        sources(&index, &links, "Бета/Встреча.md"),
        vec![("А.md".into(), vec![1]), ("В.md".into(), vec![1])]
    );
    assert_eq!(
        sources(&index, &links, "Альфа/Встреча.md"),
        vec![("Б.md".into(), vec![1])]
    );
    // Список заметок поменялся (старшая удалена) — ссылки ведут к другой
    // без пересчёта индекса.
    let links = Links::new(vec![
        NoteRef {
            path: vp("Альфа/Встреча.md"),
            created_ms: 10,
        },
        NoteRef {
            path: vp("А.md"),
            created_ms: 1,
        },
        NoteRef {
            path: vp("Б.md"),
            created_ms: 1,
        },
    ]);
    assert_eq!(
        sources(&index, &links, "Альфа/Встреча.md"),
        vec![("А.md".into(), vec![1]), ("Б.md".into(), vec![1])]
    );
}

#[test]
fn self_link_not_counted() {
    let (index, links) = index_of(&[("Б.md", "[[Б]] и [[Б#Раздел]]", 1), ("А.md", "[[Б]]", 2)]);
    assert_eq!(
        sources(&index, &links, "Б.md"),
        vec![("А.md".into(), vec![1])]
    );
}

#[test]
fn heading_link_is_backlink() {
    let (index, links) = index_of(&[("Б.md", "# Раздел", 1), ("А.md", "[[Б#Раздел|тут]]", 2)]);
    assert_eq!(
        sources(&index, &links, "Б.md"),
        vec![("А.md".into(), vec![1])]
    );
}

#[test]
fn embed_is_backlink() {
    let (index, links) = index_of(&[("Б.md", "", 1), ("А.md", "x\n![[Б]]", 2)]);
    assert_eq!(
        sources(&index, &links, "Б.md"),
        vec![("А.md".into(), vec![2])]
    );
    let site = &index.backlinks(&vp("Б.md"), &links)[0].sites[0];
    assert_eq!(&"x\n![[Б]]"[site.span.clone()], "![[Б]]");
}

#[test]
fn link_in_code_or_frontmatter_ignored() {
    let text = "---\ntags: [x]\nlink: \"[[Б]]\"\n---\n`[[Б]]`\n\n```\n[[Б]]\n```\n\n    [[Б]]\n";
    let (index, links) = index_of(&[("Б.md", "", 1), ("А.md", text, 2)]);
    assert!(sources(&index, &links, "Б.md").is_empty());
}

#[test]
fn links_named_lists_links_by_target_name_key() {
    let (index, _) = index_of(&[
        ("А.md", "[[Папка/встреча]] [[Другое]]", 1),
        ("Б.md", "[[ВСТРЕЧА#x]]", 2),
    ]);
    let mut got: Vec<(String, String)> = index
        .links_named(&crate::names::key("Встреча"))
        .map(|(from, link)| (from.components().join("/"), link.target.clone()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            ("А.md".into(), "Папка/встреча".into()),
            ("Б.md".into(), "ВСТРЕЧА".into())
        ]
    );
}

#[test]
fn tag_case_folds_to_one() {
    let (index, _) = index_of(&[
        ("А.md", "#Проект и #проект", 1),
        ("Б.md", "---\ntags: [ПРОЕКТ]\n---\n#идея", 2),
        ("В.md", "#ё #е", 3),
    ]);
    let tags = tag_list(&index);
    let keys: Vec<&str> = tags.iter().map(|t| t.0.as_str()).collect();
    // Порядок байтов: «ё» (U+0451) после «я».
    assert_eq!(keys, vec!["е", "идея", "проект", "ё"]);
    assert_eq!(tags[2].2, 2);
    let mut notes: Vec<String> = index
        .tag_notes("проект")
        .iter()
        .map(|p| p.components().join("/"))
        .collect();
    notes.sort();
    assert_eq!(notes, vec!["А.md", "Б.md"]);
    assert!(index.tag_notes("нет").is_empty());
}

#[test]
fn tag_display_most_frequent_then_oldest() {
    // Самое частое по вхождениям во всём хранилище.
    let (index, _) = index_of(&[("А.md", "#Проект", 1), ("Б.md", "#проект #проект", 2)]);
    assert_eq!(tag_list(&index)[0].1, "проект");

    // Поровну — из самой старой заметки.
    let (index, _) = index_of(&[("А.md", "#проект", 5), ("Б.md", "#Проект", 2)]);
    assert_eq!(tag_list(&index)[0].1, "Проект");

    // Даты равны — по пути.
    let (index, _) = index_of(&[("Б.md", "#проект", 2), ("А.md", "#Проект", 2)]);
    assert_eq!(tag_list(&index)[0].1, "Проект");

    // В одной заметке оба написания — первое по порядку (блок свойств, затем
    // текст).
    let (index, _) = index_of(&[("А.md", "---\ntags: [ПРОЕКТ]\n---\n#проект", 1)]);
    assert_eq!(tag_list(&index)[0].1, "ПРОЕКТ");

    // Выбор только среди равных по максимуму: самая старая заметка с редким
    // написанием не решает.
    let (index, _) = index_of(&[
        ("Старая.md", "#ПРОЕКТ", 1),
        ("А.md", "#Проект #проект", 5),
        ("Б.md", "#проект #Проект", 3),
    ]);
    assert_eq!(tag_list(&index)[0].1, "проект");

    // Составная и разложенная «й» — одно написание, показывается NFC.
    let (index, _) = index_of(&[
        ("А.md", "#Мой", 9),
        ("Б.md", "#Мои\u{306}", 1),
        ("В.md", "#мой", 2),
    ]);
    assert_eq!(tag_list(&index)[0].1, "Мой");
    assert_eq!(tag_list(&index)[0].2, 3);
}

#[test]
fn tag_without_notes_disappears() {
    let (mut index, _) = index_of(&[("А.md", "#раз #два", 1), ("Б.md", "#раз", 2)]);
    index.upsert(vp("А.md"), entry("#раз", 1));
    assert_eq!(tag_list(&index), vec![("раз".into(), "раз".into(), 2)]);
    index.remove(&vp("А.md"));
    index.remove(&vp("Б.md"));
    assert!(index.tags().is_empty());
    assert!(index.tag_notes("раз").is_empty());
}

#[test]
fn revision_changes_only_with_content() {
    let mut index = VaultIndex::new();
    let start = index.revision();
    index.upsert(vp("А.md"), entry("#тег", 1));
    let after = index.revision();
    assert_ne!(after, start);
    index.upsert(vp("А.md"), entry("#тег", 1));
    assert_eq!(index.revision(), after);
    index.remove(&vp("Нет.md"));
    assert_eq!(index.revision(), after);
    index.remove(&vp("А.md"));
    assert_ne!(index.revision(), after);
}

#[test]
fn snapshot_restores_same_index() {
    let (index, links) = index_of(&[("А.md", "[[Б]] #тег", 1), ("Б.md", "#Тег", 2)]);
    let restored = VaultIndex::from_entries(
        index
            .entries()
            .map(|(path, entry)| (path.clone(), entry.clone())),
    );
    assert_eq!(restored.status().state, IndexState::Ready);
    assert!(restored.same_content(&index));
    assert_eq!(
        sources(&restored, &links, "Б.md"),
        vec![("А.md".into(), vec![1])]
    );
    assert_eq!(VaultIndex::new().status().state, IndexState::Building);
}

// --- Изменения по одному против сборки с нуля ---------------------------

const NAMES: [&str; 5] = ["А", "Б", "Папка/А", "Папка/Встреча", "в"];

fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        (0..NAMES.len()).prop_map(|i| format!("[[{}]]", NAMES[i])),
        (0..NAMES.len()).prop_map(|i| format!("![[{}#Заголовок]]", NAMES[i])),
        (0..NAMES.len()).prop_map(|i| format!("`[[{}]]`", NAMES[i])),
        prop::sample::select(vec!["#Тег", "#тег", "#ТЕГ", "#Мои\u{306}", "#мой", "#x/y"])
            .prop_map(str::to_owned),
        Just("---\ntags: [Тег, мой]\n---\n".to_owned()),
        Just("# Заголовок".to_owned()),
        Just("```\n#тег [[А]]\n```".to_owned()),
        Just("текст".to_owned()),
    ]
}

fn note_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (fragment(), prop::sample::select(vec![" ", "\n", "\n\n"])),
        0..6,
    )
    .prop_map(|parts| {
        parts
            .into_iter()
            .map(|(f, sep)| format!("{f}{sep}"))
            .collect()
    })
}

#[derive(Debug, Clone)]
enum Op {
    Upsert(usize, String, i64),
    Remove(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..NAMES.len(), note_text(), 0i64..3).prop_map(|(i, t, c)| Op::Upsert(i, t, c)),
        1 => (0..NAMES.len()).prop_map(Op::Remove),
    ]
}

fn note_path(i: usize) -> VaultPath {
    vp(&format!("{}.md", NAMES[i]))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn incremental_equals_full_rebuild(ops in prop::collection::vec(op(), 0..40)) {
        let mut incremental = VaultIndex::new();
        let mut final_notes: BTreeMap<usize, NoteEntry> = BTreeMap::new();
        for op in ops {
            match op {
                Op::Upsert(i, text, created) => {
                    let e = entry(&text, created);
                    incremental.upsert(note_path(i), e.clone());
                    final_notes.insert(i, e);
                }
                Op::Remove(i) => {
                    incremental.remove(&note_path(i));
                    final_notes.remove(&i);
                }
            }
        }
        let full = VaultIndex::from_entries(
            final_notes.iter().map(|(i, e)| (note_path(*i), e.clone())),
        );
        prop_assert!(incremental.same_content(&full));
        prop_assert_eq!(tag_list(&incremental), tag_list(&full));
        let links = Links::new(
            final_notes
                .iter()
                .map(|(i, e)| NoteRef { path: note_path(*i), created_ms: e.created_ms })
                .collect(),
        );
        for i in 0..NAMES.len() {
            prop_assert_eq!(
                incremental.backlinks(&note_path(i), &links),
                full.backlinks(&note_path(i), &links)
            );
        }
        for tag in full.tags() {
            let mut a = incremental.tag_notes(&tag.key);
            let mut b = full.tag_notes(&tag.key);
            a.sort_by(|x, y| x.components().cmp(y.components()));
            b.sort_by(|x, y| x.components().cmp(y.components()));
            prop_assert_eq!(a, b);
        }
    }
}

// --- Сборка с диска -------------------------------------------------------

struct Disk {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    vault: Vault,
}

fn disk(notes: &[(&str, &str)]) -> Disk {
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v");
    std::fs::create_dir(&root).expect("корень");
    for (path, text) in notes {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().expect("папка")).expect("папка");
        std::fs::write(file, text).expect("заметка");
    }
    let vault = Vault::open(&root).expect("хранилище");
    Disk {
        _dir: dir,
        root,
        vault,
    }
}

#[test]
fn read_entry_parses_note_from_disk() {
    let d = disk(&[("Папка/А.md", "\u{feff}строка\r\n[[Б]] #тег\r\n")]);
    let note = NoteRef {
        path: vp("Папка/А.md"),
        created_ms: 42,
    };
    let e = read_entry(&d.vault, &note).expect("прочитана");
    assert_eq!(e.created_ms, 42);
    // Размер — файла на диске: BOM и `\r\n` входят.
    assert_eq!(e.size, 33);
    assert!(e.modified_ms > 0);
    assert_eq!(e.parsed.links.len(), 1);
    assert_eq!(e.parsed.links[0].line, 2);
    // Положения — в тексте, каким его видит редактор: без BOM, переносы `\n`.
    assert_eq!(e.parsed.links[0].span, 13..19);
    assert_eq!(e.parsed.tags[0].name, "тег");
    assert!(
        read_entry(
            &d.vault,
            &NoteRef {
                path: vp("Нет.md"),
                created_ms: 0
            }
        )
        .is_err()
    );
}

#[test]
fn rebuild_indexes_vault_and_reports_progress() {
    let mut notes: Vec<(String, String)> = (0..120)
        .map(|i| {
            (
                format!("Папка{}/З{i}.md", i % 7),
                format!("[[Узел]] #тег{}", i % 3),
            )
        })
        .collect();
    notes.push(("Узел.md".into(), "#Тег0".into()));
    notes.push(("картинка.png".into(), "[[Узел]]".into()));
    let refs: Vec<(&str, &str)> = notes
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let d = disk(&refs);
    let index = RwLock::new(VaultIndex::new());
    let seen = Mutex::new(Vec::new());
    rebuild(&d.vault, &index, &|s| seen.lock().expect("ход").push(s)).expect("собран");

    let index = index.read().expect("индекс");
    let status = index.status();
    assert_eq!(status.state, IndexState::Ready);
    assert_eq!((status.done, status.total), (121, 121));
    let seen = seen.into_inner().expect("ход");
    assert!(!seen.is_empty());
    assert!(seen.windows(2).all(|w| w[0].done <= w[1].done));
    assert!(seen.iter().all(|s| s.total == 121 || s.total == 0));
    assert_eq!(seen.last().expect("последний").state, IndexState::Ready);

    let links = Links::new(d.vault.notes().expect("заметки"));
    assert_eq!(index.backlinks(&vp("Узел.md"), &links).len(), 120);
    assert_eq!(
        tag_list(&index),
        vec![
            ("тег0".into(), "тег0".into(), 41),
            ("тег1".into(), "тег1".into(), 40),
            ("тег2".into(), "тег2".into(), 40),
        ]
    );
}

#[test]
fn rebuild_drops_notes_gone_from_disk() {
    let d = disk(&[("А.md", "#тег")]);
    let index = RwLock::new(VaultIndex::from_entries([(
        vp("Старая.md"),
        entry("#старый", 1),
    )]));
    rebuild(&d.vault, &index, &|_| {}).expect("собран");
    let index = index.read().expect("индекс");
    let paths: Vec<String> = index
        .entries()
        .map(|(p, _)| p.components().join("/"))
        .collect();
    assert_eq!(paths, vec!["А.md"]);
    assert_eq!(tag_list(&index), vec![("тег".into(), "тег".into(), 1)]);
}

#[test]
fn update_during_build_wins_over_build() {
    // Наблюдатель обновил и удалил заметки, пока шла сборка: прочитанное
    // сборкой раньше не перекрывает их.
    let d = disk(&[("А.md", "#старый"), ("Б.md", "#б"), ("В.md", "#в")]);
    let index = RwLock::new(VaultIndex::new());
    let first = std::sync::atomic::AtomicBool::new(true);
    rebuild(&d.vault, &index, &|s| {
        if s.state == IndexState::Building
            && s.total > 0
            && first.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let mut index = index.write().expect("индекс");
            index.upsert(vp("А.md"), entry("#новый", 1));
            index.remove(&vp("Б.md"));
        }
    })
    .expect("собран");
    // Изменения наблюдателя легли до вставки первой пачки сборки (ход с
    // `total` сообщается до чтения), но сборка их не перекрыла.
    let index = index.read().expect("индекс");
    let keys: Vec<String> = tag_list(&index).into_iter().map(|t| t.0).collect();
    assert_eq!(keys, vec!["в", "новый"]);
}

#[test]
fn rebuild_of_missing_vault_is_error_and_stays_building() {
    let d = disk(&[("А.md", "")]);
    std::fs::remove_dir_all(&d.root).expect("удалена");
    let index = RwLock::new(VaultIndex::new());
    assert!(rebuild(&d.vault, &index, &|_| {}).is_err());
    assert_eq!(
        index.read().expect("индекс").status().state,
        IndexState::Building
    );
}

// --- Хранилище генератора ----------------------------------------------

/// Сводка генератора: обратные ссылки «узла» и теги (число заметок и
/// отображаемое написание) совпадают с индексом.
#[test]
fn generated_vault_matches_manifest() {
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v");
    let manifest = gen_vault::generate(&root, &gen_vault::Config::new(400, 7)).expect("генератор");
    let vault = Vault::open(&root).expect("хранилище");
    let index = RwLock::new(VaultIndex::new());
    rebuild(&vault, &index, &|_| {}).expect("собран");
    let index = index.read().expect("индекс");
    assert_eq!(index.entries().count(), manifest.notes);

    let hub = manifest.hub.as_ref().expect("узел");
    let links = Links::new(vault.notes().expect("заметки"));
    let got: BTreeSet<String> = index
        .backlinks(&vp(&hub.path), &links)
        .iter()
        .map(|b| b.from.components().join("/"))
        .collect();
    let want: BTreeSet<String> = hub.backlinks.iter().map(|b| b.source.clone()).collect();
    assert_eq!(got.len(), 100);
    assert_eq!(got, want);

    let got: BTreeSet<(String, usize, String)> = index
        .tags()
        .into_iter()
        .map(|t| (t.key, t.notes as usize, t.display))
        .collect();
    let want: BTreeSet<(String, usize, String)> = manifest
        .tags
        .iter()
        .map(|t| (t.key.clone(), t.notes, t.display.clone()))
        .collect();
    assert_eq!(got, want);
    let unique: HashSet<&str> = manifest.tags.iter().map(|t| t.key.as_str()).collect();
    assert_eq!(unique.len(), manifest.tags.len());
}

/// Замер (задача 0.2-1.2): сборка индекса хранилища генератора на 10 000
/// заметок — не больше 5 с (справочно на Linux; предел — в 0.2-7.1). Запуск:
/// `cargo test -p rhizomind --lib index::tests::index_build_10k -- --ignored --nocapture`.
#[test]
#[ignore = "замер: --ignored"]
fn index_build_10k() {
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v10k");
    gen_vault::generate(&root, &gen_vault::Config::new(10_000, 7)).expect("генератор");
    let vault = Vault::open(&root).expect("хранилище");
    let index = RwLock::new(VaultIndex::new());
    let start = std::time::Instant::now();
    rebuild(&vault, &index, &|_| {}).expect("собран");
    let elapsed = start.elapsed();
    let index = index.read().expect("индекс");
    let status = index.status();
    println!(
        "index_build_10k: {} заметок, тегов {}, {:?} ({}, потоков {})",
        status.total,
        index.tags().len(),
        elapsed,
        if cfg!(debug_assertions) {
            "отладочная сборка"
        } else {
            "выпускная сборка"
        },
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    );
    assert_eq!(status.state, IndexState::Ready);
    assert!(status.total >= 10_000);
    if !cfg!(debug_assertions) {
        assert!(elapsed <= std::time::Duration::from_secs(5), "{elapsed:?}");
    }
}
