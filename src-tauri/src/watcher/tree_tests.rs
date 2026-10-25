//! Модель «дерево хранилища»: сверка с диском по путям из событий ОС и
//! полная сверка (Rescan). Детерминированно, без наблюдателя ОС.

use std::fs;
use std::path::Path;

use super::own_writes::OwnWrites;
use super::tree::TreeModel;
use crate::commands::{EntryKind, TreeChange, VaultPath};

fn vp(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("родитель")).expect("папка");
    fs::write(path, text).expect("запись");
}

fn hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[test]
fn scan_lists_folders_notes_and_attachments_without_hidden_entries() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Проекты/План.md", "# План");
    write(dir.path(), "картинка.png", "png");
    write(dir.path(), ".rhizomind/settings.json", "{}");
    write(dir.path(), ".git/HEAD", "ref");
    write(dir.path(), ".скрытая.md", "x");

    let model = TreeModel::scan(dir.path()).expect("обход");

    assert_eq!(
        model.entries(),
        vec![
            (vp("Проекты"), EntryKind::Folder),
            (vp("Проекты/План.md"), EntryKind::Note),
            (vp("картинка.png"), EntryKind::Attachment),
        ]
    );
}

#[test]
fn created_file_is_reported_with_its_kind() {
    let dir = tempfile::tempdir().expect("временная папка");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    write(dir.path(), "Новая.md", "текст");

    let changes = model.reconcile([vp("Новая.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![TreeChange::Created {
            path: vp("Новая.md"),
            entry_kind: EntryKind::Note
        }]
    );
    assert_eq!(model.entries(), vec![(vp("Новая.md"), EntryKind::Note)]);
}

#[test]
fn event_inside_new_folder_creates_folder_and_all_its_content() {
    // inotify не успевает поставить слежение на новую папку: события о
    // файлах внутри теряются, поэтому новая папка обходится целиком.
    let dir = tempfile::tempdir().expect("временная папка");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    write(dir.path(), "А/Б/1.md", "1");
    write(dir.path(), "А/Б/2.md", "2");

    let changes = model.reconcile([vp("А/Б/1.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Created {
                path: vp("А"),
                entry_kind: EntryKind::Folder
            },
            TreeChange::Created {
                path: vp("А/Б"),
                entry_kind: EntryKind::Folder
            },
            TreeChange::Created {
                path: vp("А/Б/1.md"),
                entry_kind: EntryKind::Note
            },
            TreeChange::Created {
                path: vp("А/Б/2.md"),
                entry_kind: EntryKind::Note
            },
        ]
    );
}

#[test]
fn removed_folder_reports_children_first() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/1.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_dir_all(dir.path().join("А")).expect("удаление");

    let changes = model.reconcile([vp("А")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Removed {
                path: vp("А/1.md")
            },
            TreeChange::Removed { path: vp("А") },
        ]
    );
    assert!(model.entries().is_empty());
}

#[test]
fn changed_content_is_modified() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    write(dir.path(), "a.md", "больше текста");

    let changes = model.reconcile([vp("a.md")], &OwnWrites::new());

    assert_eq!(changes, vec![TreeChange::Modified { path: vp("a.md") }]);
}

#[test]
fn event_without_change_on_disk_reports_nothing() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");

    assert!(
        model
            .reconcile([vp("a.md"), vp("нет.md")], &OwnWrites::new())
            .is_empty()
    );
}

#[test]
fn own_write_with_same_hash_and_size_is_not_external() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    let text = "новый текст из редактора";

    own.record(&vp("a.md"), &hash(text), text.len() as u64);
    write(dir.path(), "a.md", text);

    assert!(model.reconcile([vp("a.md")], &own).is_empty());
    // Модель знает новую версию: повторное событие тоже не изменение.
    assert!(model.reconcile([vp("a.md")], &own).is_empty());
}

#[test]
fn external_write_after_own_write_is_modified() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    own.record(&vp("a.md"), &hash("свой"), 8);

    write(dir.path(), "a.md", "чужой текст другой длины");

    assert_eq!(
        model.reconcile([vp("a.md")], &own),
        vec![TreeChange::Modified { path: vp("a.md") }]
    );
}

#[test]
fn own_write_with_same_size_but_other_content_is_modified() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    own.record(&vp("a.md"), &hash("abc"), 3);

    write(dir.path(), "a.md", "xyz");

    assert_eq!(
        model.reconcile([vp("a.md")], &own),
        vec![TreeChange::Modified { path: vp("a.md") }]
    );
}

#[test]
fn renamed_file_is_recognised_by_file_id() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "старое.md", "текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::create_dir(dir.path().join("Папка")).expect("папка");
    fs::rename(
        dir.path().join("старое.md"),
        dir.path().join("Папка/новое.md"),
    )
    .expect("перенос");

    let changes = model.reconcile([vp("старое.md"), vp("Папка/новое.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("старое.md"),
                to: vp("Папка/новое.md")
            },
            TreeChange::Created {
                path: vp("Папка"),
                entry_kind: EntryKind::Folder
            },
        ]
    );
}

#[test]
fn renamed_folder_moves_its_content_in_one_change() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/1.md", "1");
    write(dir.path(), "А/Б/2.md", "2");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("А"), dir.path().join("В")).expect("перенос");

    let changes = model.reconcile([vp("А"), vp("В")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![TreeChange::Renamed {
            from: vp("А"),
            to: vp("В")
        }]
    );
    assert_eq!(
        model.entries(),
        vec![
            (vp("В"), EntryKind::Folder),
            (vp("В/1.md"), EntryKind::Note),
            (vp("В/Б"), EntryKind::Folder),
            (vp("В/Б/2.md"), EntryKind::Note),
        ]
    );
}

#[test]
fn case_only_rename_is_a_rename() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "заметка.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("заметка.md"), dir.path().join("Заметка.md")).expect("перенос");

    let changes = model.reconcile([vp("заметка.md"), vp("Заметка.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![TreeChange::Renamed {
            from: vp("заметка.md"),
            to: vp("Заметка.md")
        }]
    );
}

#[test]
fn rescan_finds_everything_changed_without_events() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "удалить.md", "1");
    write(dir.path(), "изменить.md", "1");
    write(dir.path(), "перенести.md", "перенос");
    let mut model = TreeModel::scan(dir.path()).expect("обход");

    fs::remove_file(dir.path().join("удалить.md")).expect("удаление");
    write(dir.path(), "изменить.md", "другой текст");
    fs::rename(
        dir.path().join("перенести.md"),
        dir.path().join("перенесено.md"),
    )
    .expect("перенос");
    write(dir.path(), "новая.md", "н");

    let changes = model.rescan(&OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("перенести.md"),
                to: vp("перенесено.md")
            },
            TreeChange::Removed {
                path: vp("удалить.md")
            },
            TreeChange::Created {
                path: vp("новая.md"),
                entry_kind: EntryKind::Note
            },
            TreeChange::Modified {
                path: vp("изменить.md")
            },
        ]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

#[test]
fn replaced_file_like_git_checkout_is_modified_not_removed() {
    // git checkout удаляет файл и создаёт новый с тем же именем.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_file(dir.path().join("a.md")).expect("удаление");
    write(dir.path(), "a.md", "22");

    let changes = model.reconcile([vp("a.md")], &OwnWrites::new());

    assert_eq!(changes, vec![TreeChange::Modified { path: vp("a.md") }]);
}

#[test]
fn file_replaced_by_folder_is_removed_and_created() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "x", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_file(dir.path().join("x")).expect("удаление");
    fs::create_dir(dir.path().join("x")).expect("папка");

    let changes = model.reconcile([vp("x")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Removed { path: vp("x") },
            TreeChange::Created {
                path: vp("x"),
                entry_kind: EntryKind::Folder
            },
        ]
    );
}

#[test]
fn deleted_file_and_new_file_with_reused_id_are_not_a_rename() {
    // На Linux номер inode удалённого файла сразу достаётся новому.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "старая.md", "старый текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_file(dir.path().join("старая.md")).expect("удаление");
    write(dir.path(), "новая.md", "н");

    let changes = model.reconcile([vp("старая.md"), vp("новая.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Removed {
                path: vp("старая.md")
            },
            TreeChange::Created {
                path: vp("новая.md"),
                entry_kind: EntryKind::Note
            },
        ]
    );
}

#[test]
fn journal_forgets_removed_and_renamed_paths() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "а.md", "1");
    write(dir.path(), "б.md", "2");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    own.record(&vp("а.md"), &hash("1"), 1);
    own.record(&vp("б.md"), &hash("2"), 1);

    fs::remove_file(dir.path().join("а.md")).expect("удаление");
    fs::rename(dir.path().join("б.md"), dir.path().join("в.md")).expect("перенос");
    model.reconcile([vp("а.md"), vp("б.md"), vp("в.md")], &own);

    assert_eq!(own.len(), 0);
}

#[test]
fn changes_inside_renamed_folder_apply_in_order() {
    // Файл вынесли из папки, другой удалили в ней, а папку переименовали —
    // всё одной пачкой. Изменения, применённые по порядку, дают диск.
    use super::test_support::{Mirror, disk_tree};
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Ф/вынести.md", "1");
    write(dir.path(), "Ф/удалить.md", "22");
    write(dir.path(), "Ф/остаётся.md", "333");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let mut mirror = Mirror::from_disk(dir.path());
    fs::create_dir(dir.path().join("Перенесённые")).expect("папка");
    fs::rename(
        dir.path().join("Ф/вынести.md"),
        dir.path().join("Перенесённые/вынесено.md"),
    )
    .expect("перенос");
    fs::remove_file(dir.path().join("Ф/удалить.md")).expect("удаление");
    fs::rename(dir.path().join("Ф"), dir.path().join("Переименованная")).expect("перенос");

    let changes = model.reconcile(
        [vp("Ф"), vp("Переименованная"), vp("Перенесённые")],
        &OwnWrites::new(),
    );
    mirror.apply(&crate::commands::VaultChangedPayload {
        rescan: false,
        changes: changes.clone(),
    });

    assert_eq!(mirror.tree, disk_tree(dir.path()), "изменения: {changes:?}");
    assert!(changes.contains(&TreeChange::Renamed {
        from: vp("Переименованная/вынести.md"),
        to: vp("Перенесённые/вынесено.md")
    }));
}

#[test]
fn rename_is_recognised_when_event_about_old_path_was_lost() {
    // При переполнении буфера Windows событие о старом имени теряется, а
    // новое приходит (или папку с перенесёнными файлами обходят целиком).
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/заметка.md", "текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::create_dir(dir.path().join("Б")).expect("папка");
    fs::rename(
        dir.path().join("А/заметка.md"),
        dir.path().join("Б/заметка.md"),
    )
    .expect("перенос");

    let changes = model.reconcile([vp("Б")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("А/заметка.md"),
                to: vp("Б/заметка.md")
            },
            TreeChange::Created {
                path: vp("Б"),
                entry_kind: EntryKind::Folder
            },
        ]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}
