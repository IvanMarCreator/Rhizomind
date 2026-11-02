//! Тесты 0.2-4.4: правка байтов ссылок. Сравнение — побайтно по всему
//! файлу (BOM, CRLF/LF, концевые пробелы), а не по строкам.

use std::cell::RefCell;
use std::path::PathBuf;

use proptest::prelude::*;
use serde_json::Value;

use super::*;
use crate::commands::{PathRename, VaultPath};
use crate::links::Links;
use crate::markdown;
use crate::vault::NoteRef;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/relink")
}

fn p(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

/// Цели по таблице «старая цель → новая» (как в файле).
fn by_target<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&WikiLink) -> Option<String> + 'a {
    move |link| {
        pairs
            .iter()
            .find(|(old, _)| *old == link.target)
            .map(|(_, new)| (*new).to_owned())
    }
}

/// Новый текст или прежний, если менять нечего.
fn apply(text: &str, new_target: &dyn Fn(&WikiLink) -> Option<String>) -> String {
    rewrite(text, new_target).unwrap_or_else(|| text.to_owned())
}

/// Новые цели для образца — по правилу «ссылка ведёт туда же» (С2-3.5,
/// С2-3.6, Р2, Р17 п. 9): ссылка, которая до операции вела на заметку `P`,
/// переписывается на кратчайший различающий путь `P` после операции, если
/// после операции она вела бы не туда или стала бы неоднозначной (а до этого
/// была однозначной). Это упрощённый план (полный — задача 0.2-4.3), нужный
/// только чтобы получить цели для образцов.
fn fixture_targets(links: &Links, op: &PathRename) -> impl Fn(&WikiLink) -> Option<String> {
    move |link| {
        let before = links.view();
        let after = links.after_renames(std::slice::from_ref(op));
        let was = before.resolve(&link.target)?;
        let now = after.moved_to(&was).cloned().unwrap_or(was.clone());
        let jumps = after.resolve(&link.target).as_ref() != Some(&now);
        let became_ambiguous =
            after.candidates(&link.target).len() > 1 && before.candidates(&link.target).len() == 1;
        (jumps || became_ambiguous).then(|| after.shortest_unique_path(&now).expect("заметка есть"))
    }
}

#[test]
fn fixtures_relink_byte_exact() {
    let dir = fixtures_dir();
    let cases: Value =
        serde_json::from_slice(&std::fs::read(dir.join("cases.json")).expect("cases.json"))
            .expect("JSON");
    let cases = cases["cases"].as_array().expect("cases");
    assert!(cases.len() >= 23, "образцов {}", cases.len());
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let notes = case["vault"]
            .as_array()
            .expect("vault")
            .iter()
            .map(|n| NoteRef {
                path: p(n["path"].as_str().expect("path")),
                created_ms: n["created"].as_i64().expect("created"),
            })
            .collect();
        let links = Links::new(notes);
        let op = PathRename {
            from: p(case["op"]["from"].as_str().expect("from")),
            to: p(case["op"]["to"].as_str().expect("to")),
        };
        let before =
            std::fs::read(dir.join(case["before"].as_str().expect("before"))).expect("файл до");
        let after =
            std::fs::read(dir.join(case["after"].as_str().expect("after"))).expect("файл после");
        let text = String::from_utf8(before.clone()).expect("UTF-8");
        let result = rewrite(&text, &fixture_targets(&links, &op));
        if before == after {
            assert_eq!(result, None, "{name}: файл не должен меняться");
        }
        let got = result.map_or(before, String::into_bytes);
        assert!(
            got == after,
            "{name}: файл после переписывания не совпал побайтно\nполучено:  {:?}\nожидалось: {:?}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&after)
        );
    }
}

#[test]
fn crlf_bom_preserved() {
    let text = "\u{feff}# Заголовок  \r\nСм. [[Встреча]] и [[Другая]].   \r\n\r\n\
                - пункт [[Встреча|встречи]]\t\r\nодиночный\rперенос\n[[Встреча#Итоги]]";
    let want = "\u{feff}# Заголовок  \r\nСм. [[Планёрка]] и [[Другая]].   \r\n\r\n\
                - пункт [[Планёрка|встречи]]\t\r\nодиночный\rперенос\n[[Планёрка#Итоги]]";
    let got = apply(text, &by_target(&[("Встреча", "Планёрка")]));
    assert_eq!(got.as_bytes(), want.as_bytes());
}

#[test]
fn bom_frontmatter_recognized() {
    // BOM не часть текста: блок свойств в начале распознаётся.
    let text = "\u{feff}---\nсвязь: \"[[Встреча]]\"\n---\n[[Встреча]]\n";
    let want = "\u{feff}---\nсвязь: \"[[Встреча]]\"\n---\n[[Планёрка]]\n";
    let got = apply(text, &by_target(&[("Встреча", "Планёрка")]));
    assert_eq!(got.as_bytes(), want.as_bytes());
}

#[test]
fn code_and_frontmatter_untouched() {
    let text = "---\nсвязь: \"[[Встреча]]\"\n---\nКод `[[Встреча]]` и [[Встреча]].\n\n\
                ```\n[[Встреча]]\n```\n\n    [[Встреча]]\n\n~~~\n[[Встреча]]\n~~~\nКонец [[Встреча]]\n";
    let want = "---\nсвязь: \"[[Встреча]]\"\n---\nКод `[[Встреча]]` и [[Планёрка]].\n\n\
                ```\n[[Встреча]]\n```\n\n    [[Встреча]]\n\n~~~\n[[Встреча]]\n~~~\nКонец [[Планёрка]]\n";
    // Функцию спрашивают только о ссылках вне кода и блока свойств.
    let asked = RefCell::new(Vec::new());
    let got = apply(text, &|link: &WikiLink| {
        asked.borrow_mut().push(link.span.clone());
        Some("Планёрка".to_owned())
    });
    assert_eq!(got.as_bytes(), want.as_bytes());
    let asked = asked.into_inner();
    assert_eq!(asked.len(), 2);
    for span in asked {
        assert!(!text[..span.start].ends_with('`'));
    }
}

#[test]
fn stale_offsets_refound() {
    // Индекс видел `[[Встреча]]` в начале файла; с тех пор заметку правили:
    // перед ссылкой появился текст (в том числе слово «Встреча» и ссылка в
    // коде), ссылка переехала. Ссылки находятся заново по свежему тексту.
    let stale = "[[Встреча]] и [[Встреча|в]]\n";
    let fresh = "Встреча: добавлено `[[Встреча]]` в начале.\r\n[[Встреча]] и\n[[Встреча|в]]\n";
    let want = "Встреча: добавлено `[[Встреча]]` в начале.\r\n[[Планёрка]] и\n[[Планёрка|в]]\n";
    let targets = by_target(&[("Встреча", "Планёрка")]);
    assert_eq!(apply(fresh, &targets).as_bytes(), want.as_bytes());
    // Смещения ссылок в старом тексте не совпадают со свежими.
    let old: Vec<_> = markdown::parse(stale)
        .links
        .into_iter()
        .map(|l| l.target_span)
        .collect();
    let new: Vec<_> = markdown::parse(fresh)
        .links
        .into_iter()
        .map(|l| l.target_span)
        .collect();
    assert_ne!(old, new);
    // Ссылку удалили — переписывать нечего.
    assert_eq!(rewrite("Ссылку убрали.\n", &targets), None);
}

#[test]
fn already_correct_untouched() {
    // Цель уже новая (доделка после сбоя, повтор) — файл не меняется.
    let text = "[[Планёрка]] [[Планёрка.md#Итоги]] ![[ Планёрка |п]]\r\n";
    assert_eq!(rewrite(text, &|_| Some("Планёрка".to_owned())), None);
    // Функция ничего не меняет — тоже.
    assert_eq!(rewrite("[[Встреча]]\n", &|_| None), None);
    // Меняется только то, что ещё на старой цели.
    let text = "[[Планёрка]] [[Встреча]]\n";
    assert_eq!(
        apply(text, &|_| Some("Планёрка".to_owned())),
        "[[Планёрка]] [[Планёрка]]\n"
    );
}

#[test]
fn md_suffix_and_blanks_kept() {
    let text = "[[ Встреча.md ]] [[Альфа/Встреча.MD#З|п]] [[\tВстреча\t#З]]\n";
    let want = "[[ Бета/Встреча.md ]] [[Бета/Встреча.MD#З|п]] [[\tБета/Встреча\t#З]]\n";
    let got = apply(text, &|_| Some("Бета/Встреча".to_owned()));
    assert_eq!(got.as_bytes(), want.as_bytes());
}

#[test]
fn table_pipe_kept() {
    let text = "| [[встреча\\|в]] | [[Встреча#И\\|и]] |\n";
    let want = "| [[Планёрка\\|в]] | [[Планёрка#И\\|и]] |\n";
    let got = apply(text, &|_| Some("Планёрка".to_owned()));
    assert_eq!(got.as_bytes(), want.as_bytes());
}

#[test]
fn unwritable_target_left_as_is() {
    // Цель, которая сломала бы ссылку или разбор вокруг, не вставляется.
    for bad in [
        "", " Х", "Х ", "\tХ", "А]]Б", "А]Б", "А[Б", "А|Б", "А#Б", "А`Б", "А\nБ", "А\rБ", "А\\Б",
        "А//Б", "/", "../А", "А:Б",
    ] {
        assert_eq!(
            rewrite("[[Встреча]] и [[Встреча|в]]\n", &|_| Some(
                bad.to_owned()
            )),
            None,
            "цель {bad:?}"
        );
    }
    // Остальные ссылки при этом переписываются.
    let got = apply(
        "[[Встреча]] [[Другая]]\n",
        &|link: &WikiLink| {
            Some(
                if link.target == "Встреча" {
                    "А|Б"
                } else {
                    "Третья"
                }
                .to_owned(),
            )
        },
    );
    assert_eq!(got, "[[Встреча]] [[Третья]]\n");
}

#[test]
fn spec_table_snapshot() {
    // Таблица С2-3.5: переименование «Встреча» → «Планёрка» и перенос
    // `Альфа/Встреча.md` в `Бета/` при повторе имени.
    let rows = [
        "[[Встреча]]",
        "[[Встреча|встречи]]",
        "[[Встреча#Итоги]]",
        "[[Альфа/Встреча#Итоги|итоги]]",
        "![[Встреча]]",
    ];
    let mut table = String::new();
    for row in rows {
        let renamed = apply(row, &|_| Some("Планёрка".to_owned()));
        let moved = apply(row, &|_| Some("Бета/Встреча".to_owned()));
        table.push_str(&format!("{row} → {renamed} | {moved}\n"));
    }
    insta::assert_snapshot!(table);
}

/// Кусочки текста для случайных заметок: разметка ссылок, кода, блока
/// свойств, переносы всех видов, кириллица.
fn piece() -> impl Strategy<Value = String> {
    let raw = prop::sample::select(vec![
        "[[",
        "]]",
        "![[",
        "|",
        "\\|",
        "#",
        "`",
        "``",
        "```",
        "~~~",
        "    ",
        "\t",
        " ",
        "\n",
        "\r\n",
        "\r",
        "---",
        "...",
        "Встреча",
        "А/Б",
        "/Корень",
        ".md",
        ".MD",
        "a",
        "й",
        "й\u{301}",
        "- ",
        "> ",
        "1. ",
        "tags: ",
        "\u{feff}",
        "😀",
        "\\",
        "x",
        "[",
        "]",
    ])
    .prop_map(str::to_owned);
    // Целая ссылка: цель (с пробелами по краям, `.md`, путём), заголовок,
    // псевдоним (`|` или `\|`).
    let link = (
        prop::sample::select(vec!["[[", "![["]),
        prop::sample::select(vec!["", " ", "\t"]),
        prop::sample::select(vec![
            "Встреча",
            "встреча.md",
            "Альфа/Встреча.MD",
            "/Корень",
            "a",
            "",
            "Итоги.md.md",
        ]),
        prop::sample::select(vec!["", " ", "\t"]),
        prop::option::of(prop::sample::select(vec!["Итоги", " З ", ""])),
        prop::option::of((
            prop::sample::select(vec!["|", "\\|"]),
            prop::sample::select(vec!["п", " в ", ""]),
        )),
    )
        .prop_map(|(open, l, target, r, heading, alias)| {
            let heading = heading.map(|h| format!("#{h}")).unwrap_or_default();
            let alias = alias
                .map(|(bar, a)| format!("{bar}{a}"))
                .unwrap_or_default();
            format!("{open}{l}{target}{r}{heading}{alias}]]")
        });
    prop_oneof![3 => raw, 1 => link]
}

/// Новые цели: обычные, с путём, с `.md`, и такие, которые вставлять нельзя.
fn choice() -> impl Strategy<Value = Option<String>> {
    prop::option::of(
        prop::sample::select(vec![
            "Планёрка",
            "Бета/Встреча",
            "/Альфа/Встреча",
            "Итоги.md.md",
            "Встреча",
            "й",
            "a]]b",
            "x|y",
            "",
            " Х",
            "z`z",
            "a\nb",
        ])
        .prop_map(str::to_owned),
    )
}

/// `.md` в конце (любой регистр) — суффикс, который переписывание сохраняет.
fn without_suffix(target: &str) -> &str {
    match target.len().checked_sub(3) {
        Some(at)
            if at > 0
                && target.is_char_boundary(at)
                && target[at..].eq_ignore_ascii_case(".md") =>
        {
            &target[..at]
        }
        _ => target,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Всё, кроме целей ссылок, — байт в байт как было; ссылок столько же,
    /// у каждой те же встраивание, заголовок, псевдоним и строка; цель —
    /// прежняя или та, что вернула функция (с прежним суффиксом `.md`).
    #[test]
    fn bytes_outside_targets_identical(
        bom in any::<bool>(),
        pieces in prop::collection::vec(piece(), 0..60),
        choices in prop::collection::vec(choice(), 1..6),
    ) {
        let text = format!("{}{}", if bom { "\u{feff}" } else { "" }, pieces.concat());
        let pick = |link: &WikiLink| choices[link.span.start % choices.len()].clone();
        let out = apply(&text, &pick);
        // BOM в начале файла — не текст (и когда он пришёл из кусочков).
        let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let out_body = out.strip_prefix('\u{feff}').unwrap_or(&out);
        prop_assert_eq!(text.len() - body.len(), out.len() - out_body.len());
        let old = markdown::parse(body).links;
        let new = markdown::parse(out_body).links;
        prop_assert_eq!(old.len(), new.len());
        let (mut at_old, mut at_new) = (0, 0);
        for (a, b) in old.iter().zip(&new) {
            // Байты до цели — те же.
            prop_assert_eq!(
                &body.as_bytes()[at_old..a.target_span.start],
                &out_body.as_bytes()[at_new..b.target_span.start]
            );
            let (was, now) = (&body[a.target_span.clone()], &out_body[b.target_span.clone()]);
            if was != now {
                let wanted = pick(a).expect("цель меняется только по ответу функции");
                let suffix = &was[without_suffix(was).len()..];
                if suffix.is_empty() {
                    prop_assert_eq!(now, wanted.as_str());
                } else {
                    prop_assert_eq!(without_suffix(now), without_suffix(&wanted));
                    prop_assert_eq!(&now[without_suffix(now).len()..], suffix);
                }
            }
            prop_assert_eq!(a.embed, b.embed);
            prop_assert_eq!(&a.heading, &b.heading);
            prop_assert_eq!(&a.alias, &b.alias);
            prop_assert_eq!(a.line, b.line);
            at_old = a.target_span.end;
            at_new = b.target_span.end;
        }
        prop_assert_eq!(&body.as_bytes()[at_old..], &out_body.as_bytes()[at_new..]);
    }
}
