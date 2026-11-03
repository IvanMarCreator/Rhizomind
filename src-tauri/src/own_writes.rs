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
//!
//! **Свои переименования и переносы** (задача 0.2-4.2, спецификация 0.2,
//! С2-5.1, Р10). Хранилище **до** переименования или переноса на диске
//! записывает пару «откуда → куда» ([`OwnWrites::record_rename`]), а при
//! ошибке действия убирает её ([`OwnWrites::forget_rename`]). Наблюдатель и
//! обработка внешних переименований спрашивают
//! [`OwnWrites::is_own_rename`]: событие наблюдателя о своём переименовании
//! узнаётся как своё, даже если пришло раньше ответа команды. Ключ — те же
//! ключи имён компонентов (регистр и нормализация Unicode не важны).
//! Переименование папки покрывает вложенные пути: «Папка → Архив» делает
//! своим и «Папка/a.md → Архив/a.md». Запись живёт [`RENAME_TTL`] и не
//! удаляется ни вопросом, ни [`OwnWrites::forget`] (папка может прийти
//! несколькими событиями). Подтверждённое внешнее переименование, которое
//! уводит «куда» записи, её забывает ([`OwnWrites::forget_rename_by_external`],
//! задача 0.2-6.1). Записей не больше [`MAX_RENAMES`], а ключей в
//! них не больше [`MAX_RENAME_BYTES`] байт — лишние, самые старые,
//! забываются. Журнал только в памяти, на диск не пишется.

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

/// Сколько живёт запись о своём переименовании или переносе.
pub(crate) const RENAME_TTL: Duration = Duration::from_secs(120);

/// Общий предел записей о своих переименованиях.
pub(crate) const MAX_RENAMES: usize = 1024;

/// Общий предел байт ключей в записях о своих переименованиях.
pub(crate) const MAX_RENAME_BYTES: usize = 1024 * 1024;

/// Своё переименование или перенос «откуда → куда».
#[derive(Debug)]
struct Rename {
    from: PathKey,
    to: PathKey,
    /// Когда записано.
    at: Instant,
    /// Байт в ключах обоих путей — для общего предела.
    bytes: usize,
}

impl Rename {
    /// Переименование `from → to` — это запись или путь под ней, уехавший
    /// с тем же остатком пути.
    fn covers(&self, from: &[String], to: &[String]) -> bool {
        from.starts_with(&self.from)
            && to.starts_with(&self.to)
            && from[self.from.len()..] == to[self.to.len()..]
    }
}

fn key_bytes(key: &PathKey) -> usize {
    key.iter().map(String::len).sum()
}

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
    /// Свои переименования в порядке записи.
    renames: VecDeque<Rename>,
    /// Байт ключей во всех записях `renames`.
    rename_bytes: usize,
}

impl Journal {
    fn expire_renames(&mut self, now: Instant) {
        self.renames
            .retain(|r| now.saturating_duration_since(r.at) < RENAME_TTL);
        self.rename_bytes = self.renames.iter().map(|r| r.bytes).sum();
    }

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
    /// Подставные часы — только в тестах (детерминированные сроки и гонки).
    #[cfg(test)]
    clock: Arc<Mutex<Option<Instant>>>,
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
        self.record_at(path, hash, size, self.now());
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
        let now = self.now();
        let mut journal = self.lock();
        journal.expire(now);
        let versions = journal.by_path.entry(key).or_default();
        versions.push_back(Version {
            hash: hash.to_ascii_lowercase(),
            size,
            at,
        });
        let dropped = versions.len().saturating_sub(VERSIONS_PER_PATH);
        versions.drain(..dropped);
        journal.total = journal.total + 1 - dropped;
        journal.expire(now);
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
            let now = self.now();
            let mut journal = self.lock();
            journal.expire(now);
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

    /// Запоминает своё переименование или перенос `from → to` **до**
    /// действия на диске: событие наблюдателя может прийти раньше, чем
    /// команда вернёт ответ.
    pub fn record_rename(&self, from: &VaultPath, to: &VaultPath) {
        let (from, to) = (path_key(from), path_key(to));
        let bytes = key_bytes(&from) + key_bytes(&to);
        let now = self.now();
        let mut journal = self.lock();
        journal.expire_renames(now);
        journal.renames.push_back(Rename {
            from,
            to,
            at: now,
            bytes,
        });
        journal.rename_bytes += bytes;
        while journal.renames.len() > MAX_RENAMES
            || (journal.rename_bytes > MAX_RENAME_BYTES && journal.renames.len() > 1)
        {
            if let Some(oldest) = journal.renames.pop_front() {
                journal.rename_bytes -= oldest.bytes;
            }
        }
    }

    /// Убирает последнюю запись `from → to` — действие на диске не удалось.
    pub fn forget_rename(&self, from: &VaultPath, to: &VaultPath) {
        let (from, to) = (path_key(from), path_key(to));
        let mut journal = self.lock();
        if let Some(i) = journal
            .renames
            .iter()
            .rposition(|r| r.from == from && r.to == to)
            && let Some(removed) = journal.renames.remove(i)
        {
            journal.rename_bytes -= removed.bytes;
        }
    }

    /// Подтверждённое **внешнее** переименование увело `from` (задача
    /// 0.2-6.1, «Дополнительно»): записи своих переименований, у которых
    /// «куда» — `from` или лежит под ним, забываются. Иначе возврат снаружи
    /// «туда и обратно» в течение [`RENAME_TTL`] (`Б → А`, затем снова
    /// `А → Б`) был бы принят за своё и ссылки не обновились бы. Запись
    /// папки, из которой снаружи унесли один вложенный путь, остаётся: её
    /// события ещё могут идти.
    pub fn forget_rename_by_external(&self, from: &VaultPath) {
        let from = path_key(from);
        let mut journal = self.lock();
        journal.renames.retain(|r| !r.to.starts_with(&from));
        journal.rename_bytes = journal.renames.iter().map(|r| r.bytes).sum();
    }

    /// Переименование или перенос `from → to` (о котором сообщил
    /// наблюдатель) — своё: записано за последние [`RENAME_TTL`] само или
    /// как часть переименования папки над ним.
    pub fn is_own_rename(&self, from: &VaultPath, to: &VaultPath) -> bool {
        let (from, to) = (path_key(from), path_key(to));
        let now = self.now();
        let mut journal = self.lock();
        journal.expire_renames(now);
        journal.renames.iter().any(|r| r.covers(&from, &to))
    }

    /// Записана ли своя версия с таким хэшем и размером (без чтения
    /// файла) — для тестов хранилища.
    pub fn contains(&self, path: &VaultPath, hash: &str, size: u64) -> bool {
        let now = self.now();
        let mut journal = self.lock();
        journal.expire(now);
        journal
            .by_path
            .get(&path_key(path))
            .is_some_and(|versions| {
                versions
                    .iter()
                    .any(|v| v.size == size && v.hash.eq_ignore_ascii_case(hash))
            })
    }

    /// Подставляет часы журнала: дальше «сейчас» — это `now`.
    #[cfg(test)]
    pub(crate) fn set_now(&self, now: Instant) {
        *self
            .clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(now);
    }

    /// Число записей о своих переименованиях — для тестов.
    #[cfg(test)]
    pub(crate) fn renames(&self) -> usize {
        let now = self.now();
        let mut journal = self.lock();
        journal.expire_renames(now);
        journal.renames.len()
    }

    /// Байт ключей в записях о своих переименованиях — для тестов.
    #[cfg(test)]
    pub(crate) fn rename_bytes(&self) -> usize {
        self.lock().rename_bytes
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
        let now = self.now();
        let mut journal = self.lock();
        journal.expire(now);
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

    /// «Сейчас» для сроков журнала; в тестах часы можно подставить.
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(now) = *self
            .clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return now;
        }
        Instant::now()
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

    // --- Журнал своих переименований (задача 0.2-4.2) ---

    #[test]
    fn own_rename_recognized_before_command_returns() {
        // Гонка «событие наблюдателя пришло раньше ответа команды»
        // воспроизводится детерминированно: часы журнала подставлены, а
        // «событие» обрабатывается в шве хранилища сразу после переименования
        // на диске — команда ещё не вернула ответ.
        use std::cell::RefCell;
        use std::rc::Rc;
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::create_dir(dir.path().join("Папка")).expect("папка");
        std::fs::write(dir.path().join("Папка/заметка.md"), "x").expect("запись");
        let own = OwnWrites::new();
        let t0 = Instant::now();
        own.set_now(t0);
        let vault = crate::vault::Vault::open_with(
            dir.path(),
            crate::vault::OpenOptions {
                own_writes: own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");

        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let (journal, steps, root) = (own.clone(), log.clone(), dir.path().to_path_buf());
        crate::vault::after_disk_rename(move || {
            assert!(
                root.join("Проекты/заметка.md").exists(),
                "на диске уже новое имя"
            );
            // Событие наблюдателя задержалось почти до конца срока.
            journal.set_now(t0 + Duration::from_secs(119));
            let own_folder = journal.is_own_rename(&vp("Папка"), &vp("Проекты"));
            let own_child =
                journal.is_own_rename(&vp("Папка/заметка.md"), &vp("Проекты/заметка.md"));
            steps
                .borrow_mut()
                .push(format!("событие: своё={own_folder}, вложенное={own_child}"));
        });
        let new_path = vault
            .rename(&vp("Папка"), "Проекты")
            .expect("переименование");
        log.borrow_mut().push("ответ команды".to_owned());

        assert_eq!(new_path, vp("Проекты"));
        assert_eq!(
            *log.borrow(),
            vec![
                "событие: своё=true, вложенное=true".to_owned(),
                "ответ команды".to_owned()
            ]
        );

        // Перенос — так же: запись до действия.
        let journal = own.clone();
        let seen: Rc<RefCell<Option<bool>>> = Rc::default();
        let flag = seen.clone();
        crate::vault::after_disk_rename(move || {
            *flag.borrow_mut() =
                Some(journal.is_own_rename(&vp("Проекты/заметка.md"), &vp("заметка.md")));
        });
        vault
            .move_entry(&vp("Проекты/заметка.md"), &VaultPath::root())
            .expect("перенос");
        assert_eq!(*seen.borrow(), Some(true), "перенос узнан до ответа");
    }

    #[test]
    fn failed_rename_leaves_no_entry() {
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::write(dir.path().join("a.md"), "a").expect("запись");
        std::fs::write(dir.path().join("b.md"), "b").expect("запись");
        let own = OwnWrites::new();
        let vault = crate::vault::Vault::open_with(
            dir.path(),
            crate::vault::OpenOptions {
                own_writes: own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");
        assert!(vault.rename(&vp("a.md"), "b.md").is_err(), "имя занято");
        assert!(!own.is_own_rename(&vp("a.md"), &vp("b.md")));
        assert_eq!(own.renames(), 0);
    }

    #[test]
    fn own_folder_rename_covers_children() {
        let own = OwnWrites::new();
        own.record_rename(&vp("Папка"), &vp("Архив/Папка 2"));
        assert!(own.is_own_rename(&vp("Папка"), &vp("Архив/Папка 2")));
        assert!(own.is_own_rename(&vp("Папка/a.md"), &vp("Архив/Папка 2/a.md")));
        assert!(own.is_own_rename(&vp("Папка/x/y/b.md"), &vp("Архив/Папка 2/x/y/b.md")));
        // Вложенный путь уехал не туда же — не своё.
        assert!(!own.is_own_rename(&vp("Папка/a.md"), &vp("Архив/Папка 2/b.md")));
        assert!(!own.is_own_rename(&vp("Папка/a.md"), &vp("Архив/a.md")));
        // Похожее имя — не вложенный путь.
        assert!(!own.is_own_rename(&vp("Папка1/a.md"), &vp("Архив/Папка 21/a.md")));
        // Обратное направление и чужая пара — не своё.
        assert!(!own.is_own_rename(&vp("Архив/Папка 2"), &vp("Папка")));
        assert!(!own.is_own_rename(&vp("Другая"), &vp("Архив/Папка 2")));
    }

    #[test]
    fn rename_entry_expires_after_120s() {
        let own = OwnWrites::new();
        let t0 = Instant::now();
        own.set_now(t0);
        own.record_rename(&vp("a.md"), &vp("b.md"));
        own.set_now(t0 + Duration::from_millis(119_999));
        assert!(own.is_own_rename(&vp("a.md"), &vp("b.md")));
        own.set_now(t0 + Duration::from_secs(120));
        assert!(
            !own.is_own_rename(&vp("a.md"), &vp("b.md")),
            "запись старше 120 с забыта"
        );
        assert_eq!(own.renames(), 0);
    }

    #[test]
    fn case_and_nfc_insensitive() {
        let own = OwnWrites::new();
        // Интерфейс пишет «Мой.md» в NFC, диск отдаёт другой регистр и NFD.
        own.record_rename(&vp("Проекты/Мой.md"), &vp("Архив/Встреча.md"));
        assert!(own.is_own_rename(&vp("проекты/Мои\u{306}.md"), &vp("АРХИВ/встреча.md")));
        // Смена только регистра — тоже своя.
        own.record_rename(&vp("заметка.md"), &vp("Заметка.md"));
        assert!(own.is_own_rename(&vp("заметка.md"), &vp("Заметка.md")));
        // Удаление своих версий записи переименования не трогает.
        own.forget(&vp("Проекты"));
        assert!(own.is_own_rename(&vp("Проекты/Мой.md"), &vp("Архив/Встреча.md")));
    }

    // --- Дополнительно к 0.2-6.1 (по QA 0.2-4.2) ---

    /// Вопрос запись не «забирает»: своё переименование папки приходит
    /// несколькими событиями (папка и вложенные, повтор после сверки) — все
    /// свои.
    #[test]
    fn repeated_events_of_own_folder_all_own() {
        let own = OwnWrites::new();
        own.record_rename(&vp("Папка"), &vp("Архив"));
        for _ in 0..3 {
            assert!(own.is_own_rename(&vp("Папка"), &vp("Архив")));
            assert!(own.is_own_rename(&vp("Папка/a.md"), &vp("Архив/a.md")));
            assert!(own.is_own_rename(&vp("Папка/x/b.md"), &vp("Архив/x/b.md")));
        }
        assert_eq!(own.renames(), 1);
    }

    /// Смена только регистра — своя и в два шага (через скрытое имя):
    /// событие о ней узнаётся до ответа команды.
    #[test]
    fn two_step_case_change_is_own() {
        use std::cell::Cell;
        use std::rc::Rc;
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::write(dir.path().join("заметка.md"), "x").expect("запись");
        let own = OwnWrites::new();
        let vault = crate::vault::Vault::open_with(
            dir.path(),
            crate::vault::OpenOptions {
                own_writes: own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");
        let seen: Rc<Cell<Option<bool>>> = Rc::default();
        let (flag, journal) = (seen.clone(), own.clone());
        crate::vault::after_disk_rename(move || {
            flag.set(Some(
                journal.is_own_rename(&vp("заметка.md"), &vp("Заметка.md")),
            ));
        });
        vault
            .rename(&vp("заметка.md"), "Заметка.md")
            .expect("смена регистра");
        assert_eq!(seen.get(), Some(true));
        assert!(own.is_own_rename(&vp("заметка.md"), &vp("Заметка.md")));
    }

    /// Двухшаговая смена регистра сорвалась после первого шага — записи о
    /// своём переименовании нет: следующее такое же переименование снаружи
    /// — внешнее.
    #[test]
    fn two_step_case_change_failure_leaves_no_entry() {
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::write(dir.path().join("заметка.md"), "x").expect("запись");
        let own = OwnWrites::new();
        let vault = crate::vault::Vault::open_with(
            dir.path(),
            crate::vault::OpenOptions {
                own_writes: own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");
        crate::vault::fsops::crash_after_first_rename_step();
        assert!(vault.rename(&vp("заметка.md"), "Заметка.md").is_err());
        assert_eq!(own.renames(), 0);
        assert!(!own.is_own_rename(&vp("заметка.md"), &vp("Заметка.md")));
    }

    /// Подтверждённое внешнее переименование, которое уводит «куда» своей
    /// записи, забывает её: иначе возврат снаружи «туда и обратно» в
    /// течение срока записи был бы принят за своё.
    #[test]
    fn external_rename_from_own_target_forgets_entry() {
        let own = OwnWrites::new();
        own.record_rename(&vp("А.md"), &vp("Б.md"));
        own.record_rename(&vp("Папка/x.md"), &vp("Папка/y.md"));
        own.record_rename(&vp("Д.md"), &vp("Е.md"));
        // Снаружи: Б → А, затем перенос папки, где лежит «куда» другой записи.
        own.forget_rename_by_external(&vp("б.md"));
        own.forget_rename_by_external(&vp("Папка"));
        assert!(!own.is_own_rename(&vp("А.md"), &vp("Б.md")));
        assert!(!own.is_own_rename(&vp("Папка/x.md"), &vp("Папка/y.md")));
        assert!(
            own.is_own_rename(&vp("Д.md"), &vp("Е.md")),
            "чужая не тронута"
        );
        assert_eq!(own.renames(), 1);
        // Внешний перенос заметки из своей переименованной папки запись
        // папки не забывает: её события ещё могут идти.
        own.record_rename(&vp("Старая"), &vp("Новая"));
        own.forget_rename_by_external(&vp("Новая/a.md"));
        assert!(own.is_own_rename(&vp("Старая/b.md"), &vp("Новая/b.md")));
        assert!(own.rename_bytes() > 0);
    }

    #[test]
    fn bounded_total() {
        let own = OwnWrites::new();
        for n in 0..(MAX_RENAMES + 50) {
            own.record_rename(&vp(&format!("{n}.md")), &vp(&format!("{n} новая.md")));
        }
        assert!(own.renames() <= MAX_RENAMES);
        assert!(
            !own.is_own_rename(&vp("0.md"), &vp("0 новая.md")),
            "старые забыты"
        );
        let last = MAX_RENAMES + 49;
        assert!(
            own.is_own_rename(&vp(&format!("{last}.md")), &vp(&format!("{last} новая.md"))),
            "новые остаются"
        );

        // Предел и по объёму: длинные пути вытесняют старые записи раньше.
        let own = OwnWrites::new();
        let long = "д".repeat(250);
        let deep = (0..40).map(|_| long.as_str()).collect::<Vec<_>>().join("/");
        // Запись — около 40 КБ ключей: 40 записей больше предела в 1 МБ.
        for n in 0..40 {
            own.record_rename(
                &vp(&format!("{deep}/{n}.md")),
                &vp(&format!("{deep}/{n}н.md")),
            );
        }
        assert!(own.rename_bytes() <= MAX_RENAME_BYTES);
        assert!(own.renames() < 40, "старые длинные записи вытеснены");
        assert!(own.is_own_rename(
            &vp(&format!("{deep}/39.md")),
            &vp(&format!("{deep}/39н.md"))
        ));
    }
}
