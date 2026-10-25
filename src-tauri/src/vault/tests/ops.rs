//! Дерево и операции с ним: список, заметки с датой создания, создание,
//! проверка имени, переименование (в том числе только регистр),
//! перемещение, корзина.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::read::generated;
use super::*;
use crate::commands::{EntryKind, NameCheck, NameProblem, TrashOutcome};
use crate::vault::TrashFailure;

fn entries(t: &TestVault) -> HashSet<(String, EntryKind)> {
    t.vault
        .tree()
        .expect("дерево")
        .into_iter()
        .map(|e| (e.path.components().join("/"), e.kind))
        .collect()
}

fn set(items: &[(&str, EntryKind)]) -> HashSet<(String, EntryKind)> {
    items.iter().map(|(p, k)| ((*p).to_owned(), *k)).collect()
}

/// Имена в папке хранилища как на диске (без служебной папки).
fn disk_names(t: &TestVault, rel: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(t.file(rel))
        .expect("папка")
        .map(|e| {
            e.expect("элемент")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n != ".rhizomind")
        .collect();
    names.sort();
    names
}

#[test]
fn tree_lists_folders_notes_and_attachments_but_not_hidden_or_links() {
    let t = TestVault::new();
    t.put("Встреча.md", "");
    t.put("Проекты/Альфа/План.md", "");
    t.put("Проекты/схема.png", [1]);
    t.put(".скрытая.md", "");
    t.put(".скрытая папка/Внутри.md", "");
    t.put("Проекты/.git/config", "");
    std::fs::create_dir(t.file("Пустая")).expect("папка");
    symlink_dir(&t.outside(), &t.file("ссылка"));
    symlink_file(&t.outside().join("secret.md"), &t.file("секрет.md"));

    assert_eq!(
        entries(&t),
        set(&[
            ("Встреча.md", EntryKind::Note),
            ("Проекты", EntryKind::Folder),
            ("Проекты/Альфа", EntryKind::Folder),
            ("Проекты/Альфа/План.md", EntryKind::Note),
            ("Проекты/схема.png", EntryKind::Attachment),
            ("Пустая", EntryKind::Folder),
        ])
    );
}

#[test]
fn tree_and_notes_of_a_generated_vault_match_its_manifest() {
    let (t, manifest) = generated(None);
    let notes = t.vault.notes().expect("заметки");
    assert_eq!(notes.len(), manifest.notes);
    let tree_notes = entries(&t)
        .into_iter()
        .filter(|(_, kind)| *kind == EntryKind::Note)
        .count();
    assert_eq!(tree_notes, manifest.notes);
    assert!(notes.iter().all(|n| n.created_ms > 0));
    assert!(
        notes
            .iter()
            .all(|n| !n.path.components().iter().any(|c| c.starts_with('.')))
    );
}

#[test]
fn notes_report_creation_time_from_the_file_system() {
    let t = TestVault::new();
    let path = t.put("Папка/Встреча.md", "");
    let notes = t.vault.notes().expect("заметки");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].path, vp("Папка/Встреча.md"));
    let meta = std::fs::metadata(&path).expect("мета");
    let expected = meta.created().or_else(|_| meta.modified()).expect("дата");
    let expected = expected
        .duration_since(std::time::UNIX_EPOCH)
        .expect("после 1970")
        .as_millis() as i64;
    assert_eq!(notes[0].created_ms, expected);
}

#[test]
fn create_note_makes_an_empty_lf_note_without_bom() {
    let t = TestVault::new();
    std::fs::create_dir(t.file("Папка")).expect("папка");

    let path = t
        .vault
        .create_note(&vp("Папка"), "Без названия.md")
        .expect("создана");

    assert_eq!(path, vp("Папка/Без названия.md"));
    assert_eq!(t.bytes("Папка/Без названия.md"), b"");
    let empty = blake3::hash(b"").to_hex().to_string();
    assert!(t.vault.is_own_write(&path, &empty, 0));
    let note = t.vault.read_note(&path).expect("читается");
    assert_eq!(note.text, "");
}

#[test]
fn create_rejects_duplicates_bad_names_and_missing_folders() {
    let t = TestVault::new();
    t.put("Заметка.md", "моя\n");
    std::fs::create_dir(t.file("Папка")).expect("папка");
    let v = &t.vault;

    assert_eq!(
        code(v.create_note(&vp(""), "заметка.MD")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(v.create_folder(&vp(""), "ПАПКА")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        code(v.create_folder(&vp(""), "заметка.md")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        v.create_note(&vp(""), "Итоги..md"),
        Err(CoreError::invalid_name(NameProblem::TrailingDotOrSpace))
    );
    assert_eq!(
        v.create_note(&vp(""), "Схема.png"),
        Err(CoreError::invalid_name(NameProblem::NoteExtension))
    );
    assert_eq!(
        v.create_folder(&vp(""), "a*b"),
        Err(CoreError::invalid_name(NameProblem::ForbiddenChars))
    );
    assert_eq!(
        code(v.create_note(&vp("Нет"), "Новая.md")),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(v.create_note(&vp("Заметка.md"), "Новая.md")),
        ErrorCode::NotFound
    );
    assert_eq!(t.bytes("Заметка.md"), b"\xd0\xbc\xd0\xbe\xd1\x8f\n");
}

#[test]
fn create_folder_makes_a_folder() {
    let t = TestVault::new();
    let path = t.vault.create_folder(&vp(""), "Проекты").expect("создана");
    assert_eq!(path, vp("Проекты"));
    assert!(t.file("Проекты").is_dir());
    let nested = t.vault.create_folder(&path, "Альфа").expect("вложенная");
    assert_eq!(nested, vp("Проекты/Альфа"));
}

#[test]
fn name_check_compares_by_name_key_and_ignores_the_renamed_entry() {
    let t = TestVault::new();
    t.put("План.md", "");
    t.put("Папка/Другое.md", "");
    let v = &t.vault;

    assert_eq!(
        v.check_new_name(&vp(""), "Новое.md", None),
        Ok(NameCheck::Ok { warning: None })
    );
    assert_eq!(
        v.check_new_name(&vp(""), "план.MD", None),
        Ok(NameCheck::rejected(NameProblem::Duplicate))
    );
    assert_eq!(
        v.check_new_name(&vp(""), "план.md", Some(&vp("План.md"))),
        Ok(NameCheck::Ok { warning: None }),
        "смена только регистра"
    );
    assert_eq!(
        v.check_new_name(&vp(""), "план.md", Some(&vp("Папка/Другое.md"))),
        Ok(NameCheck::rejected(NameProblem::Duplicate)),
        "элемент из другой папки не исключение"
    );
    assert_eq!(
        v.check_new_name(&vp(""), "CON", None),
        Ok(NameCheck::rejected(NameProblem::ReservedName))
    );
    assert_eq!(
        code(v.check_new_name(&vp("Нет"), "a.md", None)),
        ErrorCode::NotFound
    );
}

#[test]
fn rename_keeps_content_and_returns_the_new_path() {
    let t = TestVault::new();
    t.put("Папка/Старое.md", "текст\r\n");
    t.put("Папка/Вложенная/x.md", "x");

    let note = t
        .vault
        .rename(&vp("Папка/Старое.md"), "Новое.md")
        .expect("заметка");
    let folder = t.vault.rename(&vp("Папка"), "Архив").expect("папка");

    assert_eq!(note, vp("Папка/Новое.md"));
    assert_eq!(folder, vp("Архив"));
    assert_eq!(
        t.bytes("Архив/Новое.md"),
        b"\xd1\x82\xd0\xb5\xd0\xba\xd1\x81\xd1\x82\r\n"
    );
    assert_eq!(t.bytes("Архив/Вложенная/x.md"), b"x");
    assert_eq!(disk_names(&t, ""), ["Архив"]);
}

#[test]
fn rename_changing_only_case_works() {
    let t = TestVault::new();
    t.put("план.md", "текст\n");
    std::fs::create_dir(t.file("проекты")).expect("папка");

    let note = t.vault.rename(&vp("план.md"), "План.md").expect("заметка");
    let folder = t.vault.rename(&vp("проекты"), "Проекты").expect("папка");

    assert_eq!(note, vp("План.md"));
    assert_eq!(folder, vp("Проекты"));
    assert_eq!(disk_names(&t, ""), ["План.md", "Проекты"]);
    assert_eq!(t.bytes("План.md"), "текст\n".as_bytes());
}

#[test]
fn rename_never_overwrites_and_reports_missing() {
    let t = TestVault::new();
    t.put("Один.md", "1");
    t.put("Два.md", "2");
    assert_eq!(
        code(t.vault.rename(&vp("Один.md"), "два.md")),
        ErrorCode::AlreadyExists
    );
    assert_eq!(t.bytes("Один.md"), b"1");
    assert_eq!(t.bytes("Два.md"), b"2");
    assert_eq!(
        code(t.vault.rename(&vp("Нет.md"), "Есть.md")),
        ErrorCode::NotFound
    );
    assert_eq!(
        t.vault.rename(&vp("Один.md"), "a:b.md"),
        Err(CoreError::invalid_name(NameProblem::ForbiddenChars))
    );
    assert_eq!(
        t.vault.rename(&vp("Один.md"), "Один.md"),
        Ok(vp("Один.md")),
        "то же имя — ничего не делать"
    );
}

#[test]
fn move_entry_moves_notes_and_folders() {
    let t = TestVault::new();
    t.put("Встреча.md", "в");
    t.put("Проекты/Альфа/План.md", "п");
    std::fs::create_dir(t.file("Архив")).expect("папка");

    let note = t
        .vault
        .move_entry(&vp("Встреча.md"), &vp("Архив"))
        .expect("заметка");
    let folder = t
        .vault
        .move_entry(&vp("Проекты/Альфа"), &vp("Архив"))
        .expect("папка");
    let same = t
        .vault
        .move_entry(&vp("Архив/Встреча.md"), &vp("Архив"))
        .expect("та же папка");

    assert_eq!(note, vp("Архив/Встреча.md"));
    assert_eq!(folder, vp("Архив/Альфа"));
    assert_eq!(same, vp("Архив/Встреча.md"));
    assert_eq!(t.bytes("Архив/Альфа/План.md"), "п".as_bytes());
    assert_eq!(disk_names(&t, "Архив"), ["Альфа", "Встреча.md"]);
    assert_eq!(disk_names(&t, "Проекты"), Vec::<String>::new());
}

#[test]
fn move_entry_rejects_duplicates_cycles_and_missing_targets() {
    let t = TestVault::new();
    t.put("Встреча.md", "моя");
    t.put("Архив/встреча.MD", "старая");
    t.put("Проекты/Альфа/План.md", "");
    let v = &t.vault;

    assert_eq!(
        code(v.move_entry(&vp("Встреча.md"), &vp("Архив"))),
        ErrorCode::AlreadyExists
    );
    assert_eq!(t.bytes("Архив/встреча.MD"), "старая".as_bytes());
    assert_eq!(
        code(v.move_entry(&vp("Проекты"), &vp("Проекты/Альфа"))),
        ErrorCode::InvalidPath
    );
    assert_eq!(
        code(v.move_entry(&vp("Проекты"), &vp("Проекты"))),
        ErrorCode::InvalidPath
    );
    assert_eq!(
        code(v.move_entry(&vp("Встреча.md"), &vp("Нет"))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(v.move_entry(&vp("Встреча.md"), &vp("Встреча.md"))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(v.move_entry(&vp("Нет.md"), &vp("Архив"))),
        ErrorCode::NotFound
    );
}

/// Подменная корзина: «перемещает» в папку `trash` рядом с хранилищем или
/// сообщает, что корзины на томе нет.
fn fake_trash(t: &TestVault, has_trash: bool) -> Arc<Mutex<Vec<PathBuf>>> {
    let calls: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let log = Arc::clone(&calls);
    let bin = t.outside().join("trash");
    t.vault.set_trash_backend(Box::new(move |path: &Path| {
        log.lock().expect("журнал").push(path.to_path_buf());
        if !has_trash {
            return Err(TrashFailure::NoTrash);
        }
        std::fs::create_dir_all(&bin).expect("корзина");
        std::fs::rename(path, bin.join(path.file_name().expect("имя"))).expect("в корзину");
        Ok(())
    }));
    calls
}

#[test]
fn trash_moves_to_the_os_trash_without_asking() {
    let t = TestVault::new();
    t.put("Папка/Встреча.md", "в");
    let calls = fake_trash(&t, true);
    let asked = std::cell::Cell::new(false);

    let outcome = t
        .vault
        .trash(&vp("Папка/Встреча.md"), &|_| {
            asked.set(true);
            true
        })
        .expect("удаление");

    assert_eq!(outcome, TrashOutcome::Trashed);
    assert!(
        !asked.get(),
        "подтверждение безвозвратного удаления не нужно"
    );
    assert_eq!(calls.lock().expect("журнал").len(), 1);
    assert!(!t.file("Папка/Встреча.md").exists());
    assert!(t.outside().join("trash").join("Встреча.md").exists());
}

#[test]
fn without_trash_deletion_is_permanent_only_after_confirmation() {
    let t = TestVault::new();
    t.put("Папка/Встреча.md", "в");
    t.put("Архив/Вложенная/x.md", "x");
    let _calls = fake_trash(&t, false);
    let asked = std::cell::RefCell::new(Vec::new());

    let refused = t
        .vault
        .trash(&vp("Папка/Встреча.md"), &|path| {
            asked.borrow_mut().push(path.clone());
            false
        })
        .expect("отказ");
    assert_eq!(refused, TrashOutcome::Cancelled);
    assert_eq!(t.bytes("Папка/Встреча.md"), "в".as_bytes());
    assert_eq!(asked.borrow().as_slice(), [vp("Папка/Встреча.md")]);

    let note = t
        .vault
        .trash(&vp("Папка/Встреча.md"), &|_| true)
        .expect("согласие");
    let folder = t.vault.trash(&vp("Архив"), &|_| true).expect("папка");
    assert_eq!(note, TrashOutcome::DeletedPermanently);
    assert_eq!(folder, TrashOutcome::DeletedPermanently);
    assert!(!t.file("Папка/Встреча.md").exists());
    assert!(!t.file("Архив").exists());
    assert!(t.outside().join("secret.md").exists());
}

#[test]
fn trash_of_missing_entry_is_not_found() {
    let t = TestVault::new();
    let _calls = fake_trash(&t, true);
    assert_eq!(
        code(t.vault.trash(&vp("Нет.md"), &|_| true)),
        ErrorCode::NotFound
    );
}

/// Настоящая корзина ОС: файл либо в корзине, либо (тома без корзины) на
/// месте после отказа — молча не удаляется никогда.
#[test]
fn real_os_trash_never_deletes_silently() {
    let t = TestVault::new();
    t.put("В корзину.md", "в");
    let outcome = match t.vault.trash(&vp("В корзину.md"), &|_| false) {
        Ok(outcome) => outcome,
        Err(error) => {
            // Корзина отказала (Linux без корзины на этом томе): ошибка, файл цел.
            assert_eq!(error.code, ErrorCode::IoFailed);
            assert!(t.file("В корзину.md").exists());
            if cfg!(windows) {
                panic!("на Windows корзина на диске C: есть");
            }
            return;
        }
    };
    match outcome {
        TrashOutcome::Trashed => assert!(!t.file("В корзину.md").exists()),
        TrashOutcome::Cancelled => assert!(t.file("В корзину.md").exists()),
        TrashOutcome::DeletedPermanently => panic!("удалено без подтверждения"),
    }
    #[cfg(windows)]
    assert_eq!(
        outcome,
        TrashOutcome::Trashed,
        "на NTFS-диске C: корзина есть"
    );
}
