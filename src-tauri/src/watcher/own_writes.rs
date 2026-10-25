//! Журнал своих записей ядра (ADR 0006, уточнение 25.10.2026).
//!
//! Модуль `vault` перед заменой файла заметки записывает сюда путь, хэш
//! `blake3` и размер нового содержимого ([`OwnWrites::record`]): дата
//! изменения до замены ещё неизвестна. Когда наблюдатель видит изменение
//! этого файла, он сравнивает размер на диске с записанными версиями и, если
//! размер какой-то из них совпал, считает хэш файла. Совпал хэш — на диске
//! своя версия: модель дерева обновляется молча, наружу изменение не
//! уходит, ложного конфликта нет. Любое другое содержимое — внешнее
//! изменение.
//!
//! На путь хранятся несколько последних версий ([`VERSIONS_PER_PATH`]):
//! при быстрых автосохранениях следующая запись начинается раньше, чем
//! событие о предыдущей дошло до сверки. Версия живёт [`VERSION_TTL`];
//! расхождение с диском её не удаляет (чужая правка между своими записями
//! не делает следующую свою запись внешней). Весь журнал ограничен
//! [`MAX_TOTAL`] версиями — лишние, самые старые, забываются. Удаление и
//! переименование пути забывают его версии.
//!
//! Журнал — дешёвый клон общего состояния: один экземпляр у наблюдателя и у
//! хранилища; связывает их слой команд (задача 1.7).

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::commands::VaultPath;

/// Сколько последних своих версий помнится на путь.
pub(crate) const VERSIONS_PER_PATH: usize = 4;

/// Сколько живёт запись о своей версии.
pub(crate) const VERSION_TTL: Duration = Duration::from_secs(120);

/// Общий предел версий в журнале.
pub(crate) const MAX_TOTAL: usize = 4096;

/// Своя версия файла.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    /// Хэш `blake3`, 64 шестнадцатеричных символа в нижнем регистре.
    hash: String,
    /// Размер в байтах.
    size: u64,
    /// Когда записана.
    at: Instant,
}

#[derive(Debug, Default)]
struct Journal {
    by_path: HashMap<VaultPath, VecDeque<Version>>,
    total: usize,
}

impl Journal {
    fn expire(&mut self, now: Instant) {
        let mut total = 0;
        self.by_path.retain(|_, versions| {
            versions.retain(|v| now.saturating_duration_since(v.at) < VERSION_TTL);
            total += versions.len();
            !versions.is_empty()
        });
        self.total = total;
    }

    /// Убирает самую старую версию во всём журнале.
    fn evict_oldest(&mut self) {
        let oldest = self
            .by_path
            .iter()
            .filter_map(|(path, versions)| versions.front().map(|v| (v.at, path.clone())))
            .min_by_key(|(at, _)| *at);
        if let Some((_, path)) = oldest
            && let Some(versions) = self.by_path.get_mut(&path)
        {
            versions.pop_front();
            self.total -= 1;
            if versions.is_empty() {
                self.by_path.remove(&path);
            }
        }
    }
}

/// Журнал своих записей «путь → последние версии (хэш, размер)». Клон
/// указывает на тот же журнал.
#[derive(Debug, Clone, Default)]
pub struct OwnWrites {
    journal: Arc<Mutex<Journal>>,
}

impl OwnWrites {
    /// Пустой журнал.
    pub fn new() -> Self {
        Self::default()
    }

    /// Запоминает свою запись **до** замены файла: путь, хэш `blake3` нового
    /// содержимого (как в `FileVersion::hash`) и его размер.
    pub fn record(&self, path: &VaultPath, hash: &str, size: u64) {
        self.record_at(path, hash, size, Instant::now());
    }

    pub(crate) fn record_at(&self, path: &VaultPath, hash: &str, size: u64, at: Instant) {
        let mut journal = self.lock();
        journal.expire(Instant::now());
        let versions = journal.by_path.entry(path.clone()).or_default();
        versions.push_back(Version {
            hash: hash.to_ascii_lowercase(),
            size,
            at,
        });
        let dropped = versions.len().saturating_sub(VERSIONS_PER_PATH);
        versions.drain(..dropped);
        journal.total = journal.total + 1 - dropped;
        journal.expire(Instant::now());
        while journal.total > MAX_TOTAL {
            journal.evict_oldest();
        }
    }

    /// На диске (`file` размером `size`) побайтно одна из своих версий
    /// пути? Хэш считается, только если размер совпал с какой-то версией.
    pub(crate) fn is_own(&self, path: &VaultPath, file: &Path, size: u64) -> bool {
        let candidates: Vec<String> = {
            let mut journal = self.lock();
            journal.expire(Instant::now());
            journal
                .by_path
                .get(path)
                .map(|versions| {
                    versions
                        .iter()
                        .filter(|v| v.size == size)
                        .map(|v| v.hash.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        if candidates.is_empty() {
            return false;
        }
        hash_file(file).is_ok_and(|actual| candidates.contains(&actual))
    }

    /// Забывает версии по пути и всему, что под ним (путь удалён или
    /// переименован).
    pub(crate) fn forget(&self, path: &VaultPath) {
        let prefix = path.components();
        let mut journal = self.lock();
        journal
            .by_path
            .retain(|key, _| !key.components().starts_with(prefix));
        journal.total = journal.by_path.values().map(VecDeque::len).sum();
    }

    /// Число версий по пути — для тестов.
    #[cfg(test)]
    pub(crate) fn versions(&self, path: &VaultPath) -> usize {
        let mut journal = self.lock();
        journal.expire(Instant::now());
        journal.by_path.get(path).map_or(0, VecDeque::len)
    }

    /// Число версий во всём журнале — для тестов.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().total
    }

    fn lock(&self) -> MutexGuard<'_, Journal> {
        // Журнал — простая таблица: после паники в другом потоке она цела.
        self.journal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Хэш `blake3` содержимого файла в шестнадцатеричном виде.
fn hash_file(file: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(file)?)?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn vp(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn keeps_at_most_four_versions_per_path() {
        let own = OwnWrites::new();
        for n in 0..6 {
            own.record(&vp("a.md"), &format!("{n:064x}"), n);
        }
        assert_eq!(own.versions(&vp("a.md")), 4);
    }

    #[test]
    fn versions_expire() {
        let own = OwnWrites::new();
        own.record_at(
            &vp("a.md"),
            &"0".repeat(64),
            1,
            Instant::now() - Duration::from_secs(121),
        );
        own.record(&vp("b.md"), &"1".repeat(64), 1);
        assert_eq!(own.versions(&vp("a.md")), 0, "запись старше 120 с забыта");
        assert_eq!(own.versions(&vp("b.md")), 1);
    }

    #[test]
    fn total_number_of_versions_is_bounded() {
        let own = OwnWrites::new();
        for n in 0..(MAX_TOTAL + 100) {
            own.record(&vp(&format!("{n}.md")), &"0".repeat(64), 1);
        }
        assert!(own.len() <= MAX_TOTAL);
        assert_eq!(
            own.versions(&vp(&format!("{}.md", MAX_TOTAL + 99))),
            1,
            "новые остаются"
        );
    }
}
