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
