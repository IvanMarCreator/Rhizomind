//! Вики-ссылки в хранилище: виды, неразрешённые, `[[...]]` внутри кода и
//! ожидаемые разрешения из манифеста. Разбор и разрешение — свои, по С3.4.

mod common;

use common::{TempDir, read_tree};
use gen_vault::{Config, Manifest, Resolution, generate};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

fn key(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase().nfc().collect()
}

/// Найденные в тексте ссылки (содержимое между `[[` и `]]`) и число `[[`
/// внутри кода: блоки ``` и ~~~, блоки с отступом, код в строке.
#[derive(Default)]
struct Scan {
    links: Vec<String>,
    in_code: usize,
}

fn scan(text: &str) -> Scan {
    let mut out = Scan::default();
    let mut fence: Option<&str> = None;
    let mut prev_blank = true;
    let mut in_indented = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            } else {
                out.in_code += line.matches("[[").count();
            }
            prev_blank = false;
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
            prev_blank = false;
            continue;
        }
        let indented = line.starts_with("    ") || line.starts_with('\t');
        if indented && (prev_blank || in_indented) {
            in_indented = true;
            out.in_code += line.matches("[[").count();
            continue;
        }
        in_indented = false;
        prev_blank = line.trim().is_empty();
        scan_inline(line, &mut out);
    }
    out
}

/// Строка вне блоков кода: код в строке (` и ``) пропускается.
fn scan_inline(line: &str, out: &mut Scan) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let run = b[i..].iter().take_while(|&&c| c == b'`').count();
            let ticks = "`".repeat(run);
            if let Some(end) = line[i + run..].find(&ticks) {
                out.in_code += line[i + run..i + run + end].matches("[[").count();
                i += run + end + run;
                continue;
            }
            i += run;
        } else if line[i..].starts_with("[[") {
            match line[i + 2..].find("]]") {
                Some(end) => {
                    out.links.push(line[i + 2..i + 2 + end].to_string());
                    i += 2 + end + 2;
                }
                None => i += 2,
            }
        } else {
            i += 1;
        }
    }
}

struct Parts<'a> {
    target: &'a str,
    heading: Option<&'a str>,
    alias: bool,
}

fn parts(inner: &str) -> Parts<'_> {
    let (left, alias) = match inner.split_once('|') {
        Some((l, _)) => (l, true),
        None => (inner, false),
    };
    let (target, heading) = match left.split_once('#') {
        Some((t, h)) => (t, Some(h)),
        None => (left, None),
    };
    Parts {
        target,
        heading,
        alias,
    }
}

/// Заметки хранилища: путь (с `.md`) и ключи компонентов пути без `.md`.
fn note_keys(tree: &BTreeMap<String, Vec<u8>>) -> Vec<(String, Vec<String>)> {
    tree.keys()
        .filter_map(|p| {
            Some((
                p.clone(),
                p.strip_suffix(".md")?.split('/').map(key).collect(),
            ))
        })
        .collect()
}

fn resolve<'a>(notes: &'a [(String, Vec<String>)], target: &str) -> Vec<&'a String> {
    let (anchored, body) = match target.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, target),
    };
    let want: Vec<String> = body.split('/').map(key).collect();
    notes
        .iter()
        .filter(|(_, k)| {
            if anchored {
                *k == want
            } else {
                k.ends_with(&want)
            }
        })
        .map(|(p, _)| p)
        .collect()
}

fn text_of(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    Some(s.strip_prefix('\u{feff}').unwrap_or(s).to_string())
}

fn headings_of(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim_start_matches('#');
            (rest.len() < l.len() && rest.starts_with(' ')).then(|| rest.trim().to_string())
        })
        .collect()
}

fn vault(notes: usize, seed: u64) -> (TempDir, Manifest, BTreeMap<String, Vec<u8>>) {
    let dir = TempDir::new("links");
    let manifest = generate(dir.path(), &Config::new(notes, seed)).unwrap();
    let tree = read_tree(dir.path());
    (dir, manifest, tree)
}

#[test]
fn link_counts_by_kind_match_manifest() {
    let (_dir, manifest, tree) = vault(1500, 5);
    let notes = note_keys(&tree);

    let (mut total, mut plain, mut alias, mut heading, mut path, mut root) = (0, 0, 0, 0, 0, 0);
    let (mut unresolved, mut ambiguous, mut case_variant, mut in_code) = (0, 0, 0, 0);
    for (file, bytes) in &tree {
        if !file.ends_with(".md") {
            continue;
        }
        let Some(text) = text_of(bytes) else { continue };
        let s = scan(&text);
        in_code += s.in_code;
        for inner in &s.links {
            let p = parts(inner);
            total += 1;
            let is_root = p.target.starts_with('/');
            let is_path = !is_root && p.target.contains('/');
            root += is_root as usize;
            path += is_path as usize;
            heading += p.heading.is_some() as usize;
            alias += p.alias as usize;
            plain += (!is_root && !is_path && p.heading.is_none() && !p.alias) as usize;
            let hits = resolve(&notes, p.target);
            match hits.len() {
                0 => unresolved += 1,
                1 => {
                    let written = p.target.rsplit('/').next().unwrap();
                    let actual = hits[0]
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .strip_suffix(".md")
                        .unwrap();
                    case_variant += (written != actual) as usize;
                }
                _ => {
                    assert!(
                        !p.target.contains('/'),
                        "ссылка с путём неоднозначна: [[{inner}]] в {file}"
                    );
                    ambiguous += 1;
                }
            }
        }
    }

    let m = &manifest.links;
    assert_eq!(
        (m.total, m.plain, m.alias, m.heading, m.path, m.root),
        (total, plain, alias, heading, path, root),
        "(всего, простые, псевдоним, заголовок, путь, от корня)"
    );
    assert_eq!(
        (m.unresolved, m.ambiguous_bare, m.case_variant, m.in_code),
        (unresolved, ambiguous, case_variant, in_code),
        "(неразрешённые, без пути при дубликатах, другой регистр, внутри кода)"
    );
    for (name, n) in [
        ("простые", plain),
        ("псевдоним", alias),
        ("заголовок", heading),
        ("путь", path),
        ("от корня", root),
        ("неразрешённые", unresolved),
        ("без пути при дубликатах", ambiguous),
        ("другой регистр", case_variant),
        ("внутри кода", in_code),
    ] {
        assert!(n > 0, "нет ссылок вида «{name}»");
    }
}

#[test]
fn expected_links_resolve_as_manifest_says() {
    let (_dir, manifest, tree) = vault(1500, 9);
    let notes = note_keys(&tree);
    assert!(
        manifest.expected_links.len() > 100,
        "мало ожидаемых разрешений"
    );

    for e in &manifest.expected_links {
        let text = text_of(&tree[&e.source]).unwrap();
        let s = scan(&text);
        let inner = e
            .link
            .strip_prefix("[[")
            .and_then(|x| x.strip_suffix("]]"))
            .unwrap();
        assert!(
            s.links.iter().any(|l| l == inner),
            "{} нет в {}",
            e.link,
            e.source
        );

        let p = parts(inner);
        let hits: BTreeSet<String> = resolve(&notes, p.target).into_iter().cloned().collect();
        match e.resolution {
            Resolution::Unresolved => assert!(hits.is_empty() && e.target.is_none(), "{}", e.link),
            Resolution::Unique => {
                assert_eq!(hits.len(), 1, "{}", e.link);
                assert_eq!(e.target.as_ref(), hits.iter().next(), "{}", e.link);
            }
            Resolution::Oldest => {
                assert!(hits.len() > 1, "{}", e.link);
                assert_eq!(
                    e.candidates.iter().cloned().collect::<BTreeSet<_>>(),
                    hits,
                    "{}",
                    e.link
                );
                assert!(hits.contains(e.target.as_ref().unwrap()));
            }
        }
        assert_eq!(e.heading.as_deref(), p.heading, "{}", e.link);
        if let (Some(h), Some(target)) = (&e.heading, &e.target) {
            let found = headings_of(&text_of(&tree[target]).unwrap()).contains(h);
            assert_eq!(
                e.heading_exists,
                Some(found),
                "{}: заголовок «{h}» в {target}",
                e.link
            );
        }
    }
}

#[test]
fn sample_notes_hold_every_kind_and_code_is_not_a_link() {
    let (_dir, manifest, tree) = vault(200, 1);
    let all = text_of(&tree["Все виды ссылок.md"]).unwrap();
    let links = scan(&all).links;
    for inner in [
        "Ёлка",
        "Заметка 10|десятая заметка",
        "Заметка 2#Итоги",
        "Заметка 3#Цели|цели третьей",
        "Альфа/Встреча",
        "/План",
        "Работа/План",
        "Архив/2024/Отчёт",
        "Встреча",
        "заметка 20",
        "йогурт",
        "Заметка 1#Нет такого заголовка",
        "Несуществующая заметка",
        "Нет такой папки/Заметка",
        "/Нет в корне",
    ] {
        assert!(
            links.iter().any(|l| l == inner),
            "нет [[{inner}]] в «Все виды ссылок»"
        );
    }

    let code = scan(&text_of(&tree["Код и не ссылки.md"]).unwrap());
    assert_eq!(
        code.links,
        vec!["Заметка 1".to_string()],
        "вне кода — только одна ссылка"
    );
    assert_eq!(
        code.in_code, 5,
        "[[...]] внутри кода: ```, ~~~, отступ, `…`, ``…``"
    );

    let e = manifest
        .expected_links
        .iter()
        .find(|e| e.link == "[[Встреча]]")
        .unwrap();
    assert_eq!(e.resolution, Resolution::Oldest);
    assert_eq!(e.target.as_deref(), Some("Проекты/Альфа/Встреча.md"));
    let e = manifest
        .expected_links
        .iter()
        .find(|e| e.link == "[[йогурт]]")
        .unwrap();
    assert_eq!(e.target.as_deref(), Some("Юникод/И\u{306}огурт.md"));
}
