//! Доработки по ревью кода и безопасности задачи 1.2
//! (`docs/reviews/2026-10-25-task-1.2.md`).

use super::*;
use crate::commands::{NoteWriteResponse, ReadOnlyReason, Settings, Workspace};
use crate::vault::TrashFailure;
use crate::vault::atomic;

/// Имена в папке «снаружи».
fn outside_names(t: &TestVault) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(t.outside())
        .expect("снаружи")
        .map(|e| {
            e.expect("элемент")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Заменяет папку ссылкой на папку снаружи (как злоумышленник после
/// открытия хранилища).
fn replace_dir_with_link(dir: &Path, target: &Path) {
    std::fs::remove_dir_all(dir).expect("удалить папку");
    symlink_dir(target, dir);
}

// П. 1 — временная и служебная папки подменены ссылками после открытия.

#[test]
fn tmp_folder_replaced_by_link_after_open_is_not_used() {
    let t = TestVault::new();
    t.put("Заметка.md", "старое\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    let trap = t.outside().join("ловушка");
    std::fs::create_dir(&trap).expect("ловушка");
    replace_dir_with_link(&t.root().join(".rhizomind/tmp"), &trap);

    let result = t
        .vault
        .write_note(&vp("Заметка.md"), "секретный текст\n", &note.version);

    assert_eq!(code(result), ErrorCode::OutsideVault);
    assert_eq!(
        std::fs::read_dir(&trap).expect("ловушка").count(),
        0,
        "снаружи пусто"
    );
    assert_eq!(outside_names(&t), ["secret.md", "ловушка"]);
    assert_eq!(t.bytes("Заметка.md"), "старое\n".as_bytes());
    assert_eq!(
        code(t.vault.write_settings(&Settings::default())),
        ErrorCode::OutsideVault
    );
    assert_eq!(
        std::fs::read_dir(&trap).expect("ловушка").count(),
        0,
        "снаружи пусто"
    );
}

#[test]
fn service_folder_replaced_by_link_after_open_is_not_used() {
    let t = TestVault::new();
    let trap = t.outside().join("ловушка");
    std::fs::create_dir_all(trap.join("tmp")).expect("ловушка");
    replace_dir_with_link(&t.root().join(".rhizomind"), &trap);

    assert_eq!(
        code(t.vault.write_settings(&Settings::default())),
        ErrorCode::OutsideVault
    );
    assert_eq!(
        code(t.vault.write_workspace(&Workspace(serde_json::Map::new()))),
        ErrorCode::OutsideVault
    );
    assert_eq!(code(t.vault.read_settings()), ErrorCode::OutsideVault);
    let mut left: Vec<_> = std::fs::read_dir(&trap)
        .expect("ловушка")
        .map(|e| e.expect("элемент").file_name())
        .collect();
    left.sort();
    assert_eq!(left, ["tmp"], "снаружи ничего не появилось");
    assert_eq!(std::fs::read_dir(trap.join("tmp")).expect("tmp").count(), 0);
}

// П. 2 — длинные пути (больше 260 символов на Windows).

#[test]
fn long_paths_over_300_characters_work() {
    let t = TestVault::new();
    let segment = "Очень длинная папка с подробным названием проекта";
    let mut dir = VaultPath::root();
    for _ in 0..7 {
        dir = t.vault.create_folder(&dir, segment).expect("папка");
    }
    let note = t.vault.create_note(&dir, "Заметка.md").expect("заметка");
    let absolute = note.components().iter().fold(t.root(), |p, c| p.join(c));
    assert!(absolute.as_os_str().len() > 300, "путь длиннее 300");

    let opened = t.vault.read_note(&note).expect("чтение");
    let saved = t
        .vault
        .write_note(&note, "длинный путь\n", &opened.version)
        .expect("запись");
    assert!(matches!(saved, NoteWriteResponse::Saved { .. }));
    assert_eq!(
        std::fs::read(&absolute).expect("на диске"),
        "длинный путь\n".as_bytes()
    );
    let renamed = t.vault.rename(&note, "Другая.md").expect("переименование");
    assert_eq!(
        t.vault.read_note(&renamed).expect("чтение").text,
        "длинный путь\n"
    );
}

// П. 4 — сбой ReplaceFileW (1176, 1177) не теряет данные.

#[test]
fn replace_failure_1176_with_target_intact_still_saves() {
    let t = TestVault::new();
    t.put("Заметка.md", "старое\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    atomic::simulate_replace_failure(atomic::ReplaceFailure::Code1176 {
        target_removed: false,
    });

    let saved = t
        .vault
        .write_note(&vp("Заметка.md"), "новое\n", &note.version);

    assert!(
        matches!(saved, Ok(NoteWriteResponse::Saved { .. })),
        "{saved:?}"
    );
    assert_eq!(t.bytes("Заметка.md"), "новое\n".as_bytes());
    assert_eq!(
        std::fs::read_dir(t.root().join(".rhizomind/tmp"))
            .expect("tmp")
            .count(),
        0
    );
}

#[test]
fn replace_failure_1176_with_target_removed_moves_the_new_text_in() {
    let t = TestVault::new();
    t.put("Заметка.md", "старое\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    atomic::simulate_replace_failure(atomic::ReplaceFailure::Code1176 {
        target_removed: true,
    });

    let saved = t
        .vault
        .write_note(&vp("Заметка.md"), "новое\n", &note.version);

    assert!(
        matches!(saved, Ok(NoteWriteResponse::Saved { .. })),
        "{saved:?}"
    );
    assert_eq!(t.bytes("Заметка.md"), "новое\n".as_bytes());
}

#[test]
fn replace_failure_1177_recovers_from_the_backup_name() {
    let t = TestVault::new();
    t.put("Заметка.md", "старое\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    atomic::simulate_replace_failure(atomic::ReplaceFailure::Code1177);

    let saved = t
        .vault
        .write_note(&vp("Заметка.md"), "новое\n", &note.version);

    assert!(
        matches!(saved, Ok(NoteWriteResponse::Saved { .. })),
        "{saved:?}"
    );
    assert_eq!(t.bytes("Заметка.md"), "новое\n".as_bytes());
    assert_eq!(
        std::fs::read_dir(t.root().join(".rhizomind/tmp"))
            .expect("tmp")
            .count(),
        0,
        "резервная копия убрана"
    );
}

#[test]
fn unsaved_note_text_left_in_tmp_is_recovered_next_to_the_vault_root() {
    let t = TestVault::new();
    let tmp = t.root().join(".rhizomind/tmp");
    std::fs::write(tmp.join(".note-abc123"), "несохранённый текст\n").expect("остаток");
    std::fs::write(tmp.join(".svc-def456"), "{}").expect("служебный остаток");

    let reopened = Vault::open(&t.root()).expect("повторное открытие");

    assert_eq!(std::fs::read_dir(&tmp).expect("tmp").count(), 0);
    let recovered: Vec<_> = reopened
        .tree()
        .expect("дерево")
        .into_iter()
        .filter_map(|e| e.path.file_name().map(str::to_owned))
        .filter(|n| n.starts_with("Восстановлено "))
        .collect();
    assert_eq!(recovered.len(), 1, "{recovered:?}");
    assert!(recovered[0].ends_with(".md"));
    assert_eq!(t.bytes(&recovered[0]), "несохранённый текст\n".as_bytes());
}

// П. 6 — внешняя правка между подготовкой и заменой.

#[test]
fn external_change_while_the_temp_file_is_written_is_kept() {
    let t = TestVault::new();
    let path = t.put("Заметка.md", "старое\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    atomic::after_temp_written(Box::new(move || {
        std::fs::write(&path, "внешняя правка\n").expect("внешняя правка");
    }));

    let response = t
        .vault
        .write_note(&vp("Заметка.md"), "из редактора\n", &note.version)
        .expect("запись");

    let NoteWriteResponse::ConflictSaved { copy_path, .. } = response else {
        panic!("ожидался конфликт: {response:?}");
    };
    assert_eq!(t.bytes("Заметка.md"), "из редактора\n".as_bytes());
    assert_eq!(
        t.bytes(&copy_path.components().join("/")),
        "внешняя правка\n".as_bytes()
    );
}

// П. 7 — корзина отказала на томе с корзиной.

#[test]
fn trash_failure_on_a_volume_with_trash_is_an_error_without_permanent_delete() {
    let t = TestVault::new();
    t.put("Занята.md", "текст");
    t.vault
        .set_trash_backend(Box::new(|_: &Path| Err(TrashFailure::Failed)));
    let asked = std::cell::Cell::new(false);

    let result = t.vault.trash(&vp("Занята.md"), &|_| {
        asked.set(true);
        true
    });

    assert_eq!(code(result), ErrorCode::IoFailed);
    assert!(!asked.get(), "безвозвратное удаление не предлагается");
    assert_eq!(t.bytes("Занята.md"), "текст".as_bytes());
}

// П. 8 — имена вида короткого имени, которые так и называются.

#[test]
fn real_file_named_like_a_short_name_is_visible_and_readable() {
    let t = TestVault::new();
    t.put("Итог~1.md", "итог\n");
    t.put("ABC~2/x.md", "x\n");
    assert_eq!(
        t.vault.read_note(&vp("Итог~1.md")).expect("чтение").text,
        "итог\n"
    );
    assert_eq!(
        t.vault.read_note(&vp("ABC~2/x.md")).expect("чтение").text,
        "x\n"
    );
    let names: Vec<String> = t
        .vault
        .tree()
        .expect("дерево")
        .into_iter()
        .map(|e| e.path.components().join("/"))
        .collect();
    assert!(names.contains(&"Итог~1.md".to_owned()), "{names:?}");
    assert!(names.contains(&"ABC~2/x.md".to_owned()), "{names:?}");
}

/// На Windows короткое имя существующей папки — псевдоним: отклоняется.
#[cfg(windows)]
#[test]
fn short_name_alias_of_an_existing_folder_is_rejected() {
    let t = TestVault::new();
    t.put("Длинное имя папки проекта/Заметка.md", "текст\n");
    let alias = "ДЛИННО~1";
    if !t.root().join(alias).exists() {
        return; // на томе выключены короткие имена — псевдонимов нет
    }
    assert_eq!(
        code(t.vault.read_note(&vp(&format!("{alias}/Заметка.md")))),
        ErrorCode::InvalidPath
    );
}

// П. 9 — смена только регистра: прерванный двухшаговый вариант
// восстанавливается при открытии.

#[test]
fn interrupted_case_only_rename_is_finished_on_open() {
    let t = TestVault::new();
    t.put("Папка/план.md", "текст\n");
    crate::vault::fsops::crash_after_first_rename_step();

    let result = t.vault.rename(&vp("Папка/план.md"), "План.md");

    assert_eq!(code(result), ErrorCode::IoFailed);
    let _reopened = Vault::open(&t.root()).expect("повторное открытие");
    let names: Vec<String> = std::fs::read_dir(t.file("Папка"))
        .expect("папка")
        .map(|e| {
            e.expect("элемент")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["План.md"]);
    assert_eq!(t.bytes("Папка/План.md"), "текст\n".as_bytes());
    assert!(!t.root().join(".rhizomind/rename.json").exists());
}

// П. 10 — файл больше 100 МБ не читается целиком.

#[test]
fn note_larger_than_hard_limit_opens_read_only_with_empty_text() {
    let t = TestVault::new();
    let path = t.file("Огромная.md");
    let file = std::fs::File::create(&path).expect("файл");
    file.set_len(100 * 1024 * 1024 + 1).expect("размер");
    drop(file);

    let note = t.vault.read_note(&vp("Огромная.md")).expect("чтение");

    assert_eq!(note.text, "");
    assert_eq!(
        note.read_only,
        Some(ReadOnlyReason::TooLarge {
            limit_bytes: Settings::default().max_edit_size_bytes
        })
    );
    assert_eq!(note.version.size, 100 * 1024 * 1024 + 1);
}

// П. 11 — испорченный settings.json не затирается молча.

#[test]
fn broken_settings_file_is_backed_up_before_writing_defaults() {
    let t = TestVault::new();
    let settings = t.root().join(".rhizomind/settings.json");
    std::fs::write(&settings, "{ испорчено").expect("файл");

    t.vault
        .write_settings(&Settings::default())
        .expect("запись");

    assert_eq!(
        std::fs::read(t.root().join(".rhizomind/settings.json.bak")).expect("копия"),
        "{ испорчено".as_bytes()
    );
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).expect("новый")).expect("JSON");
    assert_eq!(written["format_version"], 1);
}

#[test]
fn second_broken_settings_backup_does_not_overwrite_the_first() {
    let t = TestVault::new();
    let settings = t.root().join(".rhizomind/settings.json");
    std::fs::write(&settings, "первый").expect("файл");
    t.vault
        .write_settings(&Settings::default())
        .expect("запись");
    std::fs::write(&settings, "второй").expect("файл");
    t.vault
        .write_settings(&Settings::default())
        .expect("запись");

    assert_eq!(
        std::fs::read(t.root().join(".rhizomind/settings.json.bak")).expect("копия"),
        b"\xd0\xbf\xd0\xb5\xd1\x80\xd0\xb2\xd1\x8b\xd0\xb9"
    );
    assert_eq!(
        std::fs::read(t.root().join(".rhizomind/settings.json.bak.2")).expect("вторая копия"),
        "второй".as_bytes()
    );
}
