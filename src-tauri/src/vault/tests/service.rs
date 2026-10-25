//! Служебные файлы хранилища и создание хранилища: `settings.json` с
//! версией формата, `workspace.json`, `Vault::create`.

use serde_json::json;

use super::*;
use crate::commands::{Settings, Theme, Workspace};

fn settings_json(t: &TestVault) -> serde_json::Value {
    let bytes = std::fs::read(t.root().join(".rhizomind/settings.json")).expect("settings.json");
    serde_json::from_slice(&bytes).expect("JSON")
}

#[test]
fn settings_round_trip_with_format_version() {
    let t = TestVault::new();
    assert_eq!(t.vault.read_settings(), Ok(Settings::default()));

    let mut settings = Settings::default();
    settings.theme = Theme::Dark;
    settings.font_size = 18;
    t.vault.write_settings(&settings).expect("запись");

    assert_eq!(t.vault.read_settings(), Ok(settings));
    let written = settings_json(&t);
    assert_eq!(written["format_version"], 1);
    assert_eq!(written["theme"], "dark");
    assert_eq!(
        std::fs::read_dir(t.root().join(".rhizomind/tmp"))
            .expect("tmp")
            .count(),
        0
    );
}

#[test]
fn writing_settings_keeps_unknown_fields_and_values_of_a_newer_version() {
    let t = TestVault::new();
    std::fs::write(
        t.root().join(".rhizomind/settings.json"),
        r#"{ "format_version": "2", "theme": "sepia", "font_size": 20, "graph": { "physics": true } }"#,
    )
    .expect("файл");
    let read = t.vault.read_settings().expect("чтение");
    assert_eq!(read.theme, Theme::System, "неизвестная тема — по умолчанию");
    assert_eq!(read.font_size, 20);

    let mut changed = read.clone();
    changed.font_size = 14;
    t.vault.write_settings(&changed).expect("запись");

    let written = settings_json(&t);
    assert_eq!(written["theme"], "sepia", "непонятное значение не затёрто");
    assert_eq!(written["font_size"], 14);
    assert_eq!(written["graph"], json!({ "physics": true }));
    assert_eq!(written["format_version"], 1);

    let mut themed = changed;
    themed.theme = Theme::Light;
    t.vault.write_settings(&themed).expect("смена темы");
    assert_eq!(
        settings_json(&t)["theme"],
        "light",
        "явно выбранная тема пишется"
    );
}

#[test]
fn broken_settings_file_reads_as_defaults() {
    let t = TestVault::new();
    for content in [
        "не JSON",
        "[1, 2]",
        "null",
        r#"{"format_version": -3, "theme": 5}"#,
    ] {
        std::fs::write(t.root().join(".rhizomind/settings.json"), content).expect("файл");
        assert_eq!(
            t.vault.read_settings(),
            Ok(Settings::default()),
            "{content}"
        );
    }
}

#[test]
fn workspace_round_trip_and_missing_file() {
    let t = TestVault::new();
    assert_eq!(t.vault.read_workspace(), Ok(None));

    let mut map = serde_json::Map::new();
    map.insert("version".into(), json!(1));
    map.insert("open".into(), json!(["Встреча.md"]));
    let workspace = Workspace(map);
    t.vault.write_workspace(&workspace).expect("запись");

    assert_eq!(t.vault.read_workspace(), Ok(Some(workspace)));
    assert!(t.root().join(".rhizomind/workspace.json").is_file());

    std::fs::write(t.root().join(".rhizomind/workspace.json"), "[]").expect("не объект");
    assert_eq!(t.vault.read_workspace(), Ok(None));
}

#[test]
fn service_file_replaced_by_a_link_is_not_followed() {
    let t = TestVault::new();
    symlink_file(
        &t.outside().join("secret.md"),
        &t.root().join(".rhizomind/workspace.json"),
    );
    assert!(matches!(
        code(t.vault.read_workspace()),
        ErrorCode::OutsideVault
    ));
    let workspace = Workspace(serde_json::Map::new());
    assert_eq!(
        code(t.vault.write_workspace(&workspace)),
        ErrorCode::OutsideVault
    );
    assert_eq!(
        std::fs::read(t.outside().join("secret.md")).expect("секрет"),
        "секрет\n".as_bytes()
    );
}

#[test]
fn open_rejects_service_folder_that_is_a_link() {
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("vault");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&root).expect("корень");
    std::fs::create_dir_all(&elsewhere).expect("снаружи");
    symlink_dir(&elsewhere, &root.join(".rhizomind"));
    assert_eq!(code(Vault::open(&root)), ErrorCode::OutsideVault);
}

#[test]
fn open_creates_service_folders_and_fails_for_missing_root() {
    let dir = tempfile::tempdir().expect("папка");
    let vault = Vault::open(dir.path()).expect("открыто");
    assert!(dir.path().join(".rhizomind/tmp").is_dir());
    assert_eq!(vault.tree(), Ok(Vec::new()), "служебная папка не в дереве");
    assert_eq!(
        code(Vault::open(&dir.path().join("нет"))),
        ErrorCode::VaultUnavailable
    );
}

#[test]
fn create_makes_a_new_vault_folder_only_once() {
    let dir = tempfile::tempdir().expect("папка");
    let root = Vault::create(dir.path(), "Мои заметки").expect("создано");
    assert!(root.is_dir());
    assert_eq!(
        root.file_name().and_then(|n| n.to_str()),
        Some("Мои заметки")
    );
    assert_eq!(
        code(Vault::create(dir.path(), "Мои заметки")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(Vault::create(dir.path(), "..")),
        ErrorCode::InvalidPath
    );
    assert_eq!(
        code(Vault::create(dir.path(), "a/b")),
        ErrorCode::InvalidPath
    );
    let opened = Vault::open(&root).expect("открывается");
    assert!(root.join(".rhizomind").is_dir());
    drop(opened);
}
