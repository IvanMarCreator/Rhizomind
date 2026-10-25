//! Общие средства тестов наблюдателя: сборщик пачек, «зеркало» дерева,
//! которое строится только из событий (как его видит интерфейс), и
//! независимый обход диска для сравнения.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{ChangeSink, WatcherEvent};
use crate::commands::{EntryKind, TreeChange, VaultChangedPayload, VaultPath};

pub(crate) fn vp(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

/// Дерево «путь → вид» с диска: всё, кроме имён с точки в начале и ссылок.
/// Отдельная от наблюдателя реализация — источник истины для сравнения.
pub(crate) fn disk_tree(root: &Path) -> BTreeMap<Vec<String>, EntryKind> {
    fn walk(dir: &Path, prefix: &[String], out: &mut BTreeMap<Vec<String>, EntryKind>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let mut key = prefix.to_vec();
            key.push(name.clone());
            if file_type.is_symlink() {
                continue;
            } else if file_type.is_dir() {
                walk(&entry.path(), &key, out);
                out.insert(key, EntryKind::Folder);
            } else if name.to_lowercase().ends_with(".md") && name.len() > 3 {
                out.insert(key, EntryKind::Note);
            } else {
                out.insert(key, EntryKind::Attachment);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &[], &mut out);
    out
}

/// Получатель пачек для тестов.
#[derive(Clone, Default)]
pub(crate) struct Collector {
    inner: Arc<Mutex<Received>>,
}

#[derive(Default)]
struct Received {
    payloads: Vec<VaultChangedPayload>,
    signals: Vec<WatcherEvent>,
    last: Option<Instant>,
}

impl Collector {
    pub(crate) fn sink(&self) -> ChangeSink {
        let inner = self.inner.clone();
        Box::new(move |event| {
            let mut received = inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match event {
                WatcherEvent::Changed(payload) => received.payloads.push(payload),
                other => received.signals.push(other),
            }
            received.last = Some(Instant::now());
        })
    }

    /// Все пачки, полученные до сих пор.
    pub(crate) fn payloads(&self) -> Vec<VaultChangedPayload> {
        self.inner.lock().expect("сборщик").payloads.clone()
    }

    /// Сигналы наблюдателя (кроме пачек изменений). На Windows тест с
    /// сигналами не идёт: папку под наблюдением там не переименовать.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn signals(&self) -> Vec<WatcherEvent> {
        self.inner.lock().expect("сборщик").signals.clone()
    }

    /// Все изменения подряд.
    pub(crate) fn changes(&self) -> Vec<TreeChange> {
        self.payloads()
            .into_iter()
            .flat_map(|payload| payload.changes)
            .collect()
    }

    fn quiet_for(&self) -> Duration {
        self.inner
            .lock()
            .expect("сборщик")
            .last
            .map_or(Duration::MAX, |last| last.elapsed())
    }

    /// Ждёт, пока `done` не станет истинным для полученных изменений.
    pub(crate) fn wait_for(
        &self,
        timeout: Duration,
        mut done: impl FnMut(&[VaultChangedPayload]) -> bool,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if done(&self.payloads()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        done(&self.payloads())
    }

    /// Ждёт затихания: зеркало из событий совпало с диском и новых пачек нет
    /// `quiet`. Возвращает зеркало и время до совпадения.
    pub(crate) fn settle(
        &self,
        mirror: &mut Mirror,
        root: &Path,
        quiet: Duration,
        timeout: Duration,
    ) -> Result<Duration, String> {
        let start = Instant::now();
        loop {
            let payloads = self.payloads();
            for payload in &payloads[mirror.applied..] {
                mirror.apply(payload);
            }
            mirror.applied = payloads.len();
            // Тишина считается и от начала ожидания: изменения содержимого
            // дерево не меняют, и до первой пачки зеркало уже «совпадает».
            if self.quiet_for().min(start.elapsed()) >= quiet && mirror.tree == disk_tree(root) {
                return Ok(start.elapsed());
            }
            if start.elapsed() > timeout {
                let disk = disk_tree(root);
                let missing: Vec<_> = disk
                    .keys()
                    .filter(|k| !mirror.tree.contains_key(*k))
                    .take(5)
                    .collect();
                let extra: Vec<_> = mirror
                    .tree
                    .keys()
                    .filter(|k| !disk.contains_key(*k))
                    .take(5)
                    .collect();
                return Err(format!(
                    "за {timeout:?} зеркало не совпало с диском: в зеркале {} элементов, на диске {}; \
                     нет в зеркале: {missing:?}; лишние: {extra:?}",
                    mirror.tree.len(),
                    disk.len()
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Дерево, которое строится только из событий `vault:changed`, — так его
/// поддерживает интерфейс.
pub(crate) struct Mirror {
    pub(crate) tree: BTreeMap<Vec<String>, EntryKind>,
    /// Сколько пачек сборщика уже применено.
    applied: usize,
}

impl Mirror {
    pub(crate) fn from_disk(root: &Path) -> Self {
        Self {
            tree: disk_tree(root),
            applied: 0,
        }
    }

    /// Применяет изменения строго по порядку, как интерфейс: родитель
    /// появившегося уже есть, переименуемое и удаляемое существует, места
    /// назначения нет. Иначе — паника с описанием.
    pub(crate) fn apply(&mut self, payload: &VaultChangedPayload) {
        for change in &payload.changes {
            match change {
                TreeChange::Created { path, entry_kind } => {
                    let key = path.components().to_vec();
                    self.expect_parent(&key, change);
                    assert!(
                        self.tree.insert(key, *entry_kind).is_none(),
                        "появился уже существующий: {change:?}"
                    );
                }
                TreeChange::Removed { path } => {
                    let key = path.components();
                    assert!(
                        self.tree.contains_key(key),
                        "исчез несуществующий: {change:?}"
                    );
                    self.tree.retain(|k, _| !k.starts_with(key));
                }
                TreeChange::Renamed { from, to } => {
                    let from = from.components();
                    assert!(
                        self.tree.contains_key(from),
                        "переименован несуществующий: {change:?}"
                    );
                    let to_key = to.components().to_vec();
                    assert!(
                        !self.tree.contains_key(&to_key) || to_key.starts_with(from),
                        "место назначения занято: {change:?}"
                    );
                    self.expect_parent(&to_key, change);
                    let moved: Vec<_> = self
                        .tree
                        .iter()
                        .filter(|(k, _)| k.starts_with(from))
                        .map(|(k, v)| (k.clone(), *v))
                        .collect();
                    for (key, _) in &moved {
                        self.tree.remove(key);
                    }
                    for (key, kind) in moved {
                        let mut new_key = to_key.clone();
                        new_key.extend_from_slice(&key[from.len()..]);
                        self.tree.insert(new_key, kind);
                    }
                }
                TreeChange::Modified { path } => {
                    assert!(
                        self.tree.contains_key(path.components()),
                        "изменён несуществующий: {change:?}"
                    );
                }
            }
        }
    }

    fn expect_parent(&self, key: &[String], change: &TreeChange) {
        if key.len() > 1 {
            assert_eq!(
                self.tree.get(&key[..key.len() - 1]),
                Some(&EntryKind::Folder),
                "нет папки-родителя: {change:?}"
            );
        }
    }
}
