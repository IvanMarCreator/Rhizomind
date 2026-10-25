//! Образцы кодировок и переносов строк, большой файл, вложения PNG и PDF.

mod common;

use common::{TempDir, is_hidden, read_tree, read_tree_all};
use gen_vault::{Config, generate};

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Переносы строк файла: "lf", "crlf", "mixed" или "none".
fn eol_kind(bytes: &[u8]) -> &'static str {
    let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();
    let lf = bytes.iter().filter(|&&c| c == b'\n').count();
    match (crlf, lf - crlf) {
        (0, 0) => "none",
        (0, _) => "lf",
        (_, 0) => "crlf",
        _ => "mixed",
    }
}

#[test]
fn encoding_samples_and_counts_match_manifest() {
    let dir = TempDir::new("enc");
    let m = generate(dir.path(), &Config::new(1500, 21)).unwrap();
    let tree = read_tree(dir.path());
    let notes: Vec<(&String, &Vec<u8>)> = tree.iter().filter(|(p, _)| p.ends_with(".md")).collect();

    let count = |kind: &str| notes.iter().filter(|(_, b)| eol_kind(b) == kind).count();
    let e = &m.encodings;
    assert_eq!(
        (e.lf, e.crlf, e.mixed),
        (count("lf"), count("crlf"), count("mixed")),
        "(LF, CRLF, смешанные)"
    );
    assert_eq!(
        e.bom,
        notes.iter().filter(|(_, b)| b.starts_with(BOM)).count(),
        "с BOM"
    );
    assert_eq!(
        e.no_final_newline,
        notes.iter().filter(|(_, b)| !b.ends_with(b"\n")).count()
    );
    assert!(
        e.crlf > 10 && e.mixed > 0 && e.bom > 10,
        "массовые заметки тоже бывают CRLF, смешанные, с BOM"
    );

    let s = &e.samples;
    let file = |p: &str| tree.get(p).unwrap_or_else(|| panic!("нет образца {p}"));
    assert!(eol_kind(file(&s.lf)) == "lf" && !file(&s.lf).starts_with(BOM));
    assert!(eol_kind(file(&s.crlf)) == "crlf" && !file(&s.crlf).starts_with(BOM));
    assert!(eol_kind(file(&s.lf_bom)) == "lf" && file(&s.lf_bom).starts_with(BOM));
    assert!(eol_kind(file(&s.crlf_bom)) == "crlf" && file(&s.crlf_bom).starts_with(BOM));
    assert_eq!(eol_kind(file(&s.mixed)), "mixed");
    assert!(!file(&s.no_final_newline).ends_with(b"\n"));
    assert!(
        std::str::from_utf8(file(&s.invalid_utf8)).is_err(),
        "образец невалидного UTF-8 валиден"
    );
    let invalid = notes
        .iter()
        .filter(|(_, b)| std::str::from_utf8(b).is_err())
        .count();
    assert_eq!(invalid, 1, "невалидный UTF-8 — ровно один файл");
    assert!(
        s.big.is_none() && !tree.keys().any(|p| p.contains("Большой файл")),
        "без --big-mb большого файла нет"
    );
}

#[test]
fn big_file_is_larger_than_the_limit() {
    let dir = TempDir::new("big");
    let mut cfg = Config::new(100, 2);
    cfg.big_mb = Some(1);
    let m = generate(dir.path(), &cfg).unwrap();

    let big = m
        .encodings
        .samples
        .big
        .expect("нет большого файла в манифесте");
    let size = std::fs::metadata(dir.path().join(&big.path)).unwrap().len();
    assert!(size > 1024 * 1024, "размер {size} не больше 1 МБ");
    assert_eq!((big.bytes, big.limit_mb), (size, 1));
    assert_eq!(
        read_tree(dir.path())
            .keys()
            .filter(|p| p.ends_with(".md"))
            .count(),
        100
    );
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Имена фрагментов PNG по порядку; каждый с верной контрольной суммой.
fn png_chunks(bytes: &[u8]) -> Vec<String> {
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "нет подписи PNG");
    let mut chunks = Vec::new();
    let mut i = 8;
    while i < bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let body = &bytes[i + 4..i + 8 + len];
        let crc = u32::from_be_bytes(bytes[i + 8 + len..i + 12 + len].try_into().unwrap());
        assert_eq!(crc32(body), crc, "контрольная сумма фрагмента");
        chunks.push(String::from_utf8(body[..4].to_vec()).unwrap());
        i += 12 + len;
    }
    chunks
}

#[test]
fn attachments_are_minimal_valid_png_and_pdf() {
    let dir = TempDir::new("att");
    let m = generate(dir.path(), &Config::new(800, 4)).unwrap();
    let tree = read_tree(dir.path());

    let png: Vec<_> = tree.iter().filter(|(p, _)| p.ends_with(".png")).collect();
    let pdf: Vec<_> = tree.iter().filter(|(p, _)| p.ends_with(".pdf")).collect();
    assert_eq!(
        (m.attachments.png, m.attachments.pdf),
        (png.len(), pdf.len())
    );
    assert!(png.len() >= 2 && pdf.len() >= 2);
    let mut paths: Vec<&String> = png.iter().chain(&pdf).map(|(p, _)| *p).collect();
    paths.sort();
    let mut listed: Vec<&String> = m.attachments.paths.iter().collect();
    listed.sort();
    assert_eq!(paths, listed);
    assert!(paths.iter().any(|p| p.starts_with("Вложения/")));
    assert!(
        paths.iter().any(|p| !p.starts_with("Вложения/")),
        "вложения есть и в других папках"
    );

    for (path, bytes) in png {
        assert_eq!(png_chunks(bytes), ["IHDR", "IDAT", "IEND"], "{path}");
    }
    for (path, bytes) in pdf {
        let text = String::from_utf8_lossy(bytes);
        assert!(
            text.starts_with("%PDF-1.4\n") && text.ends_with("%%EOF\n"),
            "{path}"
        );
        let start: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            text[start..].starts_with("xref\n"),
            "{path}: startxref указывает не на xref"
        );
    }
}

#[test]
fn hidden_entries_other_attachments_and_hash_in_name() {
    let dir = TempDir::new("extra");
    let m = generate(dir.path(), &Config::new(300, 6)).unwrap();
    let all = read_tree_all(dir.path());

    // Скрытые файл и папка (С3.2): в число заметок не входят.
    let hidden: Vec<String> = all.keys().filter(|p| is_hidden(p)).cloned().collect();
    assert_eq!(hidden, m.hidden);
    assert!(
        hidden.iter().any(|p| !p.contains('/')),
        "скрытый файл в корне"
    );
    assert!(
        hidden
            .iter()
            .any(|p| p.split('/').next().unwrap().starts_with('.') && p.contains('/')),
        "скрытая папка"
    );

    // Вложения других форматов.
    for ext in [".txt", ".csv", ".zip"] {
        assert!(
            m.attachments.other.iter().any(|p| p.ends_with(ext)),
            "нет вложения {ext}"
        );
    }
    for p in &m.attachments.other {
        let bytes = &all[p];
        if p.ends_with(".zip") {
            assert!(
                bytes.len() >= 22 && bytes[bytes.len() - 22..].starts_with(b"PK\x05\x06"),
                "{p}: не zip"
            );
        }
    }

    // Имя с `#` есть, но ссылок на него нет: `#` в ссылке — начало заголовка.
    let name = &m.samples.hash_in_name;
    assert!(all.contains_key(name), "нет {name}");
    let stem = name
        .rsplit('/')
        .next()
        .unwrap()
        .strip_suffix(".md")
        .unwrap();
    let base = stem.split('#').next().unwrap();
    for (p, bytes) in read_tree(dir.path()) {
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(&format!("[[{base}#")),
            "ссылка на имя с # в {p}"
        );
    }
}
