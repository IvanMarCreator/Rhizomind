//! Наблюдатель с настоящими событиями ОС: короткие сценарии.
//! Нагрузочные — в `load_tests.rs`.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::VaultWatcher;
use super::test_support::{Collector, Mirror, vp};
use crate::commands::{EntryKind, ErrorCode, FileVersion, TreeChange};

/// Сколько ждать события: спецификация обещает дерево «в течение секунды»,
/// запас — на медленные машины CI.
const WAIT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(800);

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("родитель")).expect("папка");
    fs::write(path, text).expect("запись");
}

/// Запись как у хранилища: временный файл в `.rhizomind/tmp/` → замена.
fn write_atomically(root: &Path, rel: &str, text: &str) {
    let tmp_dir = root.join(".rhizomind/tmp");
    fs::create_dir_all(&tmp_dir).expect("папка");
    let tmp = tmp_dir.join("запись.tmp");
    fs::write(&tmp, text).expect("временный файл");
    fs::rename(&tmp, root.join(rel)).expect("замена");
}

fn version(text: &str) -> FileVersion {
    FileVersion {
        hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
        size: text.len() as u64,
        modified_ms: 0,
    }
}

#[test]
fn missing_vault_folder_is_unavailable() {
    let dir = tempfile::tempdir().expect("временная папка");
    let err = VaultWatcher::start(&dir.path().join("нет"), Collector::default().sink())
        .expect_err("папки нет");
    assert_eq!(err.code, ErrorCode::VaultUnavailable);
}

#[test]
fn external_changes_arrive_as_tree_changes() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "есть.md", "1");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    write(dir.path(), "Новая.md", "текст");
    assert!(
        collector.wait_for(WAIT, |_| collector.changes().contains(
            &TreeChange::Created {
                path: vp("Новая.md"),
                entry_kind: EntryKind::Note
            }
        )),
        "нет события о новой заметке: {:?}",
        collector.payloads()
    );

    write(dir.path(), "есть.md", "другой текст");
    assert!(
        collector.wait_for(WAIT, |_| collector.changes().contains(
            &TreeChange::Modified {
                path: vp("есть.md")
            }
        )),
        "нет события об изменении: {:?}",
        collector.payloads()
    );

    fs::rename(dir.path().join("Новая.md"), dir.path().join("Другая.md")).expect("перенос");
    assert!(
        collector.wait_for(WAIT, |_| collector.changes().contains(
            &TreeChange::Renamed {
                from: vp("Новая.md"),
                to: vp("Другая.md")
            }
        )),
        "нет события о переименовании: {:?}",
        collector.payloads()
    );
    assert!(collector.payloads().iter().all(|p| !p.rescan));
}

#[test]
fn service_folder_and_hidden_files_are_ignored() {
    let dir = tempfile::tempdir().expect("временная папка");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    write(dir.path(), ".rhizomind/settings.json", "{}");
    write(dir.path(), ".rhizomind/tmp/x.tmp", "x");
    write(dir.path(), ".git/HEAD", "ref");
    write(dir.path(), "метка.md", "m");

    assert!(collector.wait_for(WAIT, |_| !collector.changes().is_empty()));
    std::thread::sleep(QUIET);
    assert_eq!(
        collector.changes(),
        vec![TreeChange::Created {
            path: vp("метка.md"),
            entry_kind: EntryKind::Note
        }]
    );
}

#[test]
fn own_atomic_write_is_not_external_change() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "моя.md", "старый текст");
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    let text = "текст из редактора";
    watcher.remember_own_write(&vp("моя.md"), &version(text));
    write_atomically(dir.path(), "моя.md", text);
    // Метка: внешнее изменение после своей записи.
    write(dir.path(), "метка.md", "m");

    assert!(collector.wait_for(WAIT, |_| collector.changes().contains(
        &TreeChange::Created {
            path: vp("метка.md"),
            entry_kind: EntryKind::Note
        }
    )));
    std::thread::sleep(QUIET);
    assert!(
        !collector
            .changes()
            .iter()
            .any(|c| matches!(c, TreeChange::Modified { path } if *path == vp("моя.md"))),
        "своя запись ушла как внешняя: {:?}",
        collector.payloads()
    );

    // А вот внешняя запись того же файла — изменение.
    write(dir.path(), "моя.md", "чужая правка");
    assert!(collector.wait_for(WAIT, |_| collector.changes().contains(
        &TreeChange::Modified {
            path: vp("моя.md")
        }
    )));
}

#[test]
fn rescan_after_lost_events_brings_tree_in_line_with_disk() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "А/1.md", "1");
    write(dir.path(), "удалить.md", "x");
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    // Тестовый шов: события ОС теряются, как при переполнении буфера.
    watcher.lose_os_events(true);
    fs::rename(dir.path().join("А"), dir.path().join("Б")).expect("перенос");
    fs::remove_file(dir.path().join("удалить.md")).expect("удаление");
    write(dir.path(), "новая/2.md", "2");
    std::thread::sleep(QUIET);
    assert!(
        collector.payloads().is_empty(),
        "события должны были потеряться"
    );
    watcher.lose_os_events(false);

    watcher.rescan();

    collector
        .settle(&mut mirror, dir.path(), QUIET, WAIT)
        .expect("после Rescan дерево совпадает с диском");
    let payloads = collector.payloads();
    assert!(payloads[0].rescan, "пачка после полной сверки — с rescan");
    assert!(payloads[0].changes.contains(&TreeChange::Renamed {
        from: vp("А"),
        to: vp("Б")
    }));
}

#[test]
fn rescan_without_changes_still_tells_interface() {
    let dir = tempfile::tempdir().expect("временная папка");
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    watcher.rescan();

    assert!(collector.wait_for(WAIT, |p| p.len() == 1));
    assert!(collector.payloads()[0].rescan);
    assert!(collector.payloads()[0].changes.is_empty());
}

#[test]
fn no_changes_are_delivered_after_watcher_is_dropped() {
    let dir = tempfile::tempdir().expect("временная папка");
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    drop(watcher);

    write(dir.path(), "после.md", "x");
    std::thread::sleep(QUIET);

    assert!(collector.payloads().is_empty());
}

// --- Ревью 1.4 ---

#[cfg(unix)]
#[test]
fn vault_folder_disappearing_signals_unavailable_without_removals() {
    // Как отключённая флешка: корня по этому пути больше нет. На Windows
    // папку под наблюдением переименовать нельзя — там проверка в tree_tests.
    use super::WatcherEvent;
    let parent = tempfile::tempdir().expect("временная папка");
    let root = parent.path().join("хранилище");
    write(&root, "1.md", "1");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(&root, collector.sink()).expect("слежение");

    let away = parent.path().join("отключено");
    fs::rename(&root, &away).expect("отключение");
    write(&away, "2.md", "2");
    assert!(
        collector.wait_for(WAIT, |_| collector
            .signals()
            .contains(&WatcherEvent::Unavailable)),
        "нет сигнала «хранилище недоступно»: {:?}",
        collector.signals()
    );
    assert!(
        !collector
            .changes()
            .iter()
            .any(|c| matches!(c, TreeChange::Removed { .. })),
        "{:?}",
        collector.payloads()
    );

    fs::rename(&away, &root).expect("подключение");
    assert!(
        collector.wait_for(WAIT, |p| collector
            .signals()
            .contains(&WatcherEvent::Available)
            && p.iter().any(|p| p.rescan
                && p.changes.contains(&TreeChange::Created {
                    path: vp("2.md"),
                    entry_kind: EntryKind::Note
                }))),
        "после возвращения — сигнал и полная сверка: {:?} {:?}",
        collector.signals(),
        collector.payloads()
    );
}

#[test]
fn panicking_sink_does_not_stop_the_watcher() {
    use super::WatcherEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().expect("временная папка");
    let calls = Arc::new(AtomicUsize::new(0));
    let received = Arc::new(Mutex::new(Vec::new()));
    let (c, r) = (calls.clone(), received.clone());
    let _watcher = VaultWatcher::start(
        dir.path(),
        Box::new(move |event| {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("получатель упал");
            }
            r.lock().expect("список").push(event);
        }),
    )
    .expect("слежение");

    write(dir.path(), "1.md", "1");
    std::thread::sleep(Duration::from_secs(2));
    write(dir.path(), "2.md", "2");

    let start = std::time::Instant::now();
    while start.elapsed() < WAIT {
        let got = received.lock().expect("список").clone();
        let failed = got.contains(&WatcherEvent::DeliveryFailed);
        let rescued = got.iter().any(|e| {
            matches!(e, WatcherEvent::Changed(p) if p.rescan && p.changes.iter().any(|c|
                matches!(c, TreeChange::Created { path, .. } if *path == vp("2.md"))))
        });
        if failed && rescued {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "после паники получателя — сигнал и полная сверка: {:?}",
        received.lock().expect("список")
    );
}

/// После сбоя получателя полная сверка приходит сама, по таймеру, — не
/// дожидаясь следующего изменения на диске (повторное ревью 1.4).
#[test]
fn delivery_failure_schedules_full_rescan_without_new_changes() {
    use super::WatcherEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().expect("временная папка");
    let calls = Arc::new(AtomicUsize::new(0));
    let received = Arc::new(Mutex::new(Vec::new()));
    let (c, r) = (calls.clone(), received.clone());
    let _watcher = VaultWatcher::start(
        dir.path(),
        Box::new(move |event| {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("получатель упал");
            }
            r.lock().expect("список").push(event);
        }),
    )
    .expect("слежение");

    write(dir.path(), "1.md", "1");

    let start = std::time::Instant::now();
    while start.elapsed() < WAIT {
        let got = received.lock().expect("список").clone();
        // Модель уже знает «1.md» — сверка без изменений, но с `rescan: true`:
        // интерфейс перечитывает дерево целиком.
        let rescued = got
            .iter()
            .any(|e| matches!(e, WatcherEvent::Changed(p) if p.rescan));
        if got.contains(&WatcherEvent::DeliveryFailed) && rescued {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "после сбоя получателя — полная сверка по таймеру: {:?}",
        received.lock().expect("список")
    );
}

#[test]
fn rescan_flag_and_watcher_errors_from_notify_request_full_rescan() {
    use super::EventPaths;
    use notify::event::{EventKind, Flag};
    use notify_debouncer_full::DebouncedEvent;
    let dir = tempfile::tempdir().expect("временная папка");
    let paths = EventPaths::new(dir.path());
    let now = std::time::Instant::now();

    let flagged = paths.batch(Ok(vec![DebouncedEvent::new(
        notify::Event::new(EventKind::Other).set_flag(Flag::Rescan),
        now,
    )]));
    assert!(flagged.rescan);

    let failed = paths.batch(Err(vec![notify::Error::generic("переполнение")]));
    assert!(failed.rescan);

    let filtered = paths.batch(Ok(vec![DebouncedEvent::new(
        notify::Event::new(EventKind::Any)
            .add_path(dir.path().join(".rhizomind").join("tmp").join("x"))
            .add_path(dir.path().join("Папка").join("a.md")),
        now,
    )]));
    assert!(!filtered.rescan);
    assert_eq!(filtered.paths, vec![vp("Папка/a.md")]);
}

// --- Повторное ревью 1.4 (тесты ревьюера) ---

#[cfg(unix)]
#[test]
fn events_arrive_after_vault_folder_is_recreated() {
    // notify снимает наблюдение, когда корень пропадает (inotify:
    // IN_DELETE_SELF; Windows: ERROR_ACCESS_DENIED → unwatch).
    use super::WatcherEvent;
    let parent = tempfile::tempdir().expect("временная папка");
    let root = parent.path().join("хранилище");
    write(&root, "1.md", "1");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(&root, collector.sink()).expect("слежение");

    fs::remove_dir_all(&root).expect("удаление корня");
    assert!(
        collector.wait_for(WAIT, |_| collector
            .signals()
            .contains(&WatcherEvent::Unavailable)),
        "{:?} {:?}",
        collector.signals(),
        collector.payloads()
    );
    write(&root, "1.md", "1");
    assert!(
        collector.wait_for(WAIT, |_| collector
            .signals()
            .contains(&WatcherEvent::Available)),
        "{:?}",
        collector.signals()
    );
    std::thread::sleep(Duration::from_secs(1));

    write(&root, "после.md", "x");
    assert!(
        collector.wait_for(WAIT, |_| collector.changes().contains(
            &TreeChange::Created {
                path: vp("после.md"),
                entry_kind: EntryKind::Note
            }
        )),
        "событие после возвращения корня не пришло: {:?}",
        collector.payloads()
    );
}

#[test]
fn external_revert_to_older_own_version_is_external() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "n.md", "0");
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    watcher.remember_own_write(&vp("n.md"), &version("AAAA"));
    write_atomically(dir.path(), "n.md", "AAAA");
    std::thread::sleep(Duration::from_secs(2));
    watcher.remember_own_write(&vp("n.md"), &version("BBBB"));
    write_atomically(dir.path(), "n.md", "BBBB");
    std::thread::sleep(Duration::from_secs(2));

    // Внешняя программа вернула прежнюю версию.
    write(dir.path(), "n.md", "AAAA");
    write(dir.path(), "метка.md", "m");
    assert!(collector.wait_for(WAIT, |_| collector.changes().contains(
        &TreeChange::Created {
            path: vp("метка.md"),
            entry_kind: EntryKind::Note
        }
    )));
    std::thread::sleep(QUIET);
    assert!(
        collector
            .changes()
            .contains(&TreeChange::Modified { path: vp("n.md") }),
        "внешний откат к своей прежней версии подавлен: {:?}",
        collector.payloads()
    );
}

#[test]
fn retry_of_unreadable_paths_backs_off_up_to_a_minute() {
    // Папки, которые не читаются никогда (`System Volume Information`), не
    // должны сверяться каждые 2 секунды вечно.
    use super::retry_delay;
    assert_eq!(retry_delay(0), Duration::from_secs(2));
    assert_eq!(retry_delay(1), Duration::from_secs(4));
    assert_eq!(retry_delay(2), Duration::from_secs(8));
    assert_eq!(retry_delay(10), Duration::from_secs(60));
    assert_eq!(retry_delay(u32::MAX), Duration::from_secs(60));
}

/// Сохранение заметки внешним редактором (задача 2.9, вопрос 89): какой бы
/// способ записи ни был у редактора, открытая заметка видна как изменённая
/// (`Modified`), а не как исчезнувшая или переехавшая — иначе окно закрыло бы
/// её как удалённую и отбросило несохранённый ввод.
fn assert_saved_as_modification(name: &str, save: impl FnOnce(&Path)) {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "Заметка.md", "старый текст\n");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    save(dir.path());

    let modified = TreeChange::Modified {
        path: vp("Заметка.md"),
    };
    assert!(
        collector.wait_for(WAIT, |_| collector.changes().contains(&modified)),
        "{name}: нет изменения заметки: {:?}",
        collector.payloads()
    );
    std::thread::sleep(QUIET);
    let note = vp("Заметка.md");
    // Для самой заметки — только `Modified`: ни `Removed`, ни `Renamed`, ни
    // `Created` (иначе окно закрыло бы её как удалённую).
    let other = collector.changes().into_iter().find(|change| match change {
        TreeChange::Created { path, .. } | TreeChange::Removed { path } => *path == note,
        TreeChange::Renamed { from, to } => *from == note || *to == note,
        TreeChange::Modified { .. } => false,
    });
    assert_eq!(
        other,
        None,
        "{name}: заметка «исчезла»: {:?}",
        collector.payloads()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("Заметка.md")).expect("заметка"),
        "новый текст\n"
    );
}

#[test]
fn editor_save_in_place_is_a_modification() {
    // Блокнот, Notepad++, VS Code (по умолчанию), Vim с `backupcopy=yes`.
    assert_saved_as_modification("на месте", |root| {
        fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
    });
}

#[test]
fn editor_save_through_temp_file_is_a_modification() {
    // Атомарная запись: временный файл рядом → замена (Sublime, VS Code
    // с атомарной записью, многие программы).
    assert_saved_as_modification("через временный файл", |root| {
        let tmp = root.join("Заметка.md.tmp1234");
        fs::write(&tmp, "новый текст\n").expect("временный файл");
        fs::rename(&tmp, root.join("Заметка.md")).expect("замена");
    });
}

#[test]
fn editor_save_with_backup_rename_is_a_modification() {
    // Vim (`backupcopy=no`) и Emacs: заметка → резервная копия `~`, новая
    // заметка пишется заново, копия удаляется (Emacs — оставляет).
    assert_saved_as_modification("с резервной копией", |root| {
        fs::rename(root.join("Заметка.md"), root.join("Заметка.md~")).expect("резервная копия");
        fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
        fs::remove_file(root.join("Заметка.md~")).expect("удаление копии");
    });
}

#[test]
fn editor_safe_write_like_jetbrains_is_a_modification() {
    // «Безопасная запись» JetBrains: новый текст во временный файл, заметка
    // → `___jb_old___`, временный → заметка, старый удаляется.
    assert_saved_as_modification("безопасная запись", |root| {
        let tmp = root.join("Заметка.md___jb_tmp___");
        let old = root.join("Заметка.md___jb_old___");
        fs::write(&tmp, "новый текст\n").expect("временный файл");
        fs::rename(root.join("Заметка.md"), &old).expect("старый");
        fs::rename(&tmp, root.join("Заметка.md")).expect("замена");
        fs::remove_file(&old).expect("удаление старого");
    });
}

#[test]
fn editor_save_with_backup_copy_is_a_modification() {
    // Резервная копия `.bak` (Notepad++, Far, многие программы): копия рядом,
    // заметка переписывается на месте, копия остаётся.
    assert_saved_as_modification("с копией .bak", |root| {
        fs::copy(root.join("Заметка.md"), root.join("Заметка.md.bak")).expect("копия");
        fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
    });
}

#[test]
fn editor_save_by_delete_and_create_is_a_modification() {
    // «Удалить и положить новый»: старый файл удаляется, новый пишется под
    // тем же именем (простые скрипты, некоторые редакторы).
    assert_saved_as_modification("удалить и положить новый", |root| {
        fs::remove_file(root.join("Заметка.md")).expect("удаление");
        fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
    });
}

#[test]
fn emacs_save_leaving_backup_copy_is_a_modification() {
    // Emacs (по умолчанию): заметка → `Заметка.md~` (тот же идентификатор и
    // дата создания), новая заметка пишется заново, копия остаётся.
    assert_saved_as_modification("Emacs, копия остаётся", |root| {
        fs::rename(root.join("Заметка.md"), root.join("Заметка.md~")).expect("резервная копия");
        fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
    });
}

#[test]
fn vim_save_with_backup_removed_after_a_delay_is_a_modification() {
    // Vim (`backupcopy=no`, `writebackup`): копия `~` удаляется после
    // записи — сразу или позже, в другой склейке событий.
    for delay_ms in [0, 100, 300, 500, 700] {
        assert_saved_as_modification(
            &format!("Vim, копия удалена через {delay_ms} мс"),
            |root| {
                fs::rename(root.join("Заметка.md"), root.join("Заметка.md~"))
                    .expect("резервная копия");
                fs::write(root.join("Заметка.md"), "новый текст\n").expect("запись");
                std::thread::sleep(Duration::from_millis(delay_ms));
                fs::remove_file(root.join("Заметка.md~")).expect("удаление копии");
            },
        );
    }
}

// --- Задача 1.9: переименование и правка в одной склейке (решение 89) ---

/// Переименование `from` → `to` и сразу правка на месте (тот же файл).
fn rename_then_edit(root: &Path, from: &str, to: &str, text: &str) {
    use std::io::Write;
    fs::rename(root.join(from), root.join(to)).expect("переименование");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(root.join(to))
        .expect("открыть на запись");
    file.write_all(text.as_bytes()).expect("запись");
}

/// После затихания: для `from` и `to` только переименование и правка `to`.
fn assert_renamed_and_modified(collector: &Collector, from: &str, to: &str) {
    let renamed = TreeChange::Renamed {
        from: vp(from),
        to: vp(to),
    };
    let modified = TreeChange::Modified { path: vp(to) };
    assert!(
        collector.wait_for(WAIT, |_| {
            let changes = collector.changes();
            changes.contains(&renamed) && changes.contains(&modified)
        }),
        "нет переименования и правки: {:?}",
        collector.payloads()
    );
    std::thread::sleep(QUIET);
    let (from, to) = (vp(from), vp(to));
    let other: Vec<_> = collector
        .changes()
        .into_iter()
        .filter(|change| match change {
            TreeChange::Created { path, .. } | TreeChange::Removed { path } => {
                *path == from || *path == to
            }
            _ => false,
        })
        .collect();
    assert_eq!(other, vec![], "{:?}", collector.payloads());
    let changes = collector.changes();
    let at = |c: &TreeChange| changes.iter().position(|x| x == c).expect("есть");
    assert!(
        at(&renamed) < at(&modified),
        "правка — после переименования: {changes:?}"
    );
}

#[test]
fn note_renamed_and_edited_at_once_is_a_rename_and_a_modification() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст\n");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    rename_then_edit(dir.path(), "a.md", "b.md", "текст\nи правка снаружи\n");

    assert_renamed_and_modified(&collector, "a.md", "b.md");
}

#[test]
fn note_moved_to_another_folder_and_edited_at_once_is_a_rename_and_a_modification() {
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст\n");
    fs::create_dir(dir.path().join("Архив")).expect("папка");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    rename_then_edit(dir.path(), "a.md", "Архив/b.md", "другой текст\n");

    assert_renamed_and_modified(&collector, "a.md", "Архив/b.md");
}

#[test]
fn note_renamed_and_written_anew_is_a_removal_and_a_creation() {
    // Новый идентификатор: программа записала файл под новым именем заново
    // («Сохранить как» и удаление старого). Переименование с последующей
    // атомарной заменой — в `tree_tests` (на Linux склейка `notify` теряет
    // путь источника такого переименования: см. обзор задачи 1.9).
    let dir = tempfile::tempdir().expect("временная папка");
    write(dir.path(), "a.md", "текст\n");
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");

    fs::write(dir.path().join("b.md"), "текст\nзаписан заново\n").expect("новый файл");
    fs::remove_file(dir.path().join("a.md")).expect("удаление старого");

    let removed = TreeChange::Removed { path: vp("a.md") };
    let created = TreeChange::Created {
        path: vp("b.md"),
        entry_kind: EntryKind::Note,
    };
    assert!(
        collector.wait_for(WAIT, |_| {
            let changes = collector.changes();
            changes.contains(&removed) && changes.contains(&created)
        }),
        "нет удаления и создания: {:?}",
        collector.payloads()
    );
    std::thread::sleep(QUIET);
    assert!(
        !collector
            .changes()
            .iter()
            .any(|c| matches!(c, TreeChange::Renamed { .. })),
        "{:?}",
        collector.payloads()
    );
}
