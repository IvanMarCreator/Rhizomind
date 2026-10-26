//! ВРЕМЕННО (задача 0.10, диагностика): кто мешает переносу папки на Windows.
//! Удаляется после диагностики.

use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use super::VaultWatcher;
use super::test_support::Collector;

const BACKUP: u32 = 0x0200_0000; // FILE_FLAG_BACKUP_SEMANTICS

fn try_rename(from: &Path, to: &Path) -> String {
    match fs::rename(from, to) {
        Ok(()) => {
            fs::rename(to, from).expect("вернуть");
            "OK".into()
        }
        Err(e) => format!("ОШИБКА {e}"),
    }
}

fn layout(root: &Path) {
    fs::create_dir_all(root.join("a/b")).unwrap();
    fs::write(root.join("a/f.md"), "x").unwrap();
    fs::write(root.join("a/b/g.md"), "y").unwrap();
}

#[test]
#[ignore = "диагностика 0.10"]
fn diag_handles_block_folder_rename() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    layout(root);
    let a = root.join("a");
    let a2 = root.join("a2");

    eprintln!("[diag] без дескрипторов: {}", try_rename(&a, &a2));
    {
        let _f = fs::File::open(a.join("f.md")).unwrap();
        eprintln!(
            "[diag] файл внутри открыт File::open (RWD): {}",
            try_rename(&a, &a2)
        );
    }
    {
        let _f = fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(BACKUP)
            .open(a.join("f.md"))
            .unwrap();
        eprintln!(
            "[diag] файл внутри открыт как file-id (access 0): {}",
            try_rename(&a, &a2)
        );
    }
    {
        let _f = fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(BACKUP)
            .open(a.join("b"))
            .unwrap();
        eprintln!(
            "[diag] подпапка b открыта как file-id: {}",
            try_rename(&a, &a2)
        );
    }
    {
        let _f = fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(BACKUP)
            .open(&a)
            .unwrap();
        eprintln!(
            "[diag] сама папка a открыта как file-id: {}",
            try_rename(&a, &a2)
        );
    }
    {
        let mut it = fs::read_dir(&a).unwrap();
        let _first = it.next();
        eprintln!("[diag] read_dir(a) идёт: {}", try_rename(&a, &a2));
    }
    {
        let mut it = fs::read_dir(a.join("b")).unwrap();
        let _first = it.next();
        eprintln!("[diag] read_dir(a/b) идёт: {}", try_rename(&a, &a2));
    }
    {
        let _m = fs::metadata(a.join("f.md")).unwrap();
        eprintln!("[diag] после metadata (закрыт): {}", try_rename(&a, &a2));
    }
    {
        let collector = Collector::default();
        let _w = VaultWatcher::start(root, collector.sink()).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        eprintln!(
            "[diag] VaultWatcher на корне, затих: подпапка a: {}",
            try_rename(&a, &a2)
        );
        std::thread::sleep(Duration::from_secs(2));
        let other = dir.path().with_extension("moved");
        eprintln!(
            "[diag] VaultWatcher на корне, затих: сам корень: {}",
            try_rename(root, &other)
        );
    }
}

fn storm_once(root: &Path, watch: bool) -> (usize, usize) {
    let out = root.join("g");
    gen_vault::generate(&out, &gen_vault::Config::new(3000, 7)).unwrap();
    let collector = Collector::default();
    let watcher = watch.then(|| VaultWatcher::start(root, collector.sink()).unwrap());
    let target = out.join("Перенесённые");
    fs::create_dir(&target).unwrap();
    let mut notes = Vec::new();
    let mut stack = vec![out.clone()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                if p != target {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "md") {
                notes.push(p);
            }
        }
    }
    notes.sort();
    for (n, note) in notes.iter().enumerate().filter(|(n, _)| n % 5 == 0) {
        fs::rename(note, target.join(format!("п {n}.md"))).unwrap();
    }
    let mut folders: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && *p != target)
        .collect();
    folders.sort();
    let mut failed = 0;
    let mut tried = 0;
    for (n, folder) in folders.iter().take(6).enumerate() {
        tried += 1;
        if let Err(e) = fs::rename(folder, out.join(format!("П {n}"))) {
            failed += 1;
            eprintln!("[diag]   перенос папки {n}: {e}");
        }
    }
    drop(watcher);
    (tried, failed)
}

#[test]
#[ignore = "диагностика 0.10"]
fn diag_folder_rename_storm() {
    let rounds: usize = std::env::var("T010_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    for watch in [false, true, false, true] {
        let started = Instant::now();
        let (mut tried, mut failed) = (0, 0);
        for _ in 0..rounds {
            let dir = tempfile::tempdir().unwrap();
            let (t, f) = storm_once(dir.path(), watch);
            tried += t;
            failed += f;
        }
        eprintln!(
            "[diag] буря переносов, наблюдатель {watch}: неудач {failed} из {tried} ({:.0} с)",
            started.elapsed().as_secs_f64()
        );
    }
}
