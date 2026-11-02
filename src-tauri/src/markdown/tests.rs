//! Тесты разбора: общие образцы `tests/fixtures/` (договор с редактором,
//! задача 0.2-0.3), свойства (`proptest`), сверка с генератором хранилищ и
//! замер скорости (`--ignored markdown_parse_10k`).

use std::ops::Range;

use serde_json::Value;

use super::*;

const TAGS: &str = include_str!("../../../tests/fixtures/tags/text.json");
const TAG_KEYS: &str = include_str!("../../../tests/fixtures/tags/key.json");
const FRONTMATTER: &str = include_str!("../../../tests/fixtures/frontmatter/tags.json");
const HEADINGS: &str = include_str!("../../../tests/fixtures/headings/text.json");
const LINKS: &str = include_str!("../../../tests/fixtures/relink/links.json");
const WIKILINKS: &str = include_str!("../../../tests/fixtures/wikilinks/text.json");

fn cases(fixture: &str) -> Vec<Value> {
    let doc: Value = serde_json::from_str(fixture).expect("образец — JSON");
    assert_eq!(doc["format_version"], 1);
    doc["cases"].as_array().expect("cases").clone()
}

fn bytes(v: &Value) -> Range<usize> {
    let n = |k: &str| usize::try_from(v[k].as_u64().expect(k)).expect("usize");
    n("byte_from")..n("byte_to")
}

fn opt_bytes(v: &Value) -> Option<Range<usize>> {
    if v.is_null() { None } else { Some(bytes(v)) }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("{key}"))
}

/// Смещение UTF-16 → байт UTF-8 (образцы `wikilinks/text.json` — в UTF-16).
fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (at, c) in text.char_indices() {
        if seen >= units {
            return at;
        }
        seen += c.len_utf16();
    }
    text.len()
}

#[test]
fn fixtures_tags() {
    let all = cases(TAGS);
    assert!(all.len() >= 60);
    for case in &all {
        let text = str_of(case, "text");
        let expected: Vec<(String, Range<usize>)> = case["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .map(|t| (str_of(t, "name").to_owned(), bytes(t)))
            .collect();
        let got: Vec<(String, Range<usize>)> = scan(text)
            .tags
            .into_iter()
            .filter(|t| t.tag.source == TagSource::Text)
            .map(|t| (t.tag.name, t.span))
            .collect();
        assert_eq!(
            got,
            expected,
            "{} {}: {text:?}",
            str_of(case, "rule"),
            str_of(case, "note")
        );
        for (name, span) in &got {
            assert_eq!(&text[span.clone()], format!("#{name}"));
        }
    }
}

#[test]
fn fixtures_tag_keys() {
    for case in cases(TAG_KEYS) {
        let same = tag_key(str_of(&case, "a")) == tag_key(str_of(&case, "b"));
        assert_eq!(
            same,
            case["same"].as_bool().expect("same"),
            "{}",
            str_of(&case, "note")
        );
    }
}

#[test]
fn fixtures_frontmatter() {
    let all = cases(FRONTMATTER);
    assert!(all.len() >= 15);
    for case in &all {
        let text = str_of(case, "text");
        let scanned = scan(text);
        let note = format!(
            "{} {}: {text:?}",
            str_of(case, "rule"),
            str_of(case, "note")
        );
        assert_eq!(
            scanned.frontmatter,
            opt_bytes(&case["block"]),
            "блок, {note}"
        );
        let expected: Vec<&str> = case["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .map(|t| t.as_str().expect("тег"))
            .collect();
        let got: Vec<&str> = scanned
            .tags
            .iter()
            .filter(|t| t.tag.source == TagSource::Frontmatter)
            .map(|t| t.tag.name.as_str())
            .collect();
        assert_eq!(got, expected, "теги, {note}");
    }
}

#[test]
fn fixtures_headings() {
    for case in cases(HEADINGS) {
        let text = str_of(&case, "text");
        let expected: Vec<(u8, String, Range<usize>)> = case["headings"]
            .as_array()
            .expect("headings")
            .iter()
            .map(|h| {
                let level = u8::try_from(h["level"].as_u64().expect("level")).expect("u8");
                (level, str_of(h, "text").to_owned(), bytes(h))
            })
            .collect();
        let got: Vec<(u8, String, Range<usize>)> = scan(text)
            .headings
            .into_iter()
            .map(|h| (h.heading.level, h.heading.text, h.span))
            .collect();
        assert_eq!(got, expected, "{}: {text:?}", str_of(&case, "note"));
    }
}

#[test]
fn fixtures_wikilinks_offsets() {
    // relink/links.json: ссылка, цель, заголовок и псевдоним — в байтах.
    for case in cases(LINKS) {
        let text = str_of(&case, "text");
        let note = format!("{}: {text:?}", str_of(&case, "note"));
        let expected = case["links"].as_array().expect("links");
        let got = scan(text).links;
        assert_eq!(got.len(), expected.len(), "число ссылок, {note}");
        for (g, e) in got.iter().zip(expected) {
            assert_eq!(g.link.span, bytes(e), "ссылка, {note}");
            assert_eq!(&text[g.link.span.clone()], str_of(e, "raw"), "{note}");
            assert_eq!(g.link.embed, e["embed"].as_bool().expect("embed"), "{note}");
            assert_eq!(
                Some(g.link.target_span.clone()),
                opt_bytes(&e["target"]),
                "цель, {note}"
            );
            assert_eq!(g.link.target, text[g.link.target_span.clone()], "{note}");
            assert_eq!(
                g.heading_span,
                opt_bytes(&e["heading"]),
                "заголовок, {note}"
            );
            assert_eq!(g.alias_span, opt_bytes(&e["alias"]), "псевдоним, {note}");
            let part = |r: &Option<Range<usize>>| r.clone().map(|r| text[r].to_owned());
            assert_eq!(g.link.heading, part(&g.heading_span), "{note}");
            assert_eq!(g.link.alias, part(&g.alias_span), "{note}");
        }
    }
    // wikilinks/text.json (выпуск 0.1): те же места ссылок, смещения UTF-16.
    for case in cases(WIKILINKS) {
        let text = str_of(&case, "text");
        let expected: Vec<(Range<usize>, bool)> = case["links"]
            .as_array()
            .expect("links")
            .iter()
            .map(|l| {
                let at = |k: &str| {
                    let units = usize::try_from(l[k].as_u64().expect(k)).expect("usize");
                    utf16_to_byte(text, units)
                };
                (at("from")..at("to"), l["embed"].as_bool().expect("embed"))
            })
            .collect();
        let got: Vec<(Range<usize>, bool)> = parse(text)
            .links
            .into_iter()
            .map(|l| (l.span, l.embed))
            .collect();
        assert_eq!(got, expected, "{}: {text:?}", str_of(&case, "note"));
    }
}

#[test]
fn line_numbers_count_lf_crlf_and_lone_cr() {
    let parsed = parse("---\ntags: [a]\n---\r\n# Заг\r\n#б\rтекст [[В]]\n\n- [[Г]] #д");
    let tags: Vec<(&str, TagSource, u32)> = parsed
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.source, t.line))
        .collect();
    assert_eq!(
        tags,
        [
            ("a", TagSource::Frontmatter, 2),
            ("б", TagSource::Text, 5),
            ("д", TagSource::Text, 8)
        ]
    );
    let links: Vec<(&str, u32)> = parsed
        .links
        .iter()
        .map(|l| (l.target.as_str(), l.line))
        .collect();
    assert_eq!(links, [("В", 6), ("Г", 8)]);
    assert_eq!(
        parsed.headings,
        [Heading {
            level: 1,
            text: "Заг".to_owned(),
            line: 4
        }]
    );
}

#[test]
fn frontmatter_ignored_in_notes_over_10_mb() {
    let block = "---\ntags: [проект]\ntitle: #в_блоке\n---\n";
    let mut big = String::from(block);
    let line = "Строка текста без разметки.\n";
    while big.len() <= FRONTMATTER_MAX_NOTE_BYTES {
        big.push_str(line);
    }
    let parsed = parse(&big);
    let names: Vec<(&str, TagSource)> = parsed
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.source))
        .collect();
    assert_eq!(
        names,
        [("в_блоке", TagSource::Text)],
        "блок не распознан — его строки как текст"
    );

    let exact = format!(
        "{block}{}",
        "x".repeat(FRONTMATTER_MAX_NOTE_BYTES - block.len())
    );
    assert_eq!(exact.len(), FRONTMATTER_MAX_NOTE_BYTES);
    let names: Vec<(String, TagSource)> = parse(&exact)
        .tags
        .into_iter()
        .map(|t| (t.name, t.source))
        .collect();
    assert_eq!(
        names,
        [("проект".to_owned(), TagSource::Frontmatter)],
        "ровно 10 МБ — блок есть"
    );
}

#[test]
fn markdown_link_destination_hides_tags_on_long_lines_in_linear_time() {
    // Много незакрытых `[…](` и `` ` `` на одной строке: разбор не квадратичен.
    let text = "[a](b ".repeat(200_000) + &"` ``".repeat(100_000) + " #тег";
    let start = std::time::Instant::now();
    let parsed = parse(&text);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(parsed.tags.len(), 1);
}

mod props {
    use proptest::prelude::*;

    use super::super::*;

    /// Текст из символов разметки, букв, эмодзи, комбинируемых знаков и
    /// переносов строк — чтобы чаще получались ссылки, теги, код и блоки.
    fn markup() -> impl Strategy<Value = String> {
        let pieces = prop::sample::select(vec![
            "[[",
            "]]",
            "![[",
            "[",
            "]",
            "(",
            ")",
            "#",
            "##",
            "# ",
            "|",
            "\\|",
            "\\",
            "`",
            "```",
            "~~~",
            "---",
            "...",
            "\n",
            "\r\n",
            "\r",
            " ",
            "\t",
            "    ",
            "- ",
            "tags:",
            "tag: ",
            "\"",
            "'",
            ",",
            "/",
            "//",
            "_",
            "-",
            "a",
            "1",
            "й",
            "и\u{306}",
            "😀",
            "\u{a0}",
            "\u{200b}",
            "&",
            ".md",
            "https://x/#",
            "<",
            ">",
            "Заметка",
        ]);
        prop::collection::vec(pieces, 0..60).prop_map(|v| v.concat())
    }

    fn line_of(text: &str, at: usize) -> u32 {
        let before = &text.as_bytes()[..at];
        let mut line = 1;
        let mut i = 0;
        while i < before.len() {
            match before[i] {
                b'\r' if before.get(i + 1) == Some(&b'\n') => {
                    line += 1;
                    i += 1;
                }
                b'\r' | b'\n' => line += 1,
                _ => {}
            }
            i += 1;
        }
        line
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        #[test]
        fn never_panics(text in markup(), any in "\\PC{0,64}") {
            for t in [text.as_str(), any.as_str()] {
                let scanned = scan(t);
                for tag in &scanned.tags {
                    prop_assert!(!tag.tag.name.is_empty());
                    prop_assert!(t.get(tag.span.clone()).is_some());
                }
                for h in &scanned.headings {
                    prop_assert!((1..=6).contains(&h.heading.level));
                    prop_assert!(t.get(h.span.clone()).is_some());
                }
            }
        }

        #[test]
        fn link_offsets_point_at_brackets(text in markup()) {
            for link in parse(&text).links {
                let raw = &text[link.span.clone()];
                let open = if link.embed { "![[" } else { "[[" };
                prop_assert!(raw.starts_with(open), "{raw:?}");
                prop_assert!(raw.ends_with("]]"), "{raw:?}");
                prop_assert!(!raw.contains('\n') && !raw.contains('\r'));
                prop_assert!(link.span.start + open.len() <= link.target_span.start);
                prop_assert!(link.target_span.end + 2 <= link.span.end);
                prop_assert_eq!(&text[link.target_span.clone()], link.target.as_str());
                prop_assert_eq!(link.line, line_of(&text, link.span.start));
            }
        }
    }
}

/// Заметки сгенерированного хранилища: путь от корня через `/` → текст
/// (без BOM; не UTF-8 пропускается, как в индексе).
fn generated_notes(root: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("папка") {
            let path = entry.expect("запись").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                let Ok(text) = String::from_utf8(std::fs::read(&path).expect("файл")) else {
                    continue;
                };
                let text = text
                    .strip_prefix('\u{feff}')
                    .map(str::to_owned)
                    .unwrap_or(text);
                let rel = path.strip_prefix(root).expect("внутри").to_string_lossy();
                out.push((rel.replace('\\', "/"), text));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn generated_vault_matches_manifest() {
    use std::collections::{BTreeMap, BTreeSet};
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v");
    let manifest = gen_vault::generate(&root, &gen_vault::Config::new(400, 7)).expect("генератор");
    let notes = generated_notes(&root);
    let parsed: BTreeMap<&str, ParsedNote> =
        notes.iter().map(|(p, t)| (p.as_str(), parse(t))).collect();

    // Теги заметок из выборки генератора — по ключу.
    assert!(!manifest.expected_tags.is_empty());
    for expected in &manifest.expected_tags {
        let got: BTreeSet<String> = parsed[expected.source.as_str()]
            .tags
            .iter()
            .map(|t| tag_key(&t.name))
            .collect();
        let want: BTreeSet<String> = expected.tags.iter().cloned().collect();
        assert_eq!(got, want, "{}", expected.source);
    }
    // Все теги хранилища: число заметок и вхождения каждого написания (NFC).
    let mut spellings: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut tag_notes: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for (path, note) in &parsed {
        for tag in &note.tags {
            let key = tag_key(&tag.name);
            let nfc: String =
                unicode_normalization::UnicodeNormalization::nfc(tag.name.as_str()).collect();
            *spellings
                .entry(key.clone())
                .or_default()
                .entry(nfc)
                .or_default() += 1;
            tag_notes.entry(key).or_default().insert(path);
        }
    }
    let want: BTreeMap<String, (usize, BTreeMap<String, usize>)> = manifest
        .tags
        .iter()
        .map(|t| (t.key.clone(), (t.notes, t.spellings.clone())))
        .collect();
    let got: BTreeMap<String, (usize, BTreeMap<String, usize>)> = spellings
        .into_iter()
        .map(|(k, s)| {
            let n = tag_notes[&k].len();
            (k, (n, s))
        })
        .collect();
    assert_eq!(got, want);
    // Ссылки выборки: каждая найдена в своей заметке.
    for expected in &manifest.expected_links {
        let found = parsed[expected.source.as_str()].links.iter().any(|l| {
            let raw = &notes
                .iter()
                .find(|(p, _)| *p == expected.source)
                .expect("заметка")
                .1[l.span.clone()];
            l.embed == expected.embed && raw.trim_start_matches('!') == expected.link
        });
        assert!(found, "{} {}", expected.source, expected.link);
    }
}

/// Замер (задача 0.2-1.1): разбор 10 000 сгенерированных заметок — не больше
/// 1,5 с в выпускной сборке. Запуск:
/// `cargo test -p rhizomind --release --lib markdown::tests::markdown_parse_10k -- --ignored --nocapture`.
#[test]
#[ignore = "замер: --release --ignored"]
fn markdown_parse_10k() {
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v10k");
    gen_vault::generate(&root, &gen_vault::Config::new(10_000, 7)).expect("генератор");
    let notes = generated_notes(&root);
    assert!(notes.len() >= 10_000, "{}", notes.len());
    let total: usize = notes.iter().map(|(_, t)| t.len()).sum();
    let start = std::time::Instant::now();
    let mut found = (0, 0, 0);
    for (_, text) in &notes {
        let p = parse(text);
        found.0 += p.headings.len();
        found.1 += p.tags.len();
        found.2 += p.links.len();
    }
    let elapsed = start.elapsed();
    println!(
        "markdown_parse_10k: {} заметок, {:.1} МБ, {:?}; заголовков {}, тегов {}, ссылок {}",
        notes.len(),
        total as f64 / 1_048_576.0,
        elapsed,
        found.0,
        found.1,
        found.2
    );
    if !cfg!(debug_assertions) {
        assert!(
            elapsed <= std::time::Duration::from_millis(1500),
            "{elapsed:?}"
        );
    }
}

#[test]
fn unicode_tables_match_typescript() {
    use super::unicode_tables::{
        DECIMAL_DIGIT, PUNCT_SYMBOL, TAG_WORD, UNICODE_VERSION, WHITE_SPACE,
    };
    const TS: &str = include_str!("../../../src/core/tags/unicode-tables.ts");
    assert!(TS.contains(&format!("UNICODE_VERSION = '{UNICODE_VERSION}'")));
    for (name, table) in [
        ("PUNCT_SYMBOL", PUNCT_SYMBOL),
        ("WHITE_SPACE", WHITE_SPACE),
        ("TAG_WORD", TAG_WORD),
        ("DECIMAL_DIGIT", DECIMAL_DIGIT),
    ] {
        assert!(
            table.windows(2).all(|w| w[0].1 + 1 < w[1].0),
            "{name}: по возрастанию, без стыков"
        );
        assert!(table.iter().all(|(lo, hi)| lo <= hi), "{name}");
        let start = TS
            .find(&format!("export const {name}: readonly number[] = ["))
            .expect(name);
        let body = &TS[start..];
        let body = &body[body.find("= [").expect("= [") + 3..body.find("];").expect("];")];
        let ts: Vec<u32> = body
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("число"))
            .collect();
        let rs: Vec<u32> = table.iter().flat_map(|&(lo, hi)| [lo, hi]).collect();
        assert_eq!(
            ts, rs,
            "{name}: таблицы Rust и TypeScript из одной генерации"
        );
    }
}

#[test]
fn link_and_image_destinations_and_escapes() {
    let names = |t: &str| {
        parse(t)
            .tags
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("![схема](#якорь) #после"), ["после"]);
    assert_eq!(names("[a](<#x y>) [b](c \"#t\") [d](e (#f)) #ок"), ["ок"]);
    assert_eq!(
        names("\\[a](#x)"),
        ["x"],
        "экранированная [ не открывает ссылку"
    );
    assert_eq!(names("[a [b](#c)](#d) #e"), ["e"]);
    assert_eq!(
        names("[a](#b"),
        ["b"],
        "без ) — не ссылка: перед # знак (, это тег"
    );
}
