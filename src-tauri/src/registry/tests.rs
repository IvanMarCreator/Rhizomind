//! Список хранилищ: путь, формат, атомарная запись, испорченный файл.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().expect("временная папка")
}

fn folder(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("папка");
    path
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("файл")).expect("JSON")
}

// ---- Путь ------------------------------------------------------------------

/// Решение 46: `dirs::data_local_dir()/Rhizomind/vaults.json` — на Windows
/// `%LOCALAPPDATA%`, не перемещаемый профиль `%APPDATA%\Roaming`.
#[test]
fn profile_file_is_in_local_app_data_not_roaming() {
    let path = profile_file().expect("папка профиля");
    let local = dirs::data_local_dir().expect("data_local_dir");
    assert_eq!(path, local.join("Rhizomind").join("vaults.json"));
    assert!(
        !path
            .components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("Roaming")),
        "{}",
        path.display()
    );
    // На macOS config_dir и data_local_dir — одна папка (Application Support).
    if !cfg!(target_os = "macos") {
        let config = dirs::config_dir().expect("config_dir");
        assert!(!path.starts_with(&config), "{}", path.display());
    }
    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA");
        assert!(path.starts_with(local_app_data), "{}", path.display());
        let roaming = std::env::var_os("APPDATA").expect("APPDATA");
        assert!(!path.starts_with(roaming), "{}", path.display());
    }
}

/// Заглушка по умолчанию (тесты ядра) ничего не пишет в профиль ОС.
#[test]
fn default_registry_keeps_list_in_memory_only() {
    let dir = temp();
    let vault = folder(dir.path(), "Хранилище");
    let registry = Registry::default();
    let added = registry.add(&vault).expect("добавлено");
    assert_eq!(registry.find(&added.id).expect("есть"), added);
    assert!(registry.file().is_none());
}

// ---- Формат ----------------------------------------------------------------

/// В файле только версия формата, идентификаторы, имена и пути хранилищ,
/// последнее открытое и состояние окон — ни имён, ни содержимого заметок.
#[test]
fn file_holds_only_names_paths_and_window_state() {
    let dir = temp();
    let file = dir.path().join("Rhizomind").join("vaults.json");
    let vault = folder(dir.path(), "Работа");
    std::fs::write(vault.join("Секретная заметка.md"), "секрет").expect("заметка");
    let other = folder(dir.path(), "Дом");
    let registry = Registry::at(file.clone());

    let work = registry.add(&vault).expect("добавлено");
    registry.add(&other).expect("добавлено");
    registry.mark_opened(&work.id).expect("последнее");
    registry
        .save_window(
            &work.id,
            WindowState {
                width: 1000,
                height: 700,
                x: 10,
                y: -20,
                maximized: false,
            },
        )
        .expect("окно");

    let json = read_json(&file);
    let keys = |value: &Value| {
        let mut keys: Vec<String> = value.as_object().expect("объект").keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&json), ["format_version", "last_opened", "vaults"]);
    assert_eq!(json["format_version"], json!(FORMAT_VERSION));
    assert_eq!(json["last_opened"], json!(work.id.0));
    let vaults = json["vaults"].as_array().expect("список");
    assert_eq!(vaults.len(), 2);
    assert_eq!(keys(&vaults[0]), ["id", "name", "path", "window"]);
    assert_eq!(keys(&vaults[1]), ["id", "name", "path"]);
    assert_eq!(
        keys(&vaults[0]["window"]),
        ["height", "maximized", "width", "x", "y"]
    );
    assert_eq!(vaults[0]["name"], json!("Работа"));
    assert_eq!(
        vaults[0]["path"],
        json!(dunce::canonicalize(&vault).unwrap().to_str().unwrap())
    );
    let text = std::fs::read_to_string(&file).expect("текст");
    assert!(!text.contains("Секретная"), "{text}");
    assert!(!text.contains("секрет"), "{text}");
}

#[test]
fn list_survives_restart_and_keeps_order() {
    let dir = temp();
    let file = dir.path().join("vaults.json");
    let (a, b) = (folder(dir.path(), "А"), folder(dir.path(), "Б"));
    let first = Registry::at(file.clone());
    let added_a = first.add(&a).expect("А");
    let added_b = first.add(&b).expect("Б");
    first.mark_opened(&added_b.id).expect("последнее");
    let state = WindowState {
        width: 900,
        height: 600,
        x: 5,
        y: 6,
        maximized: true,
    };
    first.save_window(&added_b.id, state).expect("окно");

    let second = Registry::at(file);
    let entries = second.entries().expect("список");
    assert_eq!(entries.vaults, vec![added_a, added_b.clone()]);
    assert_eq!(entries.last_opened, Some(added_b.id.clone()));
    assert_eq!(second.last_opened(), Some(added_b.clone()));
    assert_eq!(second.window(&added_b.id), Some(state));
}

#[test]
fn same_folder_is_added_once() {
    let dir = temp();
    let vault = folder(dir.path(), "Хранилище");
    let registry = Registry::at(dir.path().join("vaults.json"));
    let first = registry.add(&vault).expect("добавлено");
    let again = registry
        .add(&vault.join("..").join("Хранилище"))
        .expect("то же");
    assert_eq!(first, again);
    assert_eq!(registry.entries().expect("список").vaults.len(), 1);
}

#[test]
fn ids_are_unique_and_unguessable() {
    let dir = temp();
    let registry = Registry::default();
    let ids: std::collections::BTreeSet<String> = (0..20)
        .map(|i| {
            registry
                .add(&folder(dir.path(), &format!("Х{i}")))
                .expect("добавлено")
                .id
                .0
        })
        .collect();
    assert_eq!(ids.len(), 20);
    for id in &ids {
        assert_eq!(id.len(), 16, "{id}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
    }
}

#[test]
fn missing_folder_or_file_is_not_added() {
    let dir = temp();
    let registry = Registry::default();
    let error = registry
        .add(&dir.path().join("нет"))
        .expect_err("нет папки");
    assert_eq!(error.code, ErrorCode::VaultUnavailable);
    let file = dir.path().join("файл.txt");
    std::fs::write(&file, "").expect("файл");
    let error = registry.add(&file).expect_err("не папка");
    assert_eq!(error.code, ErrorCode::VaultUnavailable);
}

#[test]
fn remove_forgets_vault_but_keeps_files() {
    let dir = temp();
    let file = dir.path().join("vaults.json");
    let vault = folder(dir.path(), "Хранилище");
    std::fs::write(vault.join("Заметка.md"), "текст").expect("заметка");
    let registry = Registry::at(file.clone());
    let added = registry.add(&vault).expect("добавлено");
    registry.mark_opened(&added.id).expect("последнее");

    registry.remove(&added.id).expect("убрано");

    assert_eq!(
        registry.find(&added.id).expect_err("нет").code,
        ErrorCode::VaultNotFound
    );
    let entries = Registry::at(file).entries().expect("список");
    assert!(entries.vaults.is_empty());
    assert_eq!(entries.last_opened, None);
    assert!(vault.join("Заметка.md").is_file());
    assert_eq!(
        registry.remove(&added.id).expect_err("уже нет").code,
        ErrorCode::VaultNotFound
    );
}

#[test]
fn unknown_vault_cannot_be_marked_or_sized() {
    let registry = Registry::default();
    let id = VaultId("0000000000000000".into());
    assert_eq!(
        registry.mark_opened(&id).expect_err("нет").code,
        ErrorCode::VaultNotFound
    );
    let state = WindowState {
        width: 800,
        height: 600,
        x: 0,
        y: 0,
        maximized: false,
    };
    assert_eq!(
        registry.save_window(&id, state).expect_err("нет").code,
        ErrorCode::VaultNotFound
    );
}

// ---- Запись и повреждение ---------------------------------------------------

/// Запись атомарная: файл заменяется целиком через временный файл в той же
/// папке; временных файлов после записи не остаётся.
#[test]
fn write_replaces_file_and_leaves_no_temp_files() {
    let dir = temp();
    let profile = dir.path().join("Rhizomind");
    let file = profile.join("vaults.json");
    let registry = Registry::at(file.clone());
    for i in 0..5 {
        registry
            .add(&folder(dir.path(), &format!("Х{i}")))
            .expect("добавлено");
    }
    let names: Vec<String> = std::fs::read_dir(&profile)
        .expect("папка")
        .map(|e| {
            e.expect("элемент")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["vaults.json"]);
    assert_eq!(read_json(&file)["vaults"].as_array().unwrap().len(), 5);
}

/// Испорченный файл не роняет приложение и не теряется: он копируется рядом
/// (`vaults.broken-<время>.json`), список начинается пустым.
#[test]
fn corrupt_file_is_copied_aside_and_list_starts_empty() {
    for broken in [
        "{ не JSON".to_owned(),
        json!({ "format_version": 99, "vaults": [], "last_opened": null }).to_string(),
        json!({ "format_version": 1, "vaults": [], "last_opened": null, "notes": ["x"] })
            .to_string(),
        json!({ "format_version": 1, "vaults": [{ "id": "", "name": "a", "path": "b" }], "last_opened": null })
            .to_string(),
        json!({ "format_version": 1, "vaults": [
            { "id": "1", "name": "a", "path": "b" },
            { "id": "1", "name": "c", "path": "d" }
        ], "last_opened": null })
        .to_string(),
    ] {
        let dir = temp();
        let file = dir.path().join("vaults.json");
        std::fs::write(&file, &broken).expect("файл");
        let registry = Registry::at(file.clone());

        let entries = registry.entries().expect("пустой список, а не ошибка");
        assert!(entries.vaults.is_empty(), "{broken}");

        let copies: Vec<PathBuf> = std::fs::read_dir(dir.path())
            .expect("папка")
            .map(|e| e.expect("элемент").path())
            .filter(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                name.starts_with("vaults.broken-") && name.ends_with(".json")
            })
            .collect();
        assert_eq!(copies.len(), 1, "{broken}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).expect("копия"),
            broken,
            "копия побайтно"
        );

        // Новый список пишется поверх испорченного — копия остаётся.
        registry
            .add(&folder(dir.path(), "Новое"))
            .expect("добавлено");
        assert_eq!(read_json(&file)["vaults"].as_array().unwrap().len(), 1);
        assert!(copies[0].is_file());
    }
}

/// Последнее открытое, которого нет в списке, — не повреждение: забывается.
#[test]
fn dangling_last_opened_is_ignored() {
    let dir = temp();
    let file = dir.path().join("vaults.json");
    std::fs::write(
        &file,
        json!({ "format_version": 1, "vaults": [{ "id": "a1", "name": "А", "path": "/нет" }], "last_opened": "b2" })
            .to_string(),
    )
    .expect("файл");
    let registry = Registry::at(file);
    let entries = registry.entries().expect("список");
    assert_eq!(entries.vaults.len(), 1);
    assert_eq!(entries.last_opened, None);
}

/// Испорченный файл, который не удалось скопировать, не перезаписывается:
/// список работает в памяти.
#[test]
fn corrupt_file_that_cannot_be_copied_is_not_overwritten() {
    let dir = temp();
    let file = dir.path().join("vaults.json");
    std::fs::write(&file, "{ не JSON").expect("файл");
    let registry = Registry::at(file.clone());
    registry.fail_backup_for_tests();

    registry
        .add(&folder(dir.path(), "Новое"))
        .expect("в памяти");
    assert_eq!(std::fs::read_to_string(&file).expect("файл"), "{ не JSON");
    assert_eq!(registry.entries().expect("список").vaults.len(), 1);
}
