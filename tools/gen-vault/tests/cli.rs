//! Командная строка: `gen-vault --notes N --out <папка> [--seed S] [--big-mb M] [--manifest <файл>]`.

mod common;

use common::{TempDir, read_tree};
use gen_vault::Manifest;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gen-vault"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn creates_vault_and_writes_manifest() {
    let dir = TempDir::new("cli");
    let out = dir.path().join("vault");
    let manifest = dir.path().join("manifest.json");
    let o = run(&[
        "--notes",
        "120",
        "--out",
        out.to_str().unwrap(),
        "--seed",
        "3",
        "--big-mb",
        "1",
        "--manifest",
        manifest.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));

    let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!((m.notes, m.seed, m.format_version), (120, 3, 1));
    assert!(m.encodings.samples.big.is_some());
    let tree = read_tree(&out);
    assert_eq!(tree.keys().filter(|p| p.ends_with(".md")).count(), 120);
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("120"),
        "сводка в выводе"
    );
}

#[test]
fn same_seed_from_command_line_gives_same_vault() {
    let dir = TempDir::new("cli-seed");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    for out in [&a, &b] {
        assert!(
            run(&[
                "--notes",
                "100",
                "--out",
                out.to_str().unwrap(),
                "--seed",
                "77"
            ])
            .status
            .success()
        );
    }
    assert!(read_tree(&a) == read_tree(&b));
}

#[test]
fn rejects_bad_arguments() {
    let dir = TempDir::new("cli-bad");
    let out = dir.path().join("vault");
    let out = out.to_str().unwrap();

    let o = run(&["--notes", "10"]);
    assert_eq!(o.status.code(), Some(2), "без --out");
    assert!(stderr(&o).contains("--out"));

    let o = run(&["--notes", "abc", "--out", out]);
    assert_eq!(o.status.code(), Some(2), "не число");

    let o = run(&["--notes", "10", "--out", out, "--colour"]);
    assert_eq!(o.status.code(), Some(2), "неизвестный параметр");

    let o = run(&["--notes", "5", "--out", out]);
    assert_eq!(o.status.code(), Some(1), "меньше образцов");
    assert!(stderr(&o).contains("не меньше"), "{}", stderr(&o));

    std::fs::create_dir_all(dir.path().join("busy")).unwrap();
    std::fs::write(dir.path().join("busy/note.md"), "x").unwrap();
    let o = run(&[
        "--notes",
        "100",
        "--out",
        dir.path().join("busy").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1), "непустая папка");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("busy/note.md")).unwrap(),
        "x",
        "чужой файл не тронут"
    );

    let inside = format!("{out}/manifest.json");
    let o = run(&["--notes", "100", "--out", out, "--manifest", &inside]);
    assert_eq!(o.status.code(), Some(2), "манифест внутри хранилища");
}
