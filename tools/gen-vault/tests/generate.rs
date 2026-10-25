//! Состав и детерминированность сгенерированного хранилища.

mod common;

use common::{TempDir, read_tree, read_tree_all};
use gen_vault::{Config, generate};

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
fn same_seed_gives_byte_identical_vault() {
    let a = TempDir::new("seed-a");
    let b = TempDir::new("seed-b");
    let c = TempDir::new("seed-c");
    let ma = generate(a.path(), &Config::new(400, 42)).unwrap();
    let mb = generate(b.path(), &Config::new(400, 42)).unwrap();
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
