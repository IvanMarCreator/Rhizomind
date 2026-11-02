//! Тесты 0.2-4.3: план переписывания по правилу «ссылка ведёт туда же».
//!
//! План проверяется вместе с правкой байтов (`rewrite`, 0.2-4.4): тексты
//! заметок после плана сравниваются с ожидаемыми, а свойства — по ссылкам
//! переписанных текстов в списке заметок «после».

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::*;
use crate::commands::VaultPath;
use crate::index::NoteEntry;
use crate::markdown;
use crate::names;
use crate::relink::{NotePlan, PlannedLink, rewrite};
use crate::vault::NoteRef;

fn p(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

fn text_of(path: &VaultPath) -> String {
    path.components().join("/")
}

fn rename(from: &str, to: &str) -> PathRename {
    PathRename {
        from: p(from),
        to: p(to),
    }
}

/// Путь после переименований, выполненных по очереди (как события на
/// диске): заметка или папка `from` и всё внутри неё.
fn moved(path: &str, renames: &[PathRename]) -> String {
    let mut path = path.to_owned();
    for r in renames {
        let (from, to) = (text_of(&r.from), text_of(&r.to));
        if path == from {
            path = to;
        } else if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
            path = format!("{to}/{rest}");
        }
    }
    path
}

/// Хранилище теста: заметки «до» — путь, дата создания, текст.
#[derive(Debug, Clone)]
struct Vault {
    notes: Vec<(String, i64, String)>,
}

impl Vault {
    fn new(notes: &[(&str, i64, &str)]) -> Self {
        Self {
            notes: notes
                .iter()
                .map(|(path, created, text)| ((*path).to_owned(), *created, (*text).to_owned()))
                .collect(),
        }
    }

    /// Индекс, как его строит `read_entry`: текст без BOM, переносы `\n`.
    fn index(&self) -> VaultIndex {
        VaultIndex::from_entries(self.notes.iter().map(|(path, created, text)| {
            let text = text.strip_prefix('\u{feff}').unwrap_or(text);
            let text = text.replace("\r\n", "\n");
            (
                p(path),
                NoteEntry {
                    size: text.len() as u64,
                    modified_ms: *created,
                    created_ms: *created,
                    parsed: markdown::parse(&text),
                },
            )
        }))
    }

    fn links(&self) -> Links {
        Links::new(
            self.notes
                .iter()
                .map(|(path, created, _)| NoteRef {
                    path: p(path),
                    created_ms: *created,
                })
                .collect(),
        )
    }

    fn plan(&self, renames: &[PathRename]) -> RelinkPlan {
        plan(&self.index(), &self.links(), renames)
    }

    /// Хранилище после переименований и выполненного плана: у каждой
    /// заметки плана цели переписаны `rewrite` по таблице «цель до → цель
    /// после» этой заметки (одна цель в одной заметке ведёт в одно место,
    /// поэтому таблица однозначна).
    fn apply(&self, renames: &[PathRename], plan: &RelinkPlan) -> Vault {
        for note in &plan.notes {
            assert!(
                self.notes
                    .iter()
                    .any(|(path, ..)| p(&moved(path, renames)) == note.path),
                "в плане заметка не из хранилища: {:?}",
                note.path
            );
            assert!(!note.links.is_empty(), "заметка без ссылок в плане");
        }
        let notes = self
            .notes
            .iter()
            .map(|(path, created, text)| {
                let after = moved(path, renames);
                let text = match plan.notes.iter().find(|n| n.path == p(&after)) {
                    Some(note) => {
                        let mut table: HashMap<&str, &str> = HashMap::new();
                        for link in &note.links {
                            let new = table.entry(&link.old_target).or_insert(&link.new_target);
                            assert_eq!(*new, link.new_target, "одна цель — два ответа");
                        }
                        rewrite(text, &|link| {
                            table.get(link.target.as_str()).map(|t| (*t).to_owned())
                        })
                        .expect("план меняет текст")
                    }
                    None => text.clone(),
                };
                (after, *created, text)
            })
            .collect();
        Vault { notes }
    }

    fn text(&self, path: &str) -> &str {
        &self
            .notes
            .iter()
            .find(|(p, ..)| p == path)
            .unwrap_or_else(|| panic!("нет заметки {path}"))
            .2
    }
}

fn planned(old: &str, new: &str, points_to: &str) -> PlannedLink {
    PlannedLink {
        old_target: old.to_owned(),
        new_target: new.to_owned(),
        points_to: p(points_to),
    }
}

/// Текст `Заметка.md` после операции и плана.
fn after_text(vault: &Vault, renames: &[PathRename]) -> String {
    let plan = vault.plan(renames);
    vault.apply(renames, &plan).text("Заметка.md").to_owned()
}

/// Строка таблицы С2-3.5: текст `Заметка.md` до; после переименования
/// «Встреча» → «Планёрка»; после переноса `Альфа/Встреча.md` в `Бета/`, когда
/// имя повторяется (образец 06: даты равны, без правки ссылка без пути
/// перешла бы на `Арбуз/Встреча`, Р17 п. 9).
fn table_row(before: &str, renamed: &str, moved_dup: &str) {
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Заметка.md", 2, before),
        ("Другая.md", 3, "[[Другая]]"),
    ]);
    let op = [rename("Альфа/Встреча.md", "Альфа/Планёрка.md")];
    let plan = vault.plan(&op);
    assert_eq!(plan.note_count(), 1, "{plan:?}");
    assert_eq!(plan.notes[0].path, p("Заметка.md"));
    assert!(
        plan.notes[0]
            .links
            .iter()
            .all(|l| l.points_to == p("Альфа/Планёрка.md"))
    );
    assert_eq!(vault.apply(&op, &plan).text("Заметка.md"), renamed);

    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Арбуз/Встреча.md", 1, ""),
        ("Заметка.md", 2, before),
    ]);
    let op = [rename("Альфа/Встреча.md", "Бета/Встреча.md")];
    let plan = vault.plan(&op);
    assert!(
        plan.notes[0]
            .links
            .iter()
            .all(|l| l.points_to == p("Бета/Встреча.md") && l.new_target == "Бета/Встреча")
    );
    assert_eq!(vault.apply(&op, &plan).text("Заметка.md"), moved_dup);
}

#[test]
fn rename_plain() {
    table_row("[[Встреча]]", "[[Планёрка]]", "[[Бета/Встреча]]");
}

#[test]
fn rename_alias() {
    table_row(
        "[[Встреча|встречи]]",
        "[[Планёрка|встречи]]",
        "[[Бета/Встреча|встречи]]",
    );
}

#[test]
fn rename_heading() {
    table_row(
        "[[Встреча#Итоги]]",
        "[[Планёрка#Итоги]]",
        "[[Бета/Встреча#Итоги]]",
    );
}

#[test]
fn rename_path_heading_alias() {
    table_row(
        "[[Альфа/Встреча#Итоги|итоги]]",
        "[[Планёрка#Итоги|итоги]]",
        "[[Бета/Встреча#Итоги|итоги]]",
    );
}

#[test]
fn rename_embed() {
    table_row("![[Встреча]]", "![[Планёрка]]", "![[Бета/Встреча]]");
}

#[test]
fn move_duplicate_gets_path() {
    // Образец 06: все строки таблицы и ссылка на другую заметку.
    let before = "[[Встреча]]\n[[Встреча|встречи]]\n[[Встреча#Итоги]]\n\
                  [[Альфа/Встреча#Итоги|итоги]]\n![[Встреча]]\n[[Арбуз/Встреча]]\n";
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Арбуз/Встреча.md", 1, ""),
        ("Заметка.md", 2, before),
    ]);
    let op = [rename("Альфа/Встреча.md", "Бета/Встреча.md")];
    let plan = vault.plan(&op);
    assert_eq!((plan.note_count(), plan.link_count()), (1, 5));
    assert_eq!(
        after_text(&vault, &op),
        "[[Бета/Встреча]]\n[[Бета/Встреча|встречи]]\n[[Бета/Встреча#Итоги]]\n\
         [[Бета/Встреча#Итоги|итоги]]\n![[Бета/Встреча]]\n[[Арбуз/Встреча]]\n"
    );

    // Даты разные (образец 20): самая старая остаётся целью ссылок без
    // пути — меняется только ссылка с путём через старую папку.
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Арбуз/Встреча.md", 2, ""),
        ("Заметка.md", 3, before),
    ]);
    assert_eq!(
        vault.plan(&op).notes[0].links,
        vec![planned("Альфа/Встреча", "Бета/Встреча", "Бета/Встреча.md")]
    );
}

#[test]
fn unique_move_changes_nothing() {
    let text = "[[Встреча]] и [[Встреча#Итоги|итоги]] ![[Встреча]] [[встреча.md]] [[ Встреча ]]";
    let vault = Vault::new(&[("Альфа/Встреча.md", 1, ""), ("Заметка.md", 2, text)]);
    assert!(
        vault
            .plan(&[rename("Альфа/Встреча.md", "Бета/Встреча.md")])
            .is_empty()
    );
    // Перенос папки с уникальными именами внутри.
    assert!(vault.plan(&[rename("Альфа", "Бета/Альфа")]).is_empty());
}

#[test]
fn case_only_rename_changes_nothing() {
    let text = "[[Встреча]] [[Альфа/Встреча]] [[/Альфа/Встреча#x]] [[Мой]]";
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Альфа/Встреча копия.md", 1, ""),
        ("Мой.md", 1, ""),
        ("Заметка.md", 2, text),
    ]);
    for op in [
        rename("Альфа/Встреча.md", "Альфа/встреча.md"),
        rename("Альфа", "альфа"),
        // Составная «й» → разложенная: тот же ключ имени.
        rename("Мой.md", "Мои\u{306}.md"),
    ] {
        assert!(vault.plan(std::slice::from_ref(&op)).is_empty(), "{op:?}");
    }
}

#[test]
fn other_note_would_jump_is_rewritten() {
    // Образец 12: более старую заметку переименовали в «Встреча».
    let vault = Vault::new(&[
        ("Альфа/Старая.md", 1, ""),
        ("Бета/Встреча.md", 2, ""),
        ("Заметка.md", 3, "[[Встреча]] и [[Старая|старая]]"),
    ]);
    let op = [rename("Альфа/Старая.md", "Альфа/Встреча.md")];
    let plan = vault.plan(&op);
    assert_eq!(
        plan.notes,
        vec![NotePlan {
            path: p("Заметка.md"),
            links: vec![
                planned("Встреча", "Бета/Встреча", "Бета/Встреча.md"),
                planned("Старая", "Альфа/Встреча", "Альфа/Встреча.md"),
            ],
        }]
    );
    assert_eq!(
        after_text(&vault, &op),
        "[[Бета/Встреча]] и [[Альфа/Встреча|старая]]"
    );
}

#[test]
fn folder_rename_only_paths_through_it() {
    let text = "[[Альфа/Встреча]] [[Встреча]] [[/Альфа/План#Цели]] [[План]] \
                [[Бета/Встреча]] [[Отчёт]] [[Вложенная/Итог]] [[Итог]]";
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Альфа/План.md", 2, ""),
        ("Альфа/Вложенная/Итог.md", 2, ""),
        ("Бета/Встреча.md", 3, ""),
        ("Бета/Отчёт.md", 4, ""),
        ("Заметка.md", 5, text),
    ]);
    let op = [rename("Альфа", "Омега")];
    assert_eq!(vault.plan(&op).link_count(), 2);
    assert_eq!(
        after_text(&vault, &op),
        "[[Омега/Встреча]] [[Встреча]] [[План#Цели]] [[План]] \
         [[Бета/Встреча]] [[Отчёт]] [[Вложенная/Итог]] [[Итог]]"
    );
    // Вложенная папка: переписывается только ссылка с путём через неё.
    let op = [rename("Альфа/Вложенная", "Альфа/Новая")];
    assert_eq!(
        after_text(&vault, &op),
        "[[Альфа/Встреча]] [[Встреча]] [[/Альфа/План#Цели]] [[План]] \
         [[Бета/Встреча]] [[Отчёт]] [[Итог]] [[Итог]]"
    );
}

#[test]
fn duplicate_appears_clarifies() {
    // Образец 16: путь одной — конец пути другой, поэтому `/Альфа/Встреча`.
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Х/Альфа/Старое.md", 2, ""),
        ("Заметка.md", 3, "[[Встреча]] [[Альфа/Встреча]] [[Старое]]"),
    ]);
    let op = [rename("Х/Альфа/Старое.md", "Х/Альфа/Встреча.md")];
    assert_eq!(
        after_text(&vault, &op),
        "[[/Альфа/Встреча]] [[/Альфа/Встреча]] [[Х/Альфа/Встреча]]"
    );
    // С2-3.6: ссылка всё ещё ведёт на самую старую, но стала неоднозначной.
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Бета/Старое.md", 2, ""),
        ("Заметка.md", 3, "[[Встреча|встречи]]"),
    ]);
    let op = [rename("Бета/Старое.md", "Бета/Встреча.md")];
    assert_eq!(after_text(&vault, &op), "[[Альфа/Встреча|встречи]]");
}

#[test]
fn path_link_becomes_ambiguous_clarifies() {
    for (old_date, new_date) in [(1, 2), (2, 1)] {
        let vault = Vault::new(&[
            ("Альфа/Встреча.md", old_date, ""),
            ("Х/Альфа/Старое.md", new_date, ""),
            ("Заметка.md", 3, "[[Альфа/Встреча]]"),
        ]);
        let op = [rename("Х/Альфа/Старое.md", "Х/Альфа/Встреча.md")];
        assert_eq!(after_text(&vault, &op), "[[/Альфа/Встреча]]");
    }
}

#[test]
fn chain_collapsed() {
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Арбуз/Встреча.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[Альфа/Встреча]]"),
    ]);
    // А → Б, Б → В — как одно А → В.
    let chain = [
        rename("Альфа/Встреча.md", "Альфа/Промежуток.md"),
        rename("Альфа/Промежуток.md", "Альфа/Планёрка.md"),
    ];
    let plan = vault.plan(&chain);
    assert_eq!(
        plan.notes[0].links,
        vec![
            planned("Встреча", "Планёрка", "Альфа/Планёрка.md"),
            planned("Альфа/Встреча", "Планёрка", "Альфа/Планёрка.md"),
        ]
    );
    // Туда и обратно — ничего.
    let back = [
        rename("Альфа/Встреча.md", "Альфа/Промежуток.md"),
        rename("Альфа/Промежуток.md", "Альфа/Встреча.md"),
    ];
    assert!(vault.plan(&back).is_empty());
    // Обмен именами через временное: А → tmp, Б → А, tmp → Б.
    let vault = Vault::new(&[
        ("Встреча.md", 1, ""),
        ("План.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[План]]"),
    ]);
    let swap = [
        rename("Встреча.md", "tmp.md"),
        rename("План.md", "Встреча.md"),
        rename("tmp.md", "План.md"),
    ];
    assert_eq!(after_text(&vault, &swap), "[[План]] [[Встреча]]");
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Арбуз/Встреча.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[Альфа/Встреча]]"),
    ]);
    // Перенос заметки, затем переименование папки, куда она попала.
    let chain = [
        rename("Альфа/Встреча.md", "Бета/Встреча.md"),
        rename("Бета", "Гамма"),
    ];
    assert_eq!(
        after_text(&vault, &chain),
        "[[Гамма/Встреча]] [[Гамма/Встреча]]"
    );
    // Переименование папки, затем заметки внутри неё.
    let chain = [
        rename("Альфа", "Бета"),
        rename("Бета/Встреча.md", "Бета/Планёрка.md"),
    ];
    assert_eq!(after_text(&vault, &chain), "[[Планёрка]] [[Планёрка]]");
}

#[test]
fn rename_spelled_differently_matched_by_key() {
    // Путь «откуда» в другом регистре или в разложенной форме (событие ОС)
    // — сопоставляется по ключу, тихого пустого плана нет.
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Альфа/Мой.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[Альфа/Мой]]"),
    ]);
    assert_eq!(
        after_text(&vault, &[rename("альфа/ВСТРЕЧА.md", "Альфа/Планёрка.md")]),
        "[[Планёрка]] [[Альфа/Мой]]"
    );
    assert_eq!(
        after_text(&vault, &[rename("Альфа/Мои\u{306}.md", "Альфа/Твой.md")]),
        "[[Встреча]] [[Твой]]"
    );
    assert_eq!(
        after_text(&vault, &[rename("АЛЬФА", "Омега")]),
        "[[Встреча]] [[Мой]]"
    );
}

#[test]
fn same_key_names_side_by_side() {
    // Linux: `А.md` и `а.md` рядом — точное написание главнее ключа.
    let vault = Vault::new(&[
        ("Альфа/А.md", 1, ""),
        ("Альфа/а.md", 2, ""),
        ("Заметка.md", 3, "[[А]]"),
    ]);
    assert!(vault.plan(&[rename("Альфа/а.md", "Альфа/Б.md")]).is_empty());
    assert_eq!(
        vault.plan(&[rename("Альфа/А.md", "Альфа/Б.md")]).notes[0].links,
        vec![planned("А", "Б", "Альфа/Б.md")]
    );
}

#[test]
fn move_to_non_note_skipped() {
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Бета/Встреча.md", 2, ""),
        ("Заметка.md", 3, "[[Встреча]] [[Альфа/Встреча]]"),
    ]);
    assert!(
        vault
            .plan(&[rename("Альфа/Встреча.md", "Альфа/Встреча.txt")])
            .is_empty()
    );
}

#[test]
fn replayed_rename_changes_nothing() {
    // Переименование, которое уже выполнено: «куда» уже есть в списке, а
    // «откуда» по ключу совпало бы с другой заметкой — её не трогаем.
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, ""),
        ("Альфа/Планёрка.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[Планёрка]]"),
    ]);
    assert!(
        vault
            .plan(&[rename("Альфа/встреча.md", "Альфа/Планёрка.md")])
            .is_empty()
    );
}

#[test]
fn name_ending_in_md_keeps_suffix() {
    // Заметка `Итоги.md.md`: цель `Итоги.md.md`, а не `Итоги.md` (ревью 4.4).
    let vault = Vault::new(&[
        ("Встреча.md", 1, ""),
        ("Итоги.md.md", 1, ""),
        ("Заметка.md", 2, "[[Встреча]] [[Итоги.md.md]]"),
    ]);
    let op = [rename("Встреча.md", "Сводка.md.md")];
    assert_eq!(
        vault.plan(&op).notes[0].links,
        vec![planned("Встреча", "Сводка.md.md", "Сводка.md.md")]
    );
    assert_eq!(after_text(&vault, &op), "[[Сводка.md.md]] [[Итоги.md.md]]");
    let op = [rename("Итоги.md.md", "Отчёт.md.md")];
    assert_eq!(after_text(&vault, &op), "[[Встреча]] [[Отчёт.md.md]]");
}

#[test]
fn plan_lists_notes_by_path_and_links_in_text_order() {
    let vault = Vault::new(&[
        ("Альфа/Встреча.md", 1, "Сама на себя: [[Встреча#Итоги]]"),
        ("Я.md", 2, "[[Встреча]] [[Другое]] [[Встреча|в]]"),
        (
            "Б.md",
            2,
            "![[Альфа/Встреча]]\n\n`[[Встреча]]`\n[[Встреча]]",
        ),
        ("Другое.md", 3, ""),
    ]);
    let plan = vault.plan(&[
        rename("Альфа", "Омега"),
        rename("Омега/Встреча.md", "Омега/Планёрка.md"),
    ]);
    let got: Vec<(String, Vec<&str>)> = plan
        .notes
        .iter()
        .map(|n| {
            (
                text_of(&n.path),
                n.links.iter().map(|l| l.old_target.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("Б.md".to_owned(), vec!["Альфа/Встреча", "Встреча"]),
            ("Омега/Планёрка.md".to_owned(), vec!["Встреча"]),
            ("Я.md".to_owned(), vec!["Встреча", "Встреча"]),
        ]
    );
    assert_eq!((plan.note_count(), plan.link_count()), (3, 5));
}

/// Все образцы `tests/fixtures/relink/` (0.2-0.3): план и `rewrite` дают файл
/// «после» байт в байт.
#[test]
fn plan_matches_relink_fixtures() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/relink");
    let cases: Value =
        serde_json::from_slice(&std::fs::read(dir.join("cases.json")).expect("cases.json"))
            .expect("JSON");
    for case in cases["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let file = case["file"].as_str().expect("file");
        let read = |key: &str| {
            String::from_utf8(
                std::fs::read(dir.join(case[key].as_str().expect("путь"))).expect("файл"),
            )
            .expect("UTF-8")
        };
        let notes: Vec<(String, i64, String)> = case["vault"]
            .as_array()
            .expect("vault")
            .iter()
            .map(|n| {
                let path = n["path"].as_str().expect("path").to_owned();
                let text = if path == file {
                    read("before")
                } else {
                    String::new()
                };
                (path, n["created"].as_i64().expect("created"), text)
            })
            .collect();
        let vault = Vault { notes };
        let op = [rename(
            case["op"]["from"].as_str().expect("from"),
            case["op"]["to"].as_str().expect("to"),
        )];
        let after = vault.apply(&op, &vault.plan(&op));
        assert_eq!(
            after.text(&moved(file, &op)).as_bytes(),
            read("after").as_bytes(),
            "образец {name}"
        );
    }
}

// --- Свойства ---

const FOLDERS: &[&str] = &["Альфа", "альфа", "Бета", "Х"];
const NAMES: &[&str] = &[
    "Встреча",
    "встреча",
    "Мой",
    "Мои\u{306}",
    "План",
    "Итоги.md",
];
const FOLDER_TO: &[&str] = &["Омега", "Альфа", "альфа", "Бета/Альфа", "Х/Х", "АЛЬФА"];

fn path_strategy() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(prop::sample::select(FOLDERS), 0..3),
        prop::sample::select(NAMES),
    )
        .prop_map(|(folders, name)| {
            let mut parts: Vec<String> = folders.iter().map(|f| (*f).to_owned()).collect();
            parts.push(format!("{name}.md"));
            parts.join("/")
        })
}

/// Сырой случай: заметки, ссылки (номер заметки, вид цели, оформление),
/// операция и, может быть, второе переименование той же заметки или папки.
#[derive(Debug, Clone)]
struct Raw {
    notes: Vec<(String, i64)>,
    links: Vec<Vec<(usize, u8, u8)>>,
    folder_op: bool,
    which: usize,
    depth: usize,
    note_to: String,
    folder_to: &'static str,
    chain: Option<(String, &'static str)>,
}

fn raw_strategy() -> impl Strategy<Value = Raw> {
    (
        prop::collection::vec((path_strategy(), 0i64..3), 1..10),
        prop::collection::vec(
            prop::collection::vec((0usize..64, 0u8..8, 0u8..8), 0..6),
            10,
        ),
        any::<bool>(),
        0usize..64,
        0usize..3,
        path_strategy(),
        prop::sample::select(FOLDER_TO),
        prop::option::of((path_strategy(), prop::sample::select(FOLDER_TO))),
    )
        .prop_map(
            |(notes, links, folder_op, which, depth, note_to, folder_to, chain)| Raw {
                notes,
                links,
                folder_op,
                which,
                depth,
                note_to,
                folder_to,
                chain,
            },
        )
}

/// Цель ссылки на заметку `path` одного из видов: имя, с папкой, от корня,
/// полный путь, в верхнем регистре, в разложенной форме, с `.md`, нет такой.
fn target_for(path: &str, form: u8) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let parts: Vec<&str> = stem.split('/').collect();
    let name = parts[parts.len() - 1];
    match form {
        0 => name.to_owned(),
        1 if parts.len() > 1 => parts[parts.len() - 2..].join("/"),
        1 => name.to_owned(),
        2 => format!("/{stem}"),
        3 => stem.to_owned(),
        4 => name.to_uppercase(),
        5 => stem.nfd().collect(),
        6 => format!("{name}.md"),
        _ => "Нет такой".to_owned(),
    }
}

fn link_text(target: &str, decor: u8) -> String {
    let tail = match decor % 4 {
        0 => "",
        1 => "#Итоги",
        2 => "|ал",
        _ => "#Итоги|ал",
    };
    let bang = if decor >= 4 { "!" } else { "" };
    format!("{bang}[[{target}{tail}]]")
}

/// Есть ли в списке заметка или папка с ровно этим путём.
fn exists(paths: &[String], path: &str) -> bool {
    paths
        .iter()
        .any(|p| p == path || p.starts_with(&format!("{path}/")))
}

/// Случай: хранилище «до» и переименования по очереди; `None` — операция
/// невозможна на диске (цель занята, папка в саму себя, нечего
/// переименовать).
fn build_case(raw: &Raw) -> Option<(Vault, Vec<PathRename>)> {
    let mut notes: Vec<(String, i64)> = Vec::new();
    for (path, created) in &raw.notes {
        if !notes.iter().any(|(p, _)| p == path) {
            notes.push((path.clone(), *created));
        }
    }
    let paths: Vec<String> = notes.iter().map(|(p, _)| p.clone()).collect();
    let texts: Vec<String> = (0..notes.len())
        .map(|i| {
            raw.links
                .get(i)
                .into_iter()
                .flatten()
                .map(|(j, form, decor)| {
                    link_text(&target_for(&paths[j % paths.len()], *form), *decor)
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let chosen = &paths[raw.which % paths.len()];
    let first = if raw.folder_op {
        let folders: Vec<&str> = chosen.split('/').collect();
        let folders = &folders[..folders.len() - 1];
        if folders.is_empty() {
            return None;
        }
        let from = folders[..1 + raw.depth % folders.len()].join("/");
        (from, raw.folder_to.to_owned())
    } else {
        (chosen.clone(), raw.note_to.clone())
    };
    let mut renames = Vec::new();
    let mut current = paths.clone();
    let mut step = |from: String, to: String, current: &mut Vec<String>| -> Option<()> {
        if from == to || exists(current, &to) || to.starts_with(&format!("{from}/")) {
            return None;
        }
        let op = rename(&from, &to);
        for path in current.iter_mut() {
            *path = moved(path, std::slice::from_ref(&op));
        }
        renames.push(op);
        Some(())
    };
    let (from, to) = first;
    step(from, to.clone(), &mut current)?;
    if let Some((note_to, folder_to)) = &raw.chain {
        let next = if raw.folder_op {
            (*folder_to).to_owned()
        } else {
            note_to.clone()
        };
        step(to, next, &mut current)?;
    }
    let vault = Vault {
        notes: notes
            .into_iter()
            .zip(texts)
            .map(|((path, created), text)| (path, created, text))
            .collect(),
    };
    Some((vault, renames))
}

fn key_path(path: &str) -> Vec<names::NameKey> {
    path.split('/').map(names::key).collect()
}

/// «Ссылка ведёт туда же»: каждая ссылка, которая вела на заметку, после
/// операции и плана ведёт на неё же; однозначная остаётся однозначной;
/// переписаны только ссылки, которые без правки вели бы не туда или стали
/// бы неоднозначными. Ссылки на заметку, у которой после операции есть
/// двойник с тем же ключом пути (Linux), не проверяются: на неё нельзя
/// сослаться никак.
fn check_same_notes(vault: &Vault, renames: &[PathRename]) -> Result<(), TestCaseError> {
    let plan = vault.plan(renames);
    let after = vault.apply(renames, &plan);
    let before_links = vault.links();
    let after_links = after.links();
    for ((path, _, old_text), (_, _, new_text)) in vault.notes.iter().zip(&after.notes) {
        let old = markdown::parse(old_text).links;
        let new = markdown::parse(new_text).links;
        prop_assert_eq!(old.len(), new.len(), "{}", path);
        for (a, b) in old.iter().zip(&new) {
            let was = before_links.candidates(&a.target);
            let Some(first) = was.first() else {
                prop_assert_eq!(&a.target, &b.target, "неразрешённая ссылка не меняется");
                continue;
            };
            let now = moved(&text_of(first), renames);
            let twins = after
                .notes
                .iter()
                .filter(|(p, ..)| key_path(p) == key_path(&now))
                .count();
            if twins > 1 {
                continue;
            }
            let hits = after_links.candidates(&b.target);
            prop_assert_eq!(
                hits.first().map(text_of),
                Some(now.clone()),
                "{} в {}: {} → {}",
                a.target,
                path,
                b.target,
                plan.link_count()
            );
            if was.len() == 1 {
                prop_assert_eq!(hits.len(), 1, "стала неоднозначной: {}", b.target);
            }
            if a.target != b.target {
                let stay = after_links.candidates(&a.target);
                let jumps = stay.first().map(text_of) != Some(now.clone());
                let ambiguous = was.len() == 1 && stay.len() > 1;
                prop_assert!(
                    jumps || ambiguous,
                    "лишняя правка {} → {}",
                    a.target,
                    b.target
                );
            }
        }
    }
    Ok(())
}

/// План повторяется: тот же вход — тот же план (порядок карт индекса не
/// влияет); то же переименование, пришедшее ещё раз после выполнения
/// (переименованное уже на месте), — пустой план. Повтор проверяется, когда
/// путей «откуда» после операции нет (цепочка `Бета → Омега`,
/// `Омега → Бета/Альфа` снова создаёт `Бета` — тогда повтор неотличим от
/// нового переименования).
fn check_idempotent(vault: &Vault, renames: &[PathRename]) -> Result<(), TestCaseError> {
    let plan = vault.plan(renames);
    prop_assert_eq!(&vault.plan(renames), &plan);
    let after = vault.apply(renames, &plan);
    let links = after.links();
    if renames.iter().all(|r| !links.has_path(&r.from)) {
        prop_assert!(after.plan(renames).is_empty(), "{:?}", after.plan(renames));
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn links_point_to_same_notes_after_op(raw in raw_strategy()) {
        let Some((vault, renames)) = build_case(&raw) else { return Ok(()) };
        check_same_notes(&vault, &renames)?;
    }

    #[test]
    fn plan_idempotent(raw in raw_strategy()) {
        let Some((vault, renames)) = build_case(&raw) else { return Ok(()) };
        check_idempotent(&vault, &renames)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// То же на 10 000 случаях (план 0.2: `--ignored`, долго для CI).
    #[test]
    #[ignore = "10 000 случаев: cargo test --lib relink::plan -- --ignored"]
    fn links_point_to_same_notes_after_op_10000(raw in raw_strategy()) {
        let Some((vault, renames)) = build_case(&raw) else { return Ok(()) };
        check_same_notes(&vault, &renames)?;
    }

    #[test]
    #[ignore = "10 000 случаев: cargo test --lib relink::plan -- --ignored"]
    fn plan_idempotent_10000(raw in raw_strategy()) {
        let Some((vault, renames)) = build_case(&raw) else { return Ok(()) };
        check_idempotent(&vault, &renames)?;
    }
}

/// Переименование папки на 1 000 заметок с 5 000 входящих ссылок (С2-8.3):
/// план ≤ 1 с в release-сборке; в отладочной — время справочно.
#[test]
#[ignore = "замер: cargo test --release --lib relink_plan_folder_1000 -- --ignored --nocapture"]
fn relink_plan_folder_1000() {
    let mut notes: Vec<(String, i64, String)> = (0..1000)
        .map(|i| (format!("Большой архив/Заметка {i}.md"), 1, String::new()))
        .collect();
    for i in 0..1000 {
        let text = (0..5)
            .map(|k| {
                let n = (i * 5 + k) % 1000;
                if k < 3 {
                    format!("[[Большой архив/Заметка {n}]]")
                } else {
                    format!("[[Заметка {n}|з]]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        notes.push((format!("Входящие/Ссылка {i}.md"), 2, text));
    }
    let vault = Vault { notes };
    let (index, links) = (vault.index(), vault.links());
    let op = [rename("Большой архив", "Архив 2026")];
    let start = Instant::now();
    let plan = plan(&index, &links, &op);
    let took = start.elapsed();
    eprintln!("relink_plan_folder_1000: {took:?}");
    assert_eq!((plan.note_count(), plan.link_count()), (1000, 3000));
    let limit = if cfg!(debug_assertions) {
        Duration::from_secs(10)
    } else {
        Duration::from_secs(1)
    };
    assert!(took <= limit, "{took:?}");
}
