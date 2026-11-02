//! Тесты 0.2-4.1: все кандидаты цели и разрешение по списку заметок «до» и
//! «после» изменения (`NotesView`).

use super::*;
use crate::commands::{ErrorCode, PathRename};

fn p(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

fn note(path: &str, created_ms: i64) -> NoteRef {
    NoteRef {
        path: p(path),
        created_ms,
    }
}

fn links(notes: &[(&str, i64)]) -> Links {
    Links::new(notes.iter().map(|&(path, at)| note(path, at)).collect())
}

fn rename(from: &str, to: &str) -> PathRename {
    PathRename {
        from: p(from),
        to: p(to),
    }
}

fn s(path: &VaultPath) -> String {
    path.components().join("/")
}

fn resolve(view: &NotesView<'_>, target: &str) -> Option<String> {
    view.resolve(target).map(|path| s(&path))
}

fn candidates(view: &NotesView<'_>, target: &str) -> Vec<String> {
    view.candidates(target).iter().map(s).collect()
}

fn shortest(view: &NotesView<'_>, path: &str) -> String {
    view.shortest_unique_path(&p(path)).unwrap()
}

#[test]
fn candidates_lists_all_duplicates() {
    let l = links(&[
        ("Б/Встреча.md", 100),
        ("А/Встреча.md", 100),
        ("В/Встреча.md", 50),
        ("Встреча.md", 200),
        ("Итоги.md", 1),
    ]);
    // Самая старая первой, при равной дате — по порядку путей ядра.
    let all: Vec<String> = l.candidates("Встреча").iter().map(s).collect();
    assert_eq!(
        all,
        ["В/Встреча.md", "А/Встреча.md", "Б/Встреча.md", "Встреча.md"]
    );
    assert_eq!(
        l.resolve(&p("Откуда.md"), "Встреча").unwrap().as_ref(),
        l.candidates("Встреча").first()
    );
    let v = l.view();
    assert_eq!(candidates(&v, "встреча|встречи"), all);
    assert_eq!(candidates(&v, "/Встреча"), ["Встреча.md"]);
    assert_eq!(candidates(&v, "А/Встреча#Итоги"), ["А/Встреча.md"]);
    assert_eq!(candidates(&v, "Итоги"), ["Итоги.md"]);
    for target in ["Нет", "../Встреча", "", "#Итоги", "Г/Встреча"] {
        assert!(v.candidates(target).is_empty(), "{target:?}");
    }
    // Путь — конец пути: `Здоровье/Звонок` подходит обеим.
    let l = links(&[("Команда/Здоровье/Звонок.md", 1), ("Здоровье/Звонок.md", 9)]);
    assert_eq!(
        candidates(&l.view(), "Здоровье/Звонок"),
        ["Команда/Здоровье/Звонок.md", "Здоровье/Звонок.md"]
    );
}

#[test]
fn resolve_against_before_and_after_lists() {
    // Образец 12: старшая заметка переименована в «Встреча» — ссылка по
    // имени «перескочила» бы.
    let l = links(&[
        ("Альфа/Старая.md", 1),
        ("Бета/Встреча.md", 2),
        ("Заметка.md", 3),
    ]);
    let before = l.view();
    let after = l.after_renames(&[rename("Альфа/Старая.md", "Альфа/Встреча.md")]);
    assert_eq!(
        resolve(&before, "Встреча").as_deref(),
        Some("Бета/Встреча.md")
    );
    assert_eq!(
        resolve(&after, "Встреча").as_deref(),
        Some("Альфа/Встреча.md")
    );
    assert_eq!(
        candidates(&after, "Встреча"),
        ["Альфа/Встреча.md", "Бета/Встреча.md"]
    );
    assert_eq!(resolve(&after, "Старая"), None);
    assert!(!after.contains(&p("Альфа/Старая.md")));
    assert!(after.contains(&p("Альфа/Встреча.md")));
    assert_eq!(
        after.moved_to(&p("Альфа/Старая.md")),
        Some(&p("Альфа/Встреча.md"))
    );
    assert_eq!(after.moved_to(&p("Бета/Встреча.md")), None);
    // Общий список не изменился.
    assert_eq!(
        resolve(&l.view(), "Старая").as_deref(),
        Some("Альфа/Старая.md")
    );
    assert_eq!(l.candidates("Встреча").len(), 1);

    // Переименование папки: заметки внутри получают новый путь.
    let l = links(&[
        ("Альфа/Встреча.md", 1),
        ("Альфа/Вложенная/План.md", 2),
        ("Альфа2/Итоги.md", 3),
        ("Бета/Встреча.md", 4),
    ]);
    let after = l.after_renames(&[rename("Альфа", "Омега")]);
    assert_eq!(after.moves().len(), 2);
    assert_eq!(resolve(&after, "Альфа/Встреча"), None);
    assert_eq!(
        resolve(&after, "Омега/Встреча").as_deref(),
        Some("Омега/Встреча.md")
    );
    assert_eq!(
        resolve(&after, "План").as_deref(),
        Some("Омега/Вложенная/План.md")
    );
    // Папка `Альфа2` не внутри `Альфа`.
    assert_eq!(resolve(&after, "Итоги").as_deref(), Some("Альфа2/Итоги.md"));
    assert_eq!(
        resolve(&after, "Встреча").as_deref(),
        Some("Омега/Встреча.md")
    );

    // Смена только регистра — та же заметка под новым написанием.
    let l = links(&[("Альфа/Встреча.md", 1)]);
    let after = l.after_renames(&[rename("Альфа/Встреча.md", "Альфа/встреча.md")]);
    assert_eq!(
        resolve(&after, "Встреча").as_deref(),
        Some("Альфа/встреча.md")
    );

    // Пачка переименований — все пути «откуда» в списке «до»: обмен именами.
    let l = links(&[("А.md", 1), ("Б.md", 2)]);
    let after = l.after_renames(&[rename("А.md", "Б.md"), rename("Б.md", "А.md")]);
    assert_eq!(candidates(&after, "А"), ["А.md"]);
    assert_eq!(after.moved_to(&p("А.md")), Some(&p("Б.md")));
    assert_eq!(after.moved_to(&p("Б.md")), Some(&p("А.md")));
    // Даты создания переезжают с заметкой: старшая теперь «Б».
    let l2 = links(&[("А.md", 1), ("Х/Б.md", 2)]);
    let after = l2.after_renames(&[rename("А.md", "Б.md")]);
    assert_eq!(candidates(&after, "Б"), ["Б.md", "Х/Б.md"]);

    // Общий вид: убрать и добавить заметки.
    let l = links(&[("Бета/Встреча.md", 5), ("Гамма/Встреча.md", 6)]);
    let changed = l.with_changes(&[p("Бета/Встреча.md")], vec![note("Новая/Встреча.md", 9)]);
    assert_eq!(
        candidates(&changed, "Встреча"),
        ["Гамма/Встреча.md", "Новая/Встреча.md"]
    );
    // Не заметка (`.txt`) в список не попадает — как в `replace_notes`.
    let changed = l.with_changes(&[], vec![note("Встреча.txt", 0)]);
    assert_eq!(
        resolve(&changed, "Встреча").as_deref(),
        Some("Бета/Встреча.md")
    );
    // Переименование того, чего нет в списке, ничего не меняет.
    let after = l.after_renames(&[rename("Нет.md", "Встреча.md"), rename("Нет", "Да")]);
    assert!(after.moves().is_empty());
    assert_eq!(candidates(&after, "Встреча").len(), 2);
}

#[test]
fn shortest_path_in_hypothetical_list() {
    // Образец 06: перенос одной из повторяющихся «Встреч».
    let l = links(&[
        ("Альфа/Встреча.md", 1),
        ("Арбуз/Встреча.md", 1),
        ("Заметка.md", 2),
    ]);
    let after = l.after_renames(&[rename("Альфа/Встреча.md", "Бета/Встреча.md")]);
    assert_eq!(shortest(&l.view(), "Альфа/Встреча.md"), "Альфа/Встреча");
    assert_eq!(shortest(&after, "Бета/Встреча.md"), "Бета/Встреча");
    assert_eq!(shortest(&after, "Арбуз/Встреча.md"), "Арбуз/Встреча");
    let err = after
        .shortest_unique_path(&p("Альфа/Встреча.md"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    // Образец 16: появилась `Х/Альфа/Встреча.md` — путь `Альфа/Встреча`
    // стал концом другого пути.
    let l = links(&[("Альфа/Встреча.md", 1), ("Х/Альфа/Старое.md", 2)]);
    let after = l.after_renames(&[rename("Х/Альфа/Старое.md", "Х/Альфа/Встреча.md")]);
    assert_eq!(shortest(&l.view(), "Альфа/Встреча.md"), "Встреча");
    assert_eq!(shortest(&after, "Альфа/Встреча.md"), "/Альфа/Встреча");
    assert_eq!(shortest(&after, "Х/Альфа/Встреча.md"), "Х/Альфа/Встреча");
    // Общий список не изменился.
    assert_eq!(
        l.shortest_unique_path(&p("Альфа/Встреча.md")).unwrap(),
        "Встреча"
    );

    // Образцы 13 и 14: переименование папки.
    let l = links(&[("Альфа/Встреча.md", 1), ("Альфа/План.md", 2)]);
    let after = l.after_renames(&[rename("Альфа", "Омега")]);
    assert_eq!(shortest(&after, "Омега/Встреча.md"), "Встреча");
    let l = links(&[("Альфа/Встреча.md", 1), ("Бета/Встреча.md", 2)]);
    let after = l.after_renames(&[rename("Альфа", "Омега")]);
    assert_eq!(shortest(&after, "Омега/Встреча.md"), "Омега/Встреча");

    // Повтор исчез — снова имя.
    let after = l.with_changes(&[p("Бета/Встреча.md")], vec![]);
    assert_eq!(shortest(&after, "Альфа/Встреча.md"), "Встреча");
}

/// Образцы `tests/fixtures/relink`: каждая переписанная ссылка в файле
/// «после» ведёт по списку «после» туда же, куда исходная вела по списку
/// «до», однозначно и записана кратчайшим различающим путём (Р2, Р3).
/// Переписывание текста — дело `relink` (0.2-4.4); здесь — только
/// разрешение.
#[test]
fn relink_fixtures_resolve_to_the_same_note() {
    use serde_json::Value;
    const CASES: &str = include_str!("../../../tests/fixtures/relink/cases.json");
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/relink");

    /// Тексты между `[[` и `]]` по порядку — грубо, без правил кода: пары
    /// «до/после» сравниваются только там, где текст изменился.
    fn inners(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("]]") else { break };
            out.push(&after[..end]);
            rest = &after[end + 2..];
        }
        out
    }

    fn shape(target: &str) -> (Vec<String>, String, bool) {
        let t = parse_target(target).unwrap();
        (t.folders, t.name, t.from_root)
    }

    let doc: Value = serde_json::from_str(CASES).unwrap();
    let mut changed_total = 0;
    for case in doc["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let notes: Vec<NoteRef> = case["vault"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| note(n["path"].as_str().unwrap(), n["created"].as_i64().unwrap()))
            .collect();
        let l = Links::new(notes);
        let op = &case["op"];
        let after = l.after_renames(&[rename(
            op["from"].as_str().unwrap(),
            op["to"].as_str().unwrap(),
        )]);
        assert!(!after.moves().is_empty(), "{name}");
        let before = l.view();
        let read = |key: &str| {
            let bytes = std::fs::read(fixtures.join(case[key].as_str().unwrap())).unwrap();
            String::from_utf8(bytes).unwrap()
        };
        let (old_text, new_text) = (read("before"), read("after"));
        let (old, new) = (inners(&old_text), inners(&new_text));
        assert_eq!(old.len(), new.len(), "{name}");
        let features: Vec<&str> = case["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        let has_code = features.contains(&"code") || features.contains(&"frontmatter");
        for (b, a) in old.iter().zip(&new) {
            let was = before.resolve(b);
            let expected = was.as_ref().map(|path| {
                after
                    .moved_to(path)
                    .cloned()
                    .unwrap_or_else(|| path.clone())
            });
            if b != a {
                changed_total += 1;
                let target = expected.clone().unwrap_or_else(|| panic!("{name}: {b}"));
                assert_eq!(
                    after.candidates(a),
                    vec![target.clone()],
                    "{name}: {b} → {a}"
                );
                assert_eq!(
                    shape(a),
                    shape(&after.shortest_unique_path(&target).unwrap()),
                    "{name}: {a}"
                );
            } else if !has_code {
                // Не переписанная ссылка и так ведёт туда же.
                assert_eq!(after.resolve(a), expected, "{name}: {a}");
            }
        }
    }
    assert_eq!(changed_total, 38, "переписанных ссылок в образцах");
}

/// Стоимость вида «после» на 50 000 заметок: перенос папки на 1 000
/// заметок не копирует общий список. Справочно:
/// `cargo test -p rhizomind --lib links::view_tests::view_cost_50000 -- --ignored --nocapture`.
#[test]
#[ignore = "замер"]
fn view_cost_50000() {
    let notes: Vec<NoteRef> = (0..50_000)
        .map(|i| {
            let folder = if i < 1_000 {
                "Большой архив".to_owned()
            } else {
                format!("Папка {}", i % 500)
            };
            note(&format!("{folder}/Заметка {}.md", i % 20_000), i)
        })
        .collect();
    let l = Links::new(notes);
    let started = std::time::Instant::now();
    let after = l.after_renames(&[rename("Большой архив", "Архив/Старое")]);
    let built = started.elapsed();
    let mut found = 0;
    for i in 0..1_000 {
        let path = p(&format!("Архив/Старое/Заметка {i}.md"));
        found += usize::from(after.shortest_unique_path(&path).is_ok());
        let _ = after.candidates(&format!("Заметка {i}"));
    }
    let total = started.elapsed();
    println!("вид после переноса 1 000 из 50 000: {built:?}; +1 000 путей и кандидатов: {total:?}");
    assert_eq!((after.moves().len(), found), (1_000, 1_000));
    assert!(total < std::time::Duration::from_secs(1), "{total:?}");
}

mod props {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    const PARTS: [&str; 7] = ["А", "а", "Б", "В", "Встреча", "ß", "ss"];

    fn path() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(PARTS.to_vec()), 1..5)
            .prop_map(|parts| format!("{}.md", parts.join("/")))
    }

    fn key_of(path: &str) -> Vec<names::NameKey> {
        path.split('/').map(names::key).collect()
    }

    /// Пути с разными ключами — как на диске Windows.
    fn distinct(
        paths: Vec<(String, i64)>,
        taken: &mut HashSet<Vec<names::NameKey>>,
    ) -> Vec<(String, i64)> {
        paths
            .into_iter()
            .filter(|(path, _)| taken.insert(key_of(path)))
            .collect()
    }

    /// Цели, которые стоит проверить: все концы путей с `/` в начале и без,
    /// в другом регистре, и мусор.
    fn targets(paths: &[String]) -> Vec<String> {
        let mut out = vec!["Нет".to_owned(), "../А".to_owned(), String::new()];
        for path in paths {
            let stem = path.trim_end_matches(".md");
            let parts: Vec<&str> = stem.split('/').collect();
            for k in 0..parts.len() {
                let tail = parts[k..].join("/");
                out.push(format!("/{tail}"));
                out.push(tail.to_uppercase());
                out.push(format!("{tail}#Итоги|и"));
                out.push(tail);
            }
        }
        out
    }

    fn check_same(
        view: &NotesView<'_>,
        full: &Links,
        paths: &[String],
        gone: &[String],
    ) -> Result<(), TestCaseError> {
        let rebuilt = full.view();
        for target in targets(paths).iter().chain(&targets(gone)) {
            prop_assert_eq!(
                view.candidates(target),
                rebuilt.candidates(target),
                "{}",
                target
            );
            prop_assert_eq!(
                view.resolve(target),
                full.resolve(&p("Откуда.md"), target).unwrap(),
                "{}",
                target
            );
        }
        for path in paths {
            prop_assert_eq!(
                view.shortest_unique_path(&p(path)).unwrap(),
                full.shortest_unique_path(&p(path)).unwrap(),
                "{}",
                path
            );
            prop_assert!(view.contains(&p(path)));
        }
        for path in gone {
            prop_assert!(view.shortest_unique_path(&p(path)).is_err(), "{}", path);
            prop_assert!(!view.contains(&p(path)));
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Вид «после» поверх общего списка отвечает так же, как список,
        /// собранный заново из заметок «после».
        #[test]
        fn resolve_same_as_full_rebuild(
            before in prop::collection::vec((path(), 0i64..3), 1..14),
            remove in prop::collection::vec(any::<bool>(), 14),
            added in prop::collection::vec((path(), 0i64..3), 0..6),
        ) {
            let mut taken = HashSet::new();
            let before = distinct(before, &mut taken);
            let removed: Vec<String> = before
                .iter()
                .zip(&remove)
                .filter(|(_, r)| **r)
                .map(|((path, _), _)| path.clone())
                .collect();
            for path in &removed {
                taken.remove(&key_of(path));
            }
            let added = distinct(added, &mut taken);
            let l = Links::new(before.iter().map(|(path, at)| note(path, *at)).collect());
            let view = l.with_changes(
                &removed.iter().map(|path| p(path)).collect::<Vec<_>>(),
                added.iter().map(|(path, at)| note(path, *at)).collect(),
            );
            let after: Vec<(String, i64)> = before
                .iter()
                .filter(|(path, _)| !removed.contains(path))
                .chain(&added)
                .cloned()
                .collect();
            let full = Links::new(after.iter().map(|(path, at)| note(path, *at)).collect());
            let paths: Vec<String> = after.iter().map(|(path, _)| path.clone()).collect();
            let gone: Vec<String> = removed
                .into_iter()
                .filter(|path| !paths.contains(path))
                .collect();
            check_same(&view, &full, &paths, &gone)?;
        }

        /// То же для переименований заметок и папки.
        #[test]
        fn after_renames_same_as_full_rebuild(
            before in prop::collection::vec((path(), 0i64..3), 1..14),
            moves in prop::collection::vec((0usize..14, path()), 0..4),
            folder in prop::sample::select(PARTS.to_vec()),
            folder_to in prop::sample::select(vec!["Г", "Д/Е", "Встреча"]),
        ) {
            let mut taken = HashSet::new();
            let before = distinct(before, &mut taken);
            let mut renames = Vec::new();
            let mut after: Vec<(String, i64)> = before.clone();
            let mut moved = HashSet::new();
            for (i, to) in moves {
                let Some((from, _)) = before.get(i) else { continue };
                if !moved.insert(i) {
                    continue;
                }
                renames.push(rename(from, &to));
                after[i].0 = to;
            }
            // Папка — первый компонент; заметки, уже переименованные выше,
            // она не трогает (первое переименование в пачке выигрывает).
            renames.push(rename(folder, folder_to));
            for (i, (path, _)) in after.iter_mut().enumerate() {
                if moved.contains(&i) {
                    continue;
                }
                if let Some(rest) = path.strip_prefix(&format!("{folder}/")) {
                    *path = format!("{folder_to}/{rest}");
                }
            }
            // Только пачки, после которых ключи путей различны.
            let keys: HashSet<_> = after.iter().map(|(path, _)| key_of(path)).collect();
            prop_assume!(keys.len() == after.len());
            let l = Links::new(before.iter().map(|(path, at)| note(path, *at)).collect());
            let view = l.after_renames(&renames);
            // Порядок списка, собранного заново, — как у вида: оставшиеся,
            // затем перемещённые в порядке списка «до».
            let stay: Vec<&(String, i64)> = before
                .iter()
                .zip(&after)
                .filter(|(b, a)| b.0 == a.0)
                .map(|(_, a)| a)
                .collect();
            let gone: Vec<String> = before
                .iter()
                .zip(&after)
                .filter(|(b, a)| b.0 != a.0)
                .map(|(b, _)| b.0.clone())
                .filter(|path| !after.iter().any(|(a, _)| a == path))
                .collect();
            let moved_notes: Vec<&(String, i64)> = before
                .iter()
                .zip(&after)
                .filter(|(b, a)| b.0 != a.0)
                .map(|(_, a)| a)
                .collect();
            let full = Links::new(
                stay.iter()
                    .chain(&moved_notes)
                    .map(|(path, at)| note(path, *at))
                    .collect(),
            );
            let paths: Vec<String> = after.iter().map(|(path, _)| path.clone()).collect();
            check_same(&view, &full, &paths, &gone)?;
        }
    }
}
