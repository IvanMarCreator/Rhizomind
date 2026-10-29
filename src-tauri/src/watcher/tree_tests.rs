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

/// Изменения сверки (без недоступности и повторов).
fn changes(
    model: &mut TreeModel,
    paths: impl IntoIterator<Item = VaultPath>,
    own: &OwnWrites,
) -> Vec<TreeChange> {
    let result = model.reconcile(paths, own);
    assert!(!result.unavailable, "хранилище доступно");
    assert!(result.retry.is_empty(), "повторов нет: {:?}", result.retry);
    result.changes
}

fn rescan_changes(model: &mut TreeModel, own: &OwnWrites) -> Vec<TreeChange> {
    let result = model.rescan(own);
    assert!(!result.unavailable && result.retry.is_empty());
    result.changes
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

    let changes = changes(&mut model, [vp("Новая.md")], &OwnWrites::new());

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

    let changes = changes(&mut model, [vp("А/Б/1.md")], &OwnWrites::new());

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

    let changes = changes(&mut model, [vp("А")], &OwnWrites::new());

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

    let changes = changes(&mut model, [vp("a.md")], &OwnWrites::new());

    assert_eq!(changes, vec![TreeChange::Modified { path: vp("a.md") }]);
}

#[test]
fn event_without_change_on_disk_reports_nothing() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");

    assert!(changes(&mut model, [vp("a.md"), vp("нет.md")], &OwnWrites::new()).is_empty());
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

    assert!(changes(&mut model, [vp("a.md")], &own).is_empty());
    // Модель знает новую версию: повторное событие тоже не изменение.
    assert!(changes(&mut model, [vp("a.md")], &own).is_empty());
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
        changes(&mut model, [vp("a.md")], &own),
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
        changes(&mut model, [vp("a.md")], &own),
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

    let changes = changes(
        &mut model,
        [vp("старое.md"), vp("Папка/новое.md")],
        &OwnWrites::new(),
    );

    assert_eq!(
        changes,
        vec![
            TreeChange::Created {
                path: vp("Папка"),
                entry_kind: EntryKind::Folder
            },
            TreeChange::Renamed {
                from: vp("старое.md"),
                to: vp("Папка/новое.md")
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

    let changes = changes(&mut model, [vp("А"), vp("В")], &OwnWrites::new());

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

    let changes = changes(
        &mut model,
        [vp("заметка.md"), vp("Заметка.md")],
        &OwnWrites::new(),
    );

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

    let changes = rescan_changes(&mut model, &OwnWrites::new());

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

    let changes = changes(&mut model, [vp("a.md")], &OwnWrites::new());

    assert_eq!(changes, vec![TreeChange::Modified { path: vp("a.md") }]);
}

#[test]
fn file_replaced_by_folder_is_removed_and_created() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "x", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_file(dir.path().join("x")).expect("удаление");
    fs::create_dir(dir.path().join("x")).expect("папка");

    let changes = changes(&mut model, [vp("x")], &OwnWrites::new());

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

    let changes = changes(
        &mut model,
        [vp("старая.md"), vp("новая.md")],
        &OwnWrites::new(),
    );

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
    changes(&mut model, [vp("а.md"), vp("б.md"), vp("в.md")], &own);

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

    let changes = changes(
        &mut model,
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

    let changes = changes(&mut model, [vp("Б")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Created {
                path: vp("Б"),
                entry_kind: EntryKind::Folder
            },
            TreeChange::Renamed {
                from: vp("А/заметка.md"),
                to: vp("Б/заметка.md")
            },
        ]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

// --- Ревью 1.4: Б2 — недоступность не равна удалению ---

#[test]
fn vault_root_gone_like_unplugged_drive_reports_unavailable_without_removals() {
    let parent = tempfile::tempdir().expect("временная папка");
    let root = parent.path().join("хранилище");
    write(&root, "А/1.md", "1");
    write(&root, "2.md", "2");
    let mut model = TreeModel::scan(&root).expect("обход");
    let before = model.entries();
    fs::rename(&root, parent.path().join("отключено")).expect("отключение");

    let result = model.rescan(&OwnWrites::new());

    assert!(result.unavailable, "корня нет — хранилище недоступно");
    assert!(
        result.changes.is_empty(),
        "ни одного изменения: {:?}",
        result.changes
    );
    assert_eq!(model.entries(), before, "модель не тронута");

    fs::rename(parent.path().join("отключено"), &root).expect("подключение");
    assert!(rescan_changes(&mut model, &OwnWrites::new()).is_empty());
}

#[test]
fn unreadable_folder_keeps_its_content_and_is_retried() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Закрыто/1.md", "1");
    write(dir.path(), "Закрыто/Вложено/2.md", "2");
    write(dir.path(), "открыто.md", "о");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let before = model.entries();
    // Тестовый шов: чтение папки даёт ошибку доступа (не «нет папки»).
    model.fail_listing(&vp("Закрыто"));

    let result = model.rescan(&OwnWrites::new());

    assert!(!result.unavailable);
    assert!(
        result.changes.is_empty(),
        "содержимое не исчезло: {:?}",
        result.changes
    );
    assert_eq!(result.retry, vec![vp("Закрыто")]);
    assert_eq!(model.entries(), before);

    // Доступ вернулся, а внутри за это время удалили файл.
    model.clear_failures();
    fs::remove_file(dir.path().join("Закрыто/1.md")).expect("удаление");
    assert_eq!(
        changes(&mut model, result.retry, &OwnWrites::new()),
        vec![TreeChange::Removed {
            path: vp("Закрыто/1.md")
        }]
    );
}

#[test]
fn unreadable_parent_of_event_path_keeps_entry() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Закрыто/1.md", "1");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    model.fail_listing(&vp("Закрыто"));

    let result = model.reconcile([vp("Закрыто/1.md")], &OwnWrites::new());

    assert!(result.changes.is_empty(), "{:?}", result.changes);
    assert_eq!(result.retry, vec![vp("Закрыто/1.md")]);
}

// --- Ревью 1.4: Б3 — быстрые автосохранения ---

#[test]
fn quick_successive_own_writes_are_not_external() {
    // Сценарий ревьюера: запись v2 начинается до того, как событие о v1
    // дошло до сверки.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "исходный текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    let v1 = "версия 1";
    let v2 = "версия 2, длиннее";

    own.record(&vp("a.md"), &hash(v1), v1.len() as u64);
    write(dir.path(), "a.md", v1);
    own.record(&vp("a.md"), &hash(v2), v2.len() as u64);
    assert!(
        changes(&mut model, [vp("a.md")], &own).is_empty(),
        "v1 — своя"
    );
    write(dir.path(), "a.md", v2);
    assert!(
        changes(&mut model, [vp("a.md")], &own).is_empty(),
        "v2 — своя"
    );
}

#[test]
fn foreign_content_between_own_writes_does_not_drop_own_versions() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "исходный");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    let mine = "моя версия";
    own.record(&vp("a.md"), &hash(mine), mine.len() as u64);

    write(dir.path(), "a.md", "чужая правка снаружи");
    assert_eq!(
        changes(&mut model, [vp("a.md")], &own),
        vec![TreeChange::Modified { path: vp("a.md") }]
    );
    // Своя версия всё ещё в журнале: её запись следом не внешняя.
    write(dir.path(), "a.md", mine);
    assert!(changes(&mut model, [vp("a.md")], &own).is_empty());
}

#[test]
fn revert_to_older_own_version_after_newer_one_is_external() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "n.md", "0");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let own = OwnWrites::new();
    own.record(&vp("n.md"), &hash("A"), 1);
    write(dir.path(), "n.md", "A");
    assert!(changes(&mut model, [vp("n.md")], &own).is_empty());
    own.record(&vp("n.md"), &hash("BB"), 2);
    write(dir.path(), "n.md", "BB");
    assert!(changes(&mut model, [vp("n.md")], &own).is_empty());

    write(dir.path(), "n.md", "A");
    assert_eq!(
        changes(&mut model, [vp("n.md")], &own),
        vec![TreeChange::Modified { path: vp("n.md") }]
    );
}

#[test]
fn file_moved_while_the_walk_is_in_progress_is_a_rename() {
    // Обход не мгновенный: файл прочитан в старой папке, а пока обход дошёл
    // до новой, его туда перенесли — в снимке он оказался в двух местах.
    // Так на Windows при переполнении (Rescan посреди переноса) переносы
    // уходили как «появился», а старый путь — отдельным «исчез».
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/заметка.md", "текст");
    fs::create_dir(dir.path().join("Я")).expect("папка");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let root = dir.path().to_path_buf();
    model.after_listing(
        vp("А"),
        Box::new(move || {
            fs::rename(root.join("А/заметка.md"), root.join("Я/заметка.md")).expect("перенос");
        }),
    );

    let changes = rescan_changes(&mut model, &OwnWrites::new());

    assert_eq!(
        changes,
        vec![TreeChange::Renamed {
            from: vp("А/заметка.md"),
            to: vp("Я/заметка.md")
        }]
    );
    model.clear_failures();
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

#[test]
fn file_moved_into_already_walked_folder_during_the_walk_is_a_rename() {
    // Обратный порядок: новая папка уже прочитана (файла там ещё нет), а
    // старая читается после переноса (файла там уже нет) — в снимке файла
    // нет нигде.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Я/заметка.md", "текст");
    fs::create_dir(dir.path().join("А")).expect("папка");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    let root = dir.path().to_path_buf();
    model.after_listing(
        vp("А"),
        Box::new(move || {
            fs::rename(root.join("Я/заметка.md"), root.join("А/заметка.md")).expect("перенос");
        }),
    );

    let changes = rescan_changes(&mut model, &OwnWrites::new());

    assert_eq!(
        changes,
        vec![TreeChange::Renamed {
            from: vp("Я/заметка.md"),
            to: vp("А/заметка.md")
        }]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

#[test]
fn vanished_file_is_held_until_its_new_path_shows_up_in_a_later_batch() {
    // Старый путь проверен в одной пачке (событие о папке-источнике), а
    // новый появится только в следующей: исчезнувший файл с идентификатором
    // не выдаётся сразу «исчез», а ждёт — иначе перенос открытой заметки
    // закрыл бы её. Так падал нагрузочный тест на Windows (1993 из 2000).
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/заметка.md", "текст");
    write(dir.path(), "Я/другая.md", "д");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(
        dir.path().join("А/заметка.md"),
        dir.path().join("Я/заметка.md"),
    )
    .expect("перенос");

    let first = model.reconcile_holding([vp("А")], &OwnWrites::new());
    assert!(first.changes.is_empty(), "{:?}", first.changes);
    assert_eq!(first.held, vec![vp("А/заметка.md")]);

    let second = model.reconcile_holding([vp("Я/заметка.md")], &OwnWrites::new());
    assert_eq!(
        second.changes,
        vec![TreeChange::Renamed {
            from: vp("А/заметка.md"),
            to: vp("Я/заметка.md")
        }]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

#[test]
fn held_file_still_missing_at_final_check_is_removed() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/заметка.md", "текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::remove_file(dir.path().join("А/заметка.md")).expect("удаление");

    let first = model.reconcile_holding([vp("А/заметка.md")], &OwnWrites::new());
    assert!(first.changes.is_empty());
    assert_eq!(
        changes(&mut model, first.held, &OwnWrites::new()),
        vec![TreeChange::Removed {
            path: vp("А/заметка.md")
        }]
    );
}

// --- Задача 1.9: переименование и правка в одной склейке (решение 89) ---

/// Правка на месте: тот же файл (идентификатор и дата создания прежние),
/// другое содержимое.
fn edit_in_place(root: &Path, rel: &str, text: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(root.join(rel))
        .expect("открыть на запись");
    file.write_all(text.as_bytes()).expect("запись");
}

#[test]
fn renamed_and_edited_file_in_one_batch_is_a_rename_and_a_modification() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("a.md"), dir.path().join("b.md")).expect("переименование");
    edit_in_place(dir.path(), "b.md", "текст и правка снаружи");

    let changes = changes(&mut model, [vp("a.md"), vp("b.md")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("a.md"),
                to: vp("b.md")
            },
            TreeChange::Modified { path: vp("b.md") },
        ]
    );
    assert_eq!(model, TreeModel::scan(dir.path()).expect("обход"));
}

#[test]
fn moved_to_another_folder_and_edited_in_one_batch_is_a_rename_and_a_modification() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст");
    fs::create_dir(dir.path().join("Архив")).expect("папка");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("a.md"), dir.path().join("Архив/b.md")).expect("перенос");
    edit_in_place(dir.path(), "Архив/b.md", "другой текст");

    let changes = changes(
        &mut model,
        [vp("a.md"), vp("Архив/b.md")],
        &OwnWrites::new(),
    );

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("a.md"),
                to: vp("Архив/b.md")
            },
            TreeChange::Modified {
                path: vp("Архив/b.md")
            },
        ]
    );
}

#[test]
fn renamed_and_edited_file_whose_old_path_came_a_batch_earlier_is_a_rename() {
    // Исчезнувший файл ждёт свой новый путь (0,6 с): новый путь пришёл
    // следующей пачкой, а файл к этому времени уже изменён.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/a.md", "текст");
    write(dir.path(), "Я/другая.md", "д");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("А/a.md"), dir.path().join("Я/b.md")).expect("перенос");
    edit_in_place(dir.path(), "Я/b.md", "текст, изменённый после переноса");

    let first = model.reconcile_holding([vp("А")], &OwnWrites::new());
    assert!(first.changes.is_empty(), "{:?}", first.changes);
    let second = model.reconcile_holding([vp("Я/b.md")], &OwnWrites::new());
    assert_eq!(
        second.changes,
        vec![
            TreeChange::Renamed {
                from: vp("А/a.md"),
                to: vp("Я/b.md")
            },
            TreeChange::Modified {
                path: vp("Я/b.md")
            },
        ]
    );
}

#[test]
fn renamed_and_rewritten_as_a_new_file_is_a_removal_and_a_creation() {
    // Программа записала переименованный файл заново (новый идентификатор):
    // это уже другой файл.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("a.md"), dir.path().join("b.md")).expect("переименование");
    let tmp = dir.path().join("b.md.tmp");
    fs::write(&tmp, "текст, записанный заново").expect("временный файл");
    fs::rename(&tmp, dir.path().join("b.md")).expect("замена");

    let changes = changes(
        &mut model,
        [vp("a.md"), vp("b.md"), vp("b.md.tmp")],
        &OwnWrites::new(),
    );

    assert_eq!(
        changes,
        vec![
            TreeChange::Removed { path: vp("a.md") },
            TreeChange::Created {
                path: vp("b.md"),
                entry_kind: EntryKind::Note
            },
        ]
    );
}

#[test]
fn renamed_folder_with_an_edited_note_inside_keeps_the_note() {
    // Папку переименовали, а заметку в ней в ту же склейку изменили: это
    // переименование папки и правка заметки, а не «заметка исчезла и
    // появилась».
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/1.md", "1");
    write(dir.path(), "А/2.md", "2");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(dir.path().join("А"), dir.path().join("Б")).expect("переименование");
    edit_in_place(dir.path(), "Б/2.md", "2 и правка");

    let changes = changes(&mut model, [vp("А"), vp("Б")], &OwnWrites::new());

    assert_eq!(
        changes,
        vec![
            TreeChange::Renamed {
                from: vp("А"),
                to: vp("Б")
            },
            TreeChange::Modified {
                path: vp("Б/2.md")
            },
        ]
    );
}

#[test]
fn emacs_backup_with_the_same_id_is_not_a_rename_of_the_note() {
    // Emacs: заметка → `~` (тот же идентификатор), новая заметка, копия
    // остаётся. Вид элемента другой (заметка → вложение) — не переименование.
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Заметка.md", "старый текст");
    let mut model = TreeModel::scan(dir.path()).expect("обход");
    fs::rename(
        dir.path().join("Заметка.md"),
        dir.path().join("Заметка.md~"),
    )
    .expect("копия");
    write(dir.path(), "Заметка.md", "новый текст, длиннее");
    edit_in_place(dir.path(), "Заметка.md~", "копия изменилась");

    let changes = changes(
        &mut model,
        [vp("Заметка.md"), vp("Заметка.md~")],
        &OwnWrites::new(),
    );

    assert_eq!(
        changes,
        vec![
            TreeChange::Created {
                path: vp("Заметка.md~"),
                entry_kind: EntryKind::Attachment
            },
            TreeChange::Modified {
                path: vp("Заметка.md")
            },
        ]
    );
}
