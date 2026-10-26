//! Журнал своих записей ядра — один на хранилище (ADR 0006, уточнение
//! 25.10.2026; задача 1.7).
//!
//! Нейтральный модуль: его пишет хранилище (`vault`) и читает наблюдатель
//! (`watcher`). Один экземпляр (дешёвый клон общего состояния) создаёт слой
//! команд при открытии хранилища и передаёт обоим: `Vault::open_with` и
//! `VaultWatcher::start_with`.
//!
//! Хранилище перед заменой файла записывает сюда путь, хэш `blake3` и размер
//! нового содержимого ([`OwnWrites::record`]): дата изменения до замены ещё
//! неизвестна. Когда наблюдатель видит изменение этого файла, он сравнивает
//! размер на диске с записанными версиями и, если размер какой-то из них
//! совпал, считает хэш файла. Совпали и размер, и хэш — на диске своя
//! версия: модель дерева обновляется молча, наружу изменение не уходит,
//! ложного конфликта нет. Любое другое содержимое — внешнее изменение.
//!
//! **Ключ пути — ключ имени каждого компонента** (`names::key`: NFC →
//! приведение регистра → NFC). Хранилище записывает путь в написании
//! интерфейса, наблюдатель спрашивает в написании с диска; на Windows они
//! могут различаться регистром или нормализацией Unicode — запись всё равно
//! находится.
//!
//! На путь хранятся несколько последних версий ([`VERSIONS_PER_PATH`]):
//! при быстрых автосохранениях следующая запись начинается раньше, чем
//! событие о предыдущей дошло до сверки. Версия живёт [`VERSION_TTL`];
//! расхождение с диском её не удаляет (чужая правка между своими записями
//! не делает следующую свою запись внешней). Когда на диске оказалась своя
//! версия, все более старые версии пути забываются: возврат к ним снаружи —
//! внешнее изменение. Весь журнал ограничен
//! [`MAX_TOTAL`] версиями — лишние, самые старые, забываются. Удаление и
//! переименование пути забывают его версии.

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

/// Путь по ключам имён компонентов.
type PathKey = Vec<String>;

fn path_key(path: &VaultPath) -> PathKey {
    path.components()
        .iter()
        .map(|c| crate::names::key(c).as_str().to_owned())
        .collect()
}

#[derive(Debug, Default)]
struct Journal {
    by_path: HashMap<PathKey, VecDeque<Version>>,
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
#[derive(Clone, Default)]
pub struct OwnWrites {
    journal: Arc<Mutex<Journal>>,
    /// Наблюдатель записей — только в тестах («записано до замены файла»).
    #[cfg(test)]
    observer: Arc<Mutex<Option<Observer>>>,
}

#[cfg(test)]
type Observer = Box<dyn Fn(&VaultPath, &str, u64) + Send>;

impl std::fmt::Debug for OwnWrites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnWrites").finish_non_exhaustive()
    }
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
        #[cfg(test)]
        if let Some(observer) = self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            observer(path, hash, size);
        }
        // Ключ — вне замка: приведение регистра не должно держать журнал.
        let key = path_key(path);
        let mut journal = self.lock();
        journal.expire(Instant::now());
        let versions = journal.by_path.entry(key).or_default();
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
    ///
    /// Размер из списка папки не используется: на Windows он обновляется с
    /// запозданием (`FindNextFile`), поэтому размер и хэш берутся из самого
    /// файла по одному дескриптору.
    pub(crate) fn is_own(&self, path: &VaultPath, file: &Path) -> bool {
        let key = path_key(path);
        let candidates: Vec<(u64, String)> = {
            let mut journal = self.lock();
            journal.expire(Instant::now());
            journal
                .by_path
                .get(&key)
                .map(|versions| versions.iter().map(|v| (v.size, v.hash.clone())).collect())
                .unwrap_or_default()
        };
        if candidates.is_empty() {
            return false;
        }
        let Ok((size, actual)) = read_version(file, &candidates) else {
            return false;
        };
        if !candidates.iter().any(|(s, h)| *s == size && *h == actual) {
            return false;
        }
        // На диске своя версия k: версии старше k уже пройдены, и возврат
        // к ним — это чужой откат, а не своя запись.
        let mut journal = self.lock();
        let mut dropped = 0;
        if let Some(versions) = journal.by_path.get_mut(&key)
            && let Some(k) = versions
                .iter()
                .position(|v| v.size == size && v.hash == actual)
        {
            versions.drain(..k);
            dropped = k;
        }
        journal.total -= dropped;
        true
    }

    /// Забывает версии по пути и всему, что под ним (путь удалён или
    /// переименован).
    pub(crate) fn forget(&self, path: &VaultPath) {
        let prefix = path_key(path);
        let mut journal = self.lock();
        journal.by_path.retain(|key, _| !key.starts_with(&prefix));
        journal.total = journal.by_path.values().map(VecDeque::len).sum();
    }

    /// Записана ли своя версия с таким хэшем и размером (без чтения
    /// файла) — для тестов хранилища.
    pub fn contains(&self, path: &VaultPath, hash: &str, size: u64) -> bool {
        let mut journal = self.lock();
        journal.expire(Instant::now());
        journal
            .by_path
            .get(&path_key(path))
            .is_some_and(|versions| {
                versions
                    .iter()
                    .any(|v| v.size == size && v.hash.eq_ignore_ascii_case(hash))
            })
    }

    /// Тестовый шов: `observer` вызывается при каждой записи, до неё.
    #[cfg(test)]
    pub(crate) fn observe(&self, observer: Observer) {
        *self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(observer);
    }

    /// Число версий по пути — для тестов.
    #[cfg(test)]
    pub(crate) fn versions(&self, path: &VaultPath) -> usize {
        let mut journal = self.lock();
        journal.expire(Instant::now());
        journal
            .by_path
            .get(&path_key(path))
            .map_or(0, VecDeque::len)
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
/// Размер и хэш `blake3` файла по одному дескриптору. Хэш считается,
/// только если размер совпал с какой-то своей версией (иначе — пустая
/// строка). Открытие повторяется: на Windows сразу после замены файл может
/// быть ненадолго занят (антивирус, индексатор) — ошибка доступа не должна
/// превращать свою запись во внешнюю.
fn read_version(file: &Path, candidates: &[(u64, String)]) -> io::Result<(u64, String)> {
    let mut last = None;
    for attempt in 0..OPEN_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(OPEN_RETRY_PAUSE);
        }
        match File::open(file) {
            Ok(mut handle) => {
                let size = handle.metadata()?.len();
                if !candidates.iter().any(|(s, _)| *s == size) {
                    return Ok((size, String::new()));
                }
                let mut hasher = blake3::Hasher::new();
                hasher.update_reader(&mut handle)?;
                return Ok((size, hasher.finalize().to_hex().to_string()));
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(err),
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("файл не открылся")))
}

/// Сколько раз пробовать открыть файл для хэша.
const OPEN_ATTEMPTS: u32 = 4;

/// Пауза между попытками открыть файл.
const OPEN_RETRY_PAUSE: Duration = Duration::from_millis(50);

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn vp(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn stale_size_from_directory_listing_does_not_hide_own_version() {
        // На Windows размер в списке папки (FindNextFile) обновляется с
        // запозданием: сверка может передать прежний размер файла.
        let dir = tempfile::tempdir().expect("временная папка");
        let file = dir.path().join("a.md");
        let text = "своя версия";
        std::fs::write(&file, text).expect("запись");
        let own = OwnWrites::new();
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        own.record(&vp("a.md"), &hash, text.len() as u64);

        // Раньше сверка передавала размер из списка папки, и при устаревшем
        // размере своя версия считалась внешней. Теперь размер — из файла.
        assert!(own.is_own(&vp("a.md"), &file));
    }

    #[test]
    fn path_is_matched_by_name_key_not_spelling() {
        let own = OwnWrites::new();
        own.record(&vp("проекты/встреча.md"), &"a".repeat(64), 3);
        assert!(own.contains(&vp("Проекты/Встреча.md"), &"a".repeat(64), 3));
        // «й» в NFD (и + U+0306) — тот же ключ имени, что «й» в NFC.
        own.record(&vp("Мой.md"), &"b".repeat(64), 1);
        assert!(own.contains(&vp("Мои\u{306}.md"), &"b".repeat(64), 1));
        assert!(!own.contains(&vp("Проекты/Встреча.md"), &"a".repeat(64), 4));
        own.forget(&vp("ПРОЕКТЫ"));
        assert!(!own.contains(&vp("проекты/встреча.md"), &"a".repeat(64), 3));
    }

    #[test]
    fn suppression_needs_both_size_and_hash() {
        let dir = tempfile::tempdir().expect("папка");
        let file = dir.path().join("a.md");
        std::fs::write(&file, "своё").expect("запись");
        let own = OwnWrites::new();
        let hash = blake3::hash("своё".as_bytes()).to_hex().to_string();
        own.record(&vp("A.md"), &hash, "своё".len() as u64);
        assert!(own.is_own(&vp("a.md"), &file));
        std::fs::write(&file, "чужо").expect("запись");
        assert!(
            !own.is_own(&vp("a.md"), &file),
            "тот же размер, другой хэш — внешнее изменение"
        );
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
