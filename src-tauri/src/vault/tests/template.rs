use super::*;
use crate::commands::Settings;

fn setup() -> TestVault {
    let t = TestVault::new();
    t.put("Шаблоны/План.md", b"template");
    t.vault
        .write_settings(&Settings {
            template_folder: Some(vp("Шаблоны")),
            ..Default::default()
        })
        .expect("настройки");
    t
}

#[test]
fn template_missing_or_hidden_source_creates_nothing() {
    let t = setup();
    t.put("Шаблоны/.tmp/План.md", b"hidden");
    t.put("Шаблоны/Вложение.txt", b"attachment");
    t.put("Шаблоны-архив/План.md", b"other");
    for source in [
        "Шаблоны/Удалён.md",
        "Шаблоны/.tmp/План.md",
        "Шаблоны/Вложение.txt",
        "Шаблоны-архив/План.md",
        ".rhizomind/settings.json",
    ] {
        assert!(
            t.vault
                .create_from_template(&vp(source), &vp(""), "Копия.md")
                .is_err(),
            "{source}"
        );
        assert!(!t.file("Копия.md").exists());
    }
}

#[test]
fn template_name_taken_during_copy_is_preserved() {
    let t = setup();
    let existing = t.file("Копия.md");
    t.vault
        .own_writes
        .record(&vp("Копия.md"), &blake3::hash(b"prior").to_hex(), 5);
    crate::vault::atomic::after_temp_written(Box::new(move || {
        std::fs::write(existing, b"template").unwrap()
    }));
    assert_eq!(
        code(
            t.vault
                .create_from_template(&vp("Шаблоны/План.md"), &vp(""), "Копия.md")
        ),
        ErrorCode::AlreadyExists
    );
    assert_eq!(t.bytes("Копия.md"), b"template");
    assert!(
        !t.vault
            .own_writes
            .is_own(&vp("Копия.md"), &t.file("Копия.md"))
    );
    t.put("Копия.md", b"prior");
    assert!(
        t.vault
            .own_writes
            .is_own(&vp("Копия.md"), &t.file("Копия.md"))
    );
    assert_eq!(t.bytes("Шаблоны/План.md"), b"template");
    assert_eq!(
        std::fs::read_dir(t.file(".rhizomind/tmp")).unwrap().count(),
        0
    );
}

#[test]
fn template_user_renames_destination_during_preparation() {
    let t = setup();
    let dir = t.file("Проект");
    std::fs::create_dir(&dir).unwrap();
    let renamed = t.file("Архив");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        let result = std::fs::rename(dir, renamed);
        #[cfg(windows)]
        assert!(result.is_err());
        #[cfg(unix)]
        result.unwrap();
    }));
    let result = t
        .vault
        .create_from_template(&vp("Шаблоны/План.md"), &vp("Проект"), "Копия.md");
    #[cfg(windows)]
    {
        result.unwrap();
        assert_eq!(t.bytes("Проект/Копия.md"), b"template");
    }
    #[cfg(unix)]
    assert!(result.is_err());
    assert!(!t.file("Архив/Копия.md").exists());
    assert_eq!(t.bytes("Шаблоны/План.md"), b"template");
}

#[test]
fn template_user_renames_the_selected_source() {
    let t = setup();
    let source = t.file("Шаблоны/План.md");
    let renamed = t.file("Шаблоны/Архив.md");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        let result = std::fs::rename(source, renamed);
        #[cfg(windows)]
        assert!(result.is_err());
        #[cfg(unix)]
        result.unwrap();
    }));
    let result = t
        .vault
        .create_from_template(&vp("Шаблоны/План.md"), &vp(""), "Копия.md");
    #[cfg(windows)]
    {
        result.unwrap();
        assert_eq!(t.bytes("Копия.md"), b"template");
    }
    #[cfg(unix)]
    {
        assert!(result.is_err());
        assert_eq!(t.bytes("Шаблоны/Архив.md"), b"template");
        assert!(!t.file("Копия.md").exists());
    }
}

#[test]
fn template_unconfigured_and_explicit_vault_root_are_distinct() {
    let t = TestVault::new();
    t.put("План.md", b"template");
    assert!(
        t.vault
            .create_from_template(&vp("План.md"), &vp(""), "Копия.md")
            .is_err()
    );
    t.vault
        .write_settings(&Settings {
            template_folder: Some(vp("")),
            ..Default::default()
        })
        .unwrap();
    t.vault
        .create_from_template(&vp("План.md"), &vp(""), "Копия.md")
        .unwrap();
    assert_eq!(t.bytes("Копия.md"), b"template");
}

// Проверка Linux использует права обычного пользователя, как реальное окно.
#[cfg(unix)]
#[test]
fn template_readonly_destination_keeps_source_and_existing_files() {
    use std::os::unix::fs::PermissionsExt;
    let t = setup();
    let existing = t.put("Чтение/Существующая.md", b"existing");
    let dir = existing.parent().unwrap();
    let old = std::fs::metadata(dir).unwrap().permissions();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = t
        .vault
        .create_from_template(&vp("Шаблоны/План.md"), &vp("Чтение"), "Копия.md");
    std::fs::set_permissions(dir, old).unwrap();
    // root сохраняет право записи даже при 0555.
    if unsafe { libc::geteuid() } != 0 {
        assert!(result.is_err());
        assert!(!t.file("Чтение/Копия.md").exists());
    }
    assert_eq!(t.bytes("Чтение/Существующая.md"), b"existing");
    assert_eq!(t.bytes("Шаблоны/План.md"), b"template");
}

#[test]
fn template_linked_source_and_destination_are_rejected() {
    let t = setup();
    for name in ["Шаблоны/Другая", "Назначение"] {
        #[cfg(windows)]
        junction::create(t.outside(), t.file(name)).expect("точка соединения");
        #[cfg(unix)]
        symlink_dir(&t.outside(), &t.file(name));
    }
    assert!(
        t.vault
            .create_from_template(&vp("Шаблоны/Другая/secret.md"), &vp(""), "Копия.md")
            .is_err()
    );
    assert!(
        t.vault
            .create_from_template(&vp("Шаблоны/План.md"), &vp("Назначение"), "Копия.md")
            .is_err()
    );
    assert!(!t.file("Копия.md").exists());
    assert!(!t.outside().join("Копия.md").exists());
}
