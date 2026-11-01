//! Состав и детерминированность сгенерированного хранилища.

mod common;

use common::{TempDir, key, read_tree, read_tree_all};
use gen_vault::{Config, generate};
use std::collections::BTreeMap;

#[test]
fn creates_exactly_n_notes_and_no_service_folder() {
    let dir = TempDir::new("count");
    let manifest = generate(dir.path(), &Config::new(300, 1)).unwrap();

    let tree = read_tree(dir.path());
    let notes = tree.keys().filter(|p| p.ends_with(".md")).count();
    assert_eq!(notes, 300);
    assert_eq!(manifest.notes, 300);
    assert!(!dir.path().join(".rhizomind").exists());
}

#[test]
fn same_seed_same_vault() {
    let a = TempDir::new("seed-a");
    let b = TempDir::new("seed-b");
    let c = TempDir::new("seed-c");
    let cfg = Config {
        dup_share: 12.0,
        ..Config::new(400, 42)
    };
    let ma = generate(a.path(), &cfg).unwrap();
    let mb = generate(b.path(), &cfg).unwrap();
    generate(c.path(), &Config::new(400, 43)).unwrap();

    assert!(
        read_tree_all(a.path()) == read_tree_all(b.path()),
        "одинаковый seed — разные хранилища"
    );
    assert_eq!(
        serde_json::to_string(&ma).unwrap(),
        serde_json::to_string(&mb).unwrap()
    );
    assert!(
        read_tree(a.path()) != read_tree(c.path()),
        "разный seed — одинаковые хранилища"
    );
}

#[test]
fn rejects_big_file_over_the_cap() {
    let dir = TempDir::new("cap");
    let mut cfg = Config::new(100, 1);
    cfg.big_mb = Some(1025);
    let err = generate(dir.path(), &cfg).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        read_tree_all(dir.path()).is_empty(),
        "при ошибке ничего не создано"
    );
}

/// Доля заметок, имя которых (по ключу) есть ещё у какой-то заметки.
fn dup_share_of(tree: &BTreeMap<String, Vec<u8>>) -> f64 {
    let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
    let names: Vec<String> = tree
        .keys()
        .filter_map(|p| p.strip_suffix(".md"))
        .map(|p| key(p.rsplit('/').next().unwrap()))
        .collect();
    for n in &names {
        *by_key.entry(n.clone()).or_default() += 1;
    }
    let dup = names.iter().filter(|n| by_key[*n] > 1).count();
    dup as f64 * 100.0 / names.len() as f64
}

#[test]
fn dup_share_respected() {
    assert_eq!(Config::new(10, 1).dup_share, 5.0, "по умолчанию 5 %");
    for share in [5.0, 20.0] {
        let dir = TempDir::new("dup");
        let cfg = Config {
            dup_share: share,
            ..Config::new(1500, 3)
        };
        let m = generate(dir.path(), &cfg).unwrap();
        let got = dup_share_of(&read_tree(dir.path()));
        assert!(
            (got - share).abs() <= 0.5,
            "доля повторов {got:.2} %, ждали {share} %"
        );
        assert_eq!(m.dup_share, share);
        let in_groups: usize = m.duplicates.iter().map(|g| g.notes.len()).sum();
        assert!((in_groups as f64 * 100.0 / 1500.0 - got).abs() < 1e-9);
    }
    let dir = TempDir::new("dup-bad");
    let cfg = Config {
        dup_share: 51.0,
        ..Config::new(100, 1)
    };
    assert_eq!(
        generate(dir.path(), &cfg).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
}
