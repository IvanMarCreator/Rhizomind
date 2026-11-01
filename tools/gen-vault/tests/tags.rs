//! Теги в тексте и в блоке свойств (С2-3.3, решение 113), заголовки.
//! Разбор — свой, по правилам спецификации, независимо от генератора.

mod common;

use common::{TempDir, key, read_tree};
use gen_vault::{Config, Manifest, generate};
use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Тег по правилам С2-3.3: тело, хотя бы одна не-цифра, `/` не в начале,
/// не в конце и не дважды подряд, до 200 символов.
fn valid_tag(body: &str) -> bool {
    !body.is_empty()
        && body.chars().count() <= 200
        && body.chars().all(tag_char)
        && body.chars().any(|c| !c.is_ascii_digit())
        && !body.starts_with('/')
        && !body.ends_with('/')
        && !body.contains("//")
}

fn tag_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c) || matches!(c, '_' | '-' | '/')
}

/// Найденные теги заметки в порядке файла: сначала блок свойств, потом текст.
#[derive(Default, Debug)]
struct Found {
    front: Vec<String>,
    text: Vec<String>,
    /// Ключи блока свойств (как написаны) и формы `tags`/`tag`.
    keys: Vec<String>,
    forms: BTreeSet<&'static str>,
    skipped: usize,
}

impl Found {
    fn all(&self) -> impl Iterator<Item = &String> {
        self.front.iter().chain(&self.text)
    }
    fn keys_set(&self) -> BTreeSet<String> {
        self.all().map(|t| key(t)).collect()
    }
}

fn parse(text: &str) -> Found {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut f = Found::default();
    let mut body_from = 0;
    if lines.first() == Some(&"---")
        && let Some(end) = lines[1..].iter().position(|l| *l == "---")
    {
        frontmatter(&lines[1..=end], &mut f);
        body_from = end + 2;
    }
    let mut fence = false;
    for line in &lines[body_from..] {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !fence {
            text_tags(line, &mut f.text);
        }
    }
    f
}

fn frontmatter(lines: &[&str], f: &mut Found) {
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.starts_with(' ') {
            continue;
        }
        f.keys.push(k.to_string());
        let lk = k.to_lowercase();
        if lk != "tags" && lk != "tag" {
            continue;
        }
        if k != "tags" {
            f.forms.insert("ключ в другом регистре или tag");
        }
        let v = v.trim();
        let values: Vec<String> = if v.is_empty() {
            f.forms.insert("список строками");
            let mut out = Vec::new();
            while i < lines.len() {
                match lines[i].trim_start().strip_prefix("- ") {
                    Some(item) if lines[i].starts_with(' ') => out.push(item.to_string()),
                    _ => break,
                }
                i += 1;
            }
            out
        } else if let Some(inner) = v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            f.forms.insert("[a, b]");
            inner.split(',').map(str::to_string).collect()
        } else if v.contains(',') {
            f.forms.insert("a, b");
            v.split(',').map(str::to_string).collect()
        } else {
            f.forms.insert("одно значение");
            vec![v.to_string()]
        };
        for raw in values {
            let mut s = raw.trim();
            for q in ['"', '\''] {
                if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
                    s = &s[1..s.len() - 1];
                    f.forms.insert("в кавычках");
                }
            }
            if let Some(rest) = s.strip_prefix('#') {
                s = rest;
                f.forms.insert("с # впереди");
            }
            if valid_tag(s) {
                f.front.push(s.to_string());
            } else {
                f.skipped += 1;
            }
        }
    }
}

/// Теги в строке вне блоков кода: пропускаются код в строке, вики-ссылки и
/// ссылки Markdown; перед `#` — начало строки, пробел или `( « " '`.
fn text_tags(line: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect();
        if c == '`' {
            let end = chars[i + 1..].iter().position(|&x| x == '`');
            i += end.map_or(1, |e| e + 2);
            continue;
        }
        if rest.starts_with("[[") {
            i += rest.find("]]").map_or(2, |e| rest[..e + 2].chars().count());
            continue;
        }
        if rest.starts_with("](") {
            i += rest.find(')').map_or(2, |e| rest[..e + 1].chars().count());
            continue;
        }
        if c == '#' {
            let before_ok = i == 0
                || chars[i - 1].is_whitespace()
                || matches!(chars[i - 1], '(' | '«' | '"' | '\'');
            let body: String = chars[i + 1..]
                .iter()
                .take_while(|&&x| tag_char(x))
                .collect();
            if before_ok && valid_tag(&body) {
                out.push(body.clone());
            }
            i += 1 + body.chars().count();
            continue;
        }
        i += 1;
    }
}

fn vault(notes: usize, seed: u64) -> (TempDir, Manifest, BTreeMap<String, Vec<u8>>) {
    let dir = TempDir::new("tags");
    let manifest = generate(dir.path(), &Config::new(notes, seed)).unwrap();
    let tree = read_tree(dir.path());
    (dir, manifest, tree)
}

fn notes(tree: &BTreeMap<String, Vec<u8>>) -> Vec<(&String, String)> {
    tree.iter()
        .filter(|(p, _)| p.ends_with(".md"))
        .filter_map(|(p, b)| Some((p, String::from_utf8(b.clone()).ok()?)))
        .collect()
}

#[test]
fn tags_in_text_and_frontmatter() {
    let (dir, manifest, tree) = vault(1500, 11);
    let notes = notes(&tree);

    // Свой подсчёт: ключ тега → заметки и написания.
    let mut by_key: BTreeMap<String, (BTreeSet<&String>, BTreeMap<String, usize>)> =
        BTreeMap::new();
    let (mut forms, mut skipped, mut keys) = (BTreeSet::new(), 0, BTreeSet::new());
    let (mut only_text, mut only_front, mut both, mut tagged) = (0, 0, 0, 0);
    let mut found_of = BTreeMap::new();
    for (path, text) in &notes {
        let f = parse(text);
        for t in f.all() {
            let e = by_key.entry(key(t)).or_default();
            e.0.insert(*path);
            *e.1.entry(t.nfc().collect()).or_default() += 1;
        }
        let n = f.keys_set().len();
        if n > 0 {
            tagged += 1;
            assert!((2..=5).contains(&n), "{path}: {n} тегов, ждали 2–5");
        }
        match (f.front.is_empty(), f.text.is_empty()) {
            (true, false) => only_text += 1,
            (false, true) => only_front += 1,
            (false, false) => both += 1,
            _ => {}
        }
        forms.extend(f.forms.iter().copied());
        skipped += f.skipped;
        keys.extend(f.keys.iter().cloned());
        found_of.insert((*path).clone(), f.keys_set());
    }
    assert!(
        tagged * 100 >= notes.len() * 95,
        "теги есть почти у всех заметок"
    );
    assert!(
        only_text > 0 && only_front > 0 && both > 0,
        "теги только в тексте, только в свойствах, и там и там"
    );
    for form in [
        "[a, b]",
        "a, b",
        "одно значение",
        "список строками",
        "в кавычках",
        "с # впереди",
        "ключ в другом регистре или tag",
    ] {
        assert!(forms.contains(form), "нет формы «{form}»");
    }
    assert!(skipped > 0, "нет значений, которые не теги (пробел, цифры)");
    assert!(keys.contains("tag"), "нет ключа tag");
    assert!(
        keys.iter()
            .any(|k| !["tags", "tag", "Tags", "TAG"].contains(&k.as_str())),
        "в блоке свойств только теги"
    );

    let all: Vec<&String> = by_key.values().flat_map(|(_, s)| s.keys()).collect();
    assert!(all.iter().any(|t| t.contains('/')), "нет тега с /");
    assert!(all.iter().any(|t| t.contains('-')), "нет тега с -");
    assert!(
        all.iter()
            .any(|t| t.chars().any(|c| ('а'..='я').contains(&c))),
        "нет кириллицы"
    );
    assert!(
        by_key.values().any(|(_, s)| s.len() > 1),
        "нет тега в разном написании"
    );
    assert!(by_key.contains_key("работа") && by_key.contains_key("работа/отчёт"));

    // Сводка совпадает со своим подсчётом.
    let got: BTreeMap<String, (usize, BTreeMap<String, usize>)> = manifest
        .tags
        .iter()
        .map(|t| (t.key.clone(), (t.notes, t.spellings.clone())))
        .collect();
    let want: BTreeMap<String, (usize, BTreeMap<String, usize>)> = by_key
        .into_iter()
        .map(|(k, (n, s))| (k, (n.len(), s)))
        .collect();
    assert_eq!(got, want, "теги в сводке");
    assert!(
        manifest.tags.iter().any(|t| t.key == "café") && found_nfd(&notes),
        "café в NFD есть в файлах, в сводке — одно написание NFC"
    );

    check_display(dir.path(), &manifest, &notes);

    assert!(manifest.expected_tags.len() > 100, "мало ожидаемых тегов");
    for e in &manifest.expected_tags {
        let want: BTreeSet<String> = e.tags.iter().cloned().collect();
        assert_eq!(found_of[&e.source], want, "{}", e.source);
    }
}

#[test]
fn tag_traps_are_not_tags() {
    let (_dir, _m, tree) = vault(1500, 12);
    let joined: String = notes(&tree).into_iter().map(|(_, t)| t).collect();
    for trap in ["#123", "#2026", "https://example.org/#", "\\#", "C#"] {
        assert!(joined.contains(trap), "нет ловушки «{trap}»");
    }
}

#[test]
fn bulk_notes_have_3_to_5_headings() {
    let (_dir, _m, tree) = vault(600, 13);
    let notes = notes(&tree);
    let ok = notes
        .iter()
        .filter(|(_, t)| {
            let n = t
                .lines()
                .filter(|l| {
                    let rest = l.trim_start_matches('#');
                    rest.len() < l.len() && l.len() - rest.len() <= 6 && rest.starts_with(' ')
                })
                .count();
            (3..=5).contains(&n)
        })
        .count();
    assert!(
        ok + 5 >= notes.len(),
        "3–5 заголовков: {ok} из {}",
        notes.len()
    );
}

fn found_nfd(notes: &[(&String, String)]) -> bool {
    let nfd: String = "café".nfd().collect();
    notes.iter().any(|(_, t)| t.contains(&nfd))
}

#[test]
fn display_spelling_tie_uses_creation_date() {
    // Малые хранилища: равенство написаний у редких тегов; ищем по зёрнам.
    let mut ties = 0;
    for seed in 1..=30 {
        let (dir, m, tree) = vault(60, seed);
        ties += check_display(dir.path(), &m, &notes(&tree));
        for t in &m.tags {
            assert!(t.spellings.contains_key(&t.display), "{}", t.key);
            assert!(
                t.spellings
                    .keys()
                    .all(|s| s.nfc().collect::<String>() == *s),
                "{}: написания не в NFC",
                t.key
            );
        }
        if ties >= 3 {
            return;
        }
    }
    panic!("за 30 зёрен тегов с равенством написаний: {ties}");
}

/// Сверяет `display` каждого тега с правилом Р18: самое частое по вхождениям;
/// при равенстве — среди равных по максимуму, из самой старой заметки (дата
/// создания, при равных — путь), первое такое вхождение в порядке файла.
/// Возвращает число тегов с равенством.
fn check_display(
    root: &std::path::Path,
    manifest: &Manifest,
    notes: &[(&String, String)],
) -> usize {
    let created = |p: &str| -> SystemTime {
        let m = std::fs::metadata(root.join(p)).unwrap();
        m.created().or_else(|_| m.modified()).unwrap()
    };
    let mut ordered: Vec<(SystemTime, &String, &String)> =
        notes.iter().map(|(p, t)| (created(p), *p, t)).collect();
    ordered.sort();
    let mut ties = 0;
    for t in &manifest.tags {
        let max = *t.spellings.values().max().unwrap();
        let top: BTreeSet<&String> = t
            .spellings
            .iter()
            .filter(|(_, n)| **n == max)
            .map(|(s, _)| s)
            .collect();
        let want = if top.len() == 1 {
            (*top.iter().next().unwrap()).clone()
        } else {
            ties += 1;
            ordered
                .iter()
                .find_map(|(_, _, text)| {
                    parse(text)
                        .all()
                        .map(|s| s.nfc().collect::<String>())
                        .find(|s| key(s) == t.key && top.contains(s))
                })
                .unwrap()
        };
        assert_eq!(t.display, want, "показываемое написание тега {}", t.key);
    }
    ties
}
