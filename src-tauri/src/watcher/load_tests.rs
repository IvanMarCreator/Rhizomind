//! Нагрузочные тесты наблюдателя (ADR 0006, спецификация 0.1, раздел 8).
//!
//! Критерий: после затихания событий дерево, построенное только из пачек
//! `vault:changed` (как его держит интерфейс), совпадает с диском; свои
//! записи не становятся внешними изменениями.
//!
//! Идут отдельным шагом CI с таймаутом:
//! `cargo test -p rhizomind --release --lib watcher::load_tests -- --ignored --nocapture --test-threads=1`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::VaultWatcher;
use super::test_support::{Collector, Mirror, disk_tree};
use crate::commands::{FileVersion, TreeChange, VaultPath};

const FILES: usize = 10_000;
const QUIET: Duration = Duration::from_millis(1500);
const TIMEOUT: Duration = Duration::from_secs(240);

fn generate(root: &Path, notes: usize) -> PathBuf {
    let out = root.join("Сгенерировано");
    gen_vault::generate(&out, &gen_vault::Config::new(notes, 7)).expect("генератор хранилища");
    out
}

/// Путь файла внутри хранилища — компонентами, как в событиях.
fn rel(root: &Path, path: &Path) -> Vec<String> {
    path.strip_prefix(root)
        .expect("внутри")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}

fn notes_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("папка").flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().expect("вид").is_dir() {
                stack.push(path);
            } else if name.ends_with(".md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Сколько раз подряд перенос папки может получить «папка занята» (задача 0.10б).
const MAX_BUSY_RETRIES: usize = 20;

/// Перенос папки, как его делает пользователь в Проводнике с кнопкой
/// «Повторить». На Windows папку нельзя перенести, пока открыто что-то внутри
/// неё (`Access is denied`, os error 5) — в том числе пока наблюдатель читает
/// список её вложенной папки (одно чтение на папку, задача 0.10:
/// `listing_windows.rs`), а также индексатор поиска или антивирус. Повтор —
/// только на эту ошибку, через 50 мс, не больше [`MAX_BUSY_RETRIES`] раз: папка,
/// занятая дольше секунды, — уже не мгновение чтения списка, а ошибка (например,
/// наблюдатель снова держит открытыми файлы внутри). Возвращает число повторов.
fn rename_like_explorer(from: &Path, to: &Path) -> usize {
    let mut retries = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return retries,
            Err(err)
                if cfg!(windows)
                    && err.kind() == std::io::ErrorKind::PermissionDenied
                    && retries < MAX_BUSY_RETRIES =>
            {
                retries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => panic!(
                "перенос папки {}: {err} (повторов: {retries})",
                from.display()
            ),
        }
    }
}

fn report(name: &str, started: Instant, settled: Duration, collector: &Collector) {
    let payloads = collector.payloads();
    for (n, p) in payloads.iter().enumerate().take(5) {
        let count = |f: fn(&TreeChange) -> bool| p.changes.iter().filter(|c| f(c)).count();
        eprintln!(
            "  пачка {n}: rescan {}, переименований {}, появлений {}, исчезновений {}, изменений {}",
            p.rescan,
            count(|c| matches!(c, TreeChange::Renamed { .. })),
            count(|c| matches!(c, TreeChange::Created { .. })),
            count(|c| matches!(c, TreeChange::Removed { .. })),
            count(|c| matches!(c, TreeChange::Modified { .. })),
        );
    }
    let rescans = payloads.iter().filter(|p| p.rescan).count();
    let changes: usize = payloads.iter().map(|p| p.changes.len()).sum();
    eprintln!(
        "[нагрузка] {name}: всего {:.1} с, затихание и совпадение {:.1} с; пачек {}, из них rescan {rescans}; изменений {changes}",
        started.elapsed().as_secs_f64(),
        settled.as_secs_f64(),
        payloads.len(),
    );
}

#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn ten_thousand_files_created_then_modified_in_one_go() {
    let dir = tempfile::tempdir().expect("временная папка");
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    let started = Instant::now();

    let out = generate(dir.path(), FILES);
    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("после создания 10 000 файлов дерево совпадает с диском");
    report("создание 10 000", started, settled, &collector);
    assert!(mirror.tree.len() > FILES);

    let started = Instant::now();
    // Изменения считаются только с этого места: при создании файл мог
    // попасть в сверку пустым (появился, потом записан) — это законное
    // `Modified` той фазы, а не правки ниже.
    let before = collector.payloads().len();
    let notes = notes_under(&out);
    for note in &notes {
        let mut text = fs::read(note).expect("чтение");
        text.extend_from_slice("\nдописано\n".as_bytes());
        fs::write(note, text).expect("запись");
    }
    let expected: std::collections::HashSet<Vec<String>> =
        notes.iter().map(|p| rel(dir.path(), p)).collect();
    collector
        .wait_modified(&expected, before, TIMEOUT)
        .expect("правка каждой заметки дошла");
    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("после изменения всех заметок дерево совпадает с диском");
    report("изменение всех заметок", started, settled, &collector);
    let modified: std::collections::HashSet<Vec<String>> = collector.payloads()[before..]
        .iter()
        .flat_map(|p| p.changes.iter())
        .filter_map(|c| match c {
            TreeChange::Modified { path } => Some(path.components().to_vec()),
            _ => None,
        })
        .collect();
    let missing: Vec<_> = expected.difference(&modified).take(3).collect();
    let extra: Vec<_> = modified.difference(&expected).take(3).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "каждая изменённая заметка — изменение, и только они: нет {missing:?}, лишние {extra:?}"
    );
}

#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn mass_replace_like_git_checkout() {
    let dir = tempfile::tempdir().expect("временная папка");
    let out = generate(dir.path(), FILES);
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    let started = Instant::now();

    // Как git checkout другой ветки: файл удаляется и создаётся заново,
    // часть файлов исчезает, появляются новые, в том числе в новых папках.
    let notes = notes_under(&out);
    for (n, note) in notes.iter().enumerate() {
        fs::remove_file(note).expect("удаление");
        match n % 10 {
            0 => {}
            1 => {
                let new = out.join(format!("ветка/{}/новая {n}.md", n % 37));
                fs::create_dir_all(new.parent().expect("родитель")).expect("папка");
                fs::write(new, format!("# Новая {n}\n")).expect("запись");
            }
            _ => fs::write(note, format!("# Версия из другой ветки {n}\n")).expect("запись"),
        }
    }
    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("после массовой замены дерево совпадает с диском");
    report("массовая замена", started, settled, &collector);
}

#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn renames_of_files_and_folders() {
    let dir = tempfile::tempdir().expect("временная папка");
    let out = generate(dir.path(), FILES);
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let _watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    let started = Instant::now();

    let notes = notes_under(&out);
    let target = out.join("Перенесённые");
    fs::create_dir(&target).expect("папка");
    let mut renamed = Vec::new();
    for (n, note) in notes.iter().enumerate().filter(|(n, _)| n % 5 == 0) {
        let new = target.join(format!("перенесённая {n}.md"));
        fs::rename(note, &new).expect("перенос");
        renamed.push(note.clone());
    }
    // Папки целиком.
    let mut folders: Vec<PathBuf> = fs::read_dir(&out)
        .expect("папка")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && *p != target)
        .collect();
    folders.sort();
    let mut busy = 0;
    for (n, folder) in folders.iter().take(3).enumerate() {
        let new = out.join(format!("Переименованная папка {n}"));
        busy += rename_like_explorer(folder, &new);
    }
    eprintln!("  перенос папок: повторов из-за «папка занята» — {busy}");

    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("после переименований дерево совпадает с диском");
    report("переименования", started, settled, &collector);

    let renames = collector
        .changes()
        .into_iter()
        .filter(|c| matches!(c, TreeChange::Renamed { .. }))
        .count();
    let others: Vec<_> = collector
        .changes()
        .into_iter()
        .filter(|c| !matches!(c, TreeChange::Renamed { .. }))
        .collect();
    eprintln!("  кроме переименований: {others:?}");
    assert!(
        renames >= renamed.len(),
        "переименования распознаны: {renames} из {} файлов",
        renamed.len()
    );
}

#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn overflow_rescan_restores_tree() {
    let dir = tempfile::tempdir().expect("временная папка");
    let out = generate(dir.path(), FILES);
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    let started = Instant::now();

    // Переполнение буфера ОС надёжно не вызвать: события теряются через
    // тестовый шов, а потом приходит Rescan — как от notify.
    watcher.lose_os_events(true);
    for (n, note) in notes_under(&out).iter().enumerate() {
        match n % 4 {
            0 => fs::remove_file(note).expect("удаление"),
            1 => fs::write(note, "изменено").expect("запись"),
            2 => fs::rename(note, note.with_extension("перенесено.md")).expect("перенос"),
            _ => {}
        }
    }
    fs::create_dir_all(out.join("новая/вложенная")).expect("папка");
    fs::write(out.join("новая/вложенная/x.md"), "x").expect("запись");
    std::thread::sleep(QUIET);
    watcher.lose_os_events(false);
    watcher.rescan();

    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("после Rescan дерево совпадает с диском");
    report("переполнение и Rescan", started, settled, &collector);
    assert!(collector.payloads().iter().any(|p| p.rescan));
}

#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn own_writes_during_external_storm_are_not_external() {
    let dir = tempfile::tempdir().expect("временная папка");
    let out = generate(dir.path(), FILES);
    let notes = notes_under(&out);
    let (own, external): (Vec<_>, Vec<_>) =
        notes.iter().enumerate().partition(|(n, _)| n % 10 == 0);
    let own: Vec<PathBuf> = own.into_iter().map(|(_, p)| p.clone()).collect();
    let external: Vec<PathBuf> = external.into_iter().map(|(_, p)| p.clone()).collect();
    let mut mirror = Mirror::from_disk(dir.path());
    let collector = Collector::default();
    let watcher = VaultWatcher::start(dir.path(), collector.sink()).expect("слежение");
    let journal = watcher.own_writes();
    let started = Instant::now();

    let root = dir.path().to_path_buf();
    let own_paths = own.clone();
    let writer = std::thread::spawn(move || {
        let tmp_dir = root.join(".rhizomind/tmp");
        fs::create_dir_all(&tmp_dir).expect("папка");
        for (n, path) in own_paths.iter().enumerate() {
            let text = format!("# Своя запись {n}\n");
            let vault_path = VaultPath::from_components(
                path.strip_prefix(&root)
                    .expect("внутри")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            );
            let version = FileVersion {
                hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
                size: text.len() as u64,
                modified_ms: 0,
            };
            // Как хранилище: журнал до замены, потом временный файл → замена.
            journal.record(&vault_path, &version.hash, version.size);
            let tmp = tmp_dir.join(format!("{n}.tmp"));
            fs::write(&tmp, &text).expect("временный файл");
            fs::rename(&tmp, path).expect("замена");
        }
    });
    for path in &external {
        fs::write(path, "внешняя правка").expect("запись");
    }
    writer.join().expect("поток своих записей");
    let external_paths: std::collections::HashSet<Vec<String>> =
        external.iter().map(|p| rel(dir.path(), p)).collect();
    collector
        .wait_modified(&external_paths, 0, TIMEOUT)
        .expect("внешние правки дошли");

    let settled = collector
        .settle(&mut mirror, dir.path(), QUIET, TIMEOUT)
        .expect("дерево совпадает с диском");
    report("свои записи под нагрузкой", started, settled, &collector);

    let own_set: std::collections::HashSet<Vec<String>> = own
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .expect("внутри")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .collect();
    let false_external: Vec<_> = collector
        .changes()
        .into_iter()
        .filter(|c| match c {
            TreeChange::Modified { path }
            | TreeChange::Removed { path }
            | TreeChange::Created { path, .. } => own_set.contains(path.components()),
            TreeChange::Renamed { from, to } => {
                own_set.contains(from.components()) || own_set.contains(to.components())
            }
        })
        .collect();
    assert!(
        false_external.is_empty(),
        "свои записи ушли как внешние: {} шт., например {:?}",
        false_external.len(),
        false_external.first()
    );
    let modified = collector
        .changes()
        .into_iter()
        .filter(|c| matches!(c, TreeChange::Modified { .. }))
        .count();
    assert!(modified > 0, "внешние правки дошли");
    assert_eq!(mirror.tree, disk_tree(dir.path()));
}

/// Ревью 1.4, Б1: точка соединения NTFS на внешнюю папку не обходится ни
/// моделью, ни кэшем склейки (`FileIdMap` ходил по ссылкам и открывал
/// каждый файл). Кэша нет — запуск не зависит от размера внешней папки.
#[cfg(windows)]
#[test]
#[ignore = "нагрузочный: отдельный шаг CI"]
fn junction_to_outside_folder_is_not_walked() {
    const OUTSIDE_FILES: usize = 30_000;
    let outside = tempfile::tempdir().expect("внешняя папка");
    for n in 0..OUTSIDE_FILES {
        fs::write(outside.path().join(format!("{n}.md")), "").expect("файл");
    }
    let control = tempfile::tempdir().expect("хранилище без ссылки");
    let vault = tempfile::tempdir().expect("хранилище со ссылкой");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(vault.path().join("ссылка"))
        .arg(outside.path())
        .status()
        .expect("mklink");
    assert!(status.success(), "точка соединения создана");

    let started = Instant::now();
    drop(VaultWatcher::start(control.path(), Collector::default().sink()).expect("слежение"));
    let without = started.elapsed();
    let started = Instant::now();
    drop(VaultWatcher::start(vault.path(), Collector::default().sink()).expect("слежение"));
    let with = started.elapsed();

    eprintln!(
        "[нагрузка] запуск: без ссылки {without:?}, с точкой соединения на {OUTSIDE_FILES} файлов {with:?}"
    );
    assert!(
        with < without + Duration::from_millis(700),
        "запуск со ссылкой {with:?} против {without:?}: внешняя папка обходится"
    );
}
