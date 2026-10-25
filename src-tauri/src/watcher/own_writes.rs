//! Журнал своих записей ядра (ADR 0006, уточнение 25.10.2026).
//!
//! Модуль `vault` перед заменой файла заметки записывает сюда путь, хэш
//! `blake3` и размер нового содержимого ([`OwnWrites::record`]): дата
//! изменения до замены ещё неизвестна. Когда наблюдатель видит изменение
//! этого файла, он сравнивает размер на диске с записанным и, если размер
//! совпал, считает хэш файла. Совпали оба — это своя запись: модель дерева
//! обновляется молча, наружу изменение не уходит, ложного конфликта нет.
//!
//! Запись живёт, пока файл на диске равен записанному: повторные события ОС
//! об одной замене (данные, атрибуты) тоже не становятся внешними. Первое же
//! расхождение (файл изменили снаружи) или удаление пути убирает запись.
//! Запись, после которой замена не состоялась, безвредна: внешним изменением
//! считается только содержимое, отличное от записанного.
//!
//! Журнал — дешёвый клон общего состояния: один экземпляр у наблюдателя и у
//! хранилища; связывает их слой команд (задача 1.7).

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::commands::VaultPath;

/// Ожидаемое содержимое файла после своей записи.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Expected {
    /// Хэш `blake3`, 64 шестнадцатеричных символа в нижнем регистре.
    hash: String,
    /// Размер в байтах.
    size: u64,
}

/// Общий предел записей журнала.
pub(crate) const MAX_TOTAL: usize = 4096;

/// Журнал своих записей «путь → (хэш, размер)». Клон указывает на тот же
/// журнал.
#[derive(Debug, Clone, Default)]
pub struct OwnWrites {
    entries: Arc<Mutex<HashMap<VaultPath, Expected>>>,
}

impl OwnWrites {
    /// Пустой журнал.
    pub fn new() -> Self {
        Self::default()
    }

    /// Запоминает свою запись **до** замены файла: путь, хэш `blake3` нового
    /// содержимого (как в `FileVersion::hash`) и его размер. Новая запись
    /// по тому же пути заменяет прежнюю.
    pub fn record(&self, path: &VaultPath, hash: &str, size: u64) {
        self.lock().insert(
            path.clone(),
            Expected {
                hash: hash.to_ascii_lowercase(),
                size,
            },
        );
    }

    /// Файл `file` с размером `size` на диске — своя запись по пути `path`?
    /// Хэш считается, только если размер совпал. Расхождение убирает запись.
    pub(crate) fn is_own(&self, path: &VaultPath, file: &Path, size: u64) -> bool {
        let Some(expected) = self.lock().get(path).cloned() else {
            return false;
        };
        let own =
            expected.size == size && hash_file(file).is_ok_and(|actual| actual == expected.hash);
        if !own {
            let mut entries = self.lock();
            // Пока считали хэш, ядро могло записать новую версию — её не трогаем.
            if entries.get(path) == Some(&expected) {
                entries.remove(path);
            }
        }
        own
    }

    /// Забывает записи по пути и всему, что под ним (путь удалён или
    /// переименован).
    pub(crate) fn forget(&self, path: &VaultPath) {
        let prefix = path.components();
        self.lock()
            .retain(|key, _| !key.components().starts_with(prefix));
    }

    /// Число версий по пути — для тестов.
    #[cfg(test)]
    pub(crate) fn versions(&self, path: &VaultPath) -> usize {
        usize::from(self.lock().contains_key(path))
    }

    /// Запись с заданным временем — для тестов.
    #[cfg(test)]
    pub(crate) fn record_at(
        &self,
        path: &VaultPath,
        hash: &str,
        size: u64,
        _at: std::time::Instant,
    ) {
        self.record(path, hash, size);
    }

    /// Число записей — для тестов.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<VaultPath, Expected>> {
        // Журнал — простая таблица: после паники в другом потоке она цела.
        self.entries
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
