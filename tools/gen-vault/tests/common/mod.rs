//! Общие помощники тестов генератора: временная папка и обход хранилища.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Временная папка, удаляется при выходе из области видимости.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("gen-vault-test-{}-{tag}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Все файлы хранилища: относительный путь через `/` → содержимое.
pub fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            walk(root, &path, out);
        } else {
            out.insert(rel(root, &path), fs::read(&path).unwrap());
        }
    }
}

/// Все папки хранилища (относительные пути через `/`), без корня.
pub fn read_dirs(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_dirs(root, root, &mut out);
    out.sort();
    out
}

fn walk_dirs(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            out.push(rel(root, &entry.path()));
            walk_dirs(root, &entry.path(), out);
        }
    }
}

pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_str().expect("имя не UTF-8").to_string())
        .collect::<Vec<_>>()
        .join("/")
}
