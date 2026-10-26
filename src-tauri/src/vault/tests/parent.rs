//! Родительская папка нового хранилища (`Vault::check_parent`, задача 2.8б).

use super::super::Vault;
use crate::commands::ErrorCode;

fn code(result: crate::commands::CoreResult<std::path::PathBuf>) -> ErrorCode {
    result.expect_err("ожидалась ошибка").code
}

#[test]
fn ordinary_folder_is_accepted_and_canonical() {
    let dir = tempfile::tempdir().expect("папка");
    let parent = dir.path().join("Заметки");
    std::fs::create_dir(&parent).expect("папка");
    let checked = Vault::check_parent(&parent.join("..").join("Заметки"), &[]).expect("подходит");
    assert_eq!(checked, dunce::canonicalize(&parent).expect("путь"));
}

#[test]
fn missing_file_or_relative_parent_is_rejected() {
    let dir = tempfile::tempdir().expect("папка");
    let file = dir.path().join("файл.txt");
    std::fs::write(&file, "x").expect("файл");
    assert_eq!(
        code(Vault::check_parent(&dir.path().join("нет"), &[])),
        ErrorCode::VaultUnavailable
    );
    assert_eq!(
        code(Vault::check_parent(&file, &[])),
        ErrorCode::VaultUnavailable
    );
    assert_eq!(
        code(Vault::check_parent(
            std::path::Path::new("относительный"),
            &[]
        )),
        ErrorCode::InvalidPath
    );
}

#[test]
fn folder_inside_a_vault_or_its_service_folder_is_rejected() {
    let dir = tempfile::tempdir().expect("папка");
    let vault = dir.path().join("Хранилище");
    let inner = vault.join("Проекты");
    std::fs::create_dir_all(&inner).expect("папки");
    Vault::open(&vault).expect("хранилище");

    for parent in [
        vault.clone(),
        inner.clone(),
        vault.join(".rhizomind"),
        vault.join(".rhizomind").join("tmp"),
    ] {
        assert_eq!(
            code(Vault::check_parent(&parent, &[])),
            ErrorCode::NestedVault,
            "{}",
            parent.display()
        );
    }
}

#[test]
fn folder_inside_a_known_vault_without_service_folder_is_rejected() {
    let dir = tempfile::tempdir().expect("папка");
    let known = dir.path().join("Известное");
    let inner = known.join("Внутри");
    std::fs::create_dir_all(&inner).expect("папки");
    assert_eq!(
        code(Vault::check_parent(&inner, std::slice::from_ref(&known))),
        ErrorCode::NestedVault
    );
    // Соседняя папка с похожим началом имени — не внутри.
    let sibling = dir.path().join("Известное 2");
    std::fs::create_dir(&sibling).expect("папка");
    assert!(Vault::check_parent(&sibling, &[known]).is_ok());
}
