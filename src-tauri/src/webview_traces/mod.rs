//! Папка данных WebView2 во временной папке ОС и её удаление (ADR 0007).
//!
//! WebView2 при каждом запуске создаёт папку данных (`Local State`, кэш
//! шейдеров, Crashpad), и выключить это нельзя. Поэтому:
//! - каждый запуск получает свою папку `<временная папка>/rhizomind-webview-<pid>-<случайное>`;
//! - после выхода ядро ждёт завершения процесса браузера и его дочерних и
//!   удаляет папку с повторами (пока процесс жив, файлы заняты);
//! - при запуске удаляются остатки прошлых запусков (аварийное завершение) —
//!   только свои папки по шаблону имени и только если их процесс не жив.

use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod process;

pub use process::{is_process_alive, wait_for_process_tree_exit};

/// Начало имени папки данных. Отличается от других временных папок проекта
/// (например, `rhizomind-portable-*` у `tools/package-portable`).
pub const DATA_DIR_PREFIX: &str = "rhizomind-webview-";

/// Длина случайной части имени (шестнадцатеричные цифры).
const RANDOM_HEX_LEN: usize = 16;

/// Имя папки данных: `rhizomind-webview-<pid>-<16 шестнадцатеричных цифр>`.
pub fn data_dir_name(pid: u32, random: u64) -> String {
    format!("{DATA_DIR_PREFIX}{pid}-{random:016x}")
}

/// Номер процесса-владельца, если имя — в точности имя папки данных.
pub fn owner_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(DATA_DIR_PREFIX)?;
    let (pid, random) = rest.split_once('-')?;
    let pid_ok = !pid.is_empty() && pid.len() <= 10 && pid.bytes().all(|b| b.is_ascii_digit());
    let random_ok = random.len() == RANDOM_HEX_LEN
        && random
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !(pid_ok && random_ok) {
        return None;
    }
    pid.parse().ok()
}

/// Случайное число из ключей `RandomState` (их стандартная библиотека берёт у ОС).
pub fn random_u64() -> u64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    hasher.finish()
}

/// Создаёт новую папку данных в `temp`. Существующая папка с тем же именем не
/// используется повторно (`create_dir`, а не `create_dir_all`).
pub fn create_data_dir(temp: &Path, pid: u32) -> io::Result<PathBuf> {
    let mut last_error = None;
    for _ in 0..8 {
        let path = temp.join(data_dir_name(pid, random_u64()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last_error = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("не удалось создать папку данных")))
}

/// Удаляет остатки прошлых запусков в `temp`: только папки (не файлы и не
/// символические ссылки или точки соединения) с именем по шаблону, чей
/// процесс-владелец не жив и не равен текущему. Возвращает удалённые пути.
pub fn sweep_leftovers(
    temp: &Path,
    current_pid: u32,
    is_alive: impl Fn(u32) -> bool,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        // file_type() не следует по ссылкам: ссылка на папку — не папка.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(owner_pid) else {
            continue;
        };
        if pid == current_pid || is_alive(pid) {
            continue;
        }
        let path = entry.path();
        if std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// Удаляет папку, повторяя попытки, пока файлы заняты (процессы движка
/// завершаются не мгновенно). Отсутствующая папка — успех.
pub fn remove_with_retries(path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(50);
    loop {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(500));
            }
        }
    }
}

/// Папка данных текущего запуска и процесс браузера, который её занимает.
#[derive(Debug)]
pub struct WebviewDataDir {
    path: PathBuf,
    browser_pid: Mutex<Option<u32>>,
}

/// Сколько ждать завершения процессов движка и удаления папки при выходе.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(15);

impl WebviewDataDir {
    /// Убирает остатки прошлых запусков и создаёт папку для этого запуска в
    /// системной временной папке.
    pub fn prepare() -> io::Result<Self> {
        Self::prepare_in(&std::env::temp_dir(), std::process::id())
    }

    /// То же в заданной папке и для заданного процесса (тесты).
    pub fn prepare_in(temp: &Path, pid: u32) -> io::Result<Self> {
        sweep_leftovers(temp, pid, is_process_alive);
        Ok(Self {
            path: create_data_dir(temp, pid)?,
            browser_pid: Mutex::new(None),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Запоминает `BrowserProcessId` движка: при выходе ядро ждёт его завершения.
    pub fn set_browser_pid(&self, pid: u32) {
        if let Ok(mut slot) = self.browser_pid.lock() {
            *slot = Some(pid);
        }
    }

    /// После закрытия окон: дождаться завершения процесса браузера и его
    /// дочерних, затем удалить папку с повторами.
    pub fn remove_after_exit(&self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let browser_pid = self.browser_pid.lock().ok().and_then(|slot| *slot);
        if let Some(pid) = browser_pid {
            wait_for_process_tree_exit(pid, timeout);
        }
        remove_with_retries(
            &self.path,
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_secs(1)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rhz-test-{tag}-{}", random_u64()));
        std::fs::create_dir_all(&dir).expect("временная папка теста");
        dir
    }

    #[test]
    fn name_has_prefix_pid_and_16_hex_digits() {
        assert_eq!(
            data_dir_name(4242, 0xab),
            "rhizomind-webview-4242-00000000000000ab"
        );
        assert_eq!(
            owner_pid("rhizomind-webview-4242-00000000000000ab"),
            Some(4242)
        );
    }

    #[test]
    fn only_exact_names_have_an_owner() {
        for name in [
            "rhizomind-portable-abc",
            "rhizomind-webview-",
            "rhizomind-webview-4242",
            "rhizomind-webview-4242-",
            "rhizomind-webview-4242-00000000000000a",
            "rhizomind-webview-4242-00000000000000abc",
            "rhizomind-webview-4242-00000000000000AB",
            "rhizomind-webview-42x2-00000000000000ab",
            "rhizomind-webview--00000000000000ab",
            "Rhizomind-webview-4242-00000000000000ab",
            "rhizomind-webview-99999999999-00000000000000ab",
            "x-rhizomind-webview-4242-00000000000000ab",
        ] {
            assert_eq!(owner_pid(name), None, "{name}");
        }
    }

    #[test]
    fn create_makes_new_unique_dir_inside_temp() {
        let root = temp_root("create");
        let a = create_data_dir(&root, 7).expect("папка");
        let b = create_data_dir(&root, 7).expect("папка");
        assert_ne!(a, b);
        for dir in [&a, &b] {
            assert!(dir.is_dir());
            assert_eq!(dir.parent(), Some(root.as_path()));
            assert_eq!(
                owner_pid(dir.file_name().unwrap().to_str().unwrap()),
                Some(7)
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sweep_removes_only_own_dirs_of_dead_processes() {
        let root = temp_root("sweep");
        let dead = root.join(data_dir_name(111, 1));
        let alive = root.join(data_dir_name(222, 2));
        let current = root.join(data_dir_name(333, 3));
        let other = root.join("rhizomind-portable-x");
        let file = root.join(data_dir_name(444, 4));
        for dir in [&dead, &alive, &current, &other] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(dead.join("Local State"), "x").unwrap();
        std::fs::write(&file, "файл с подходящим именем").unwrap();

        let removed = sweep_leftovers(&root, 333, |pid| pid == 222);

        assert_eq!(removed, vec![dead.clone()]);
        assert!(!dead.exists());
        assert!(alive.is_dir() && current.is_dir() && other.is_dir() && file.is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sweep_does_not_follow_links_to_dirs() {
        let root = temp_root("link");
        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("note.md"), "заметка").unwrap();
        let link = root.join(data_dir_name(111, 5));
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(sweep_leftovers(&root, 1, |_| false).is_empty());
        assert!(target.join("note.md").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sweep_of_missing_temp_is_empty() {
        let missing = std::env::temp_dir().join(format!("rhz-missing-{}", random_u64()));
        assert!(sweep_leftovers(&missing, 1, |_| false).is_empty());
    }

    #[test]
    fn remove_with_retries_deletes_tree_and_accepts_missing() {
        let root = temp_root("remove");
        std::fs::create_dir_all(root.join("EBWebView/Default")).unwrap();
        std::fs::write(root.join("EBWebView/Local State"), "{}").unwrap();
        remove_with_retries(&root, Duration::from_secs(1)).expect("удалено");
        assert!(!root.exists());
        remove_with_retries(&root, Duration::from_millis(10)).expect("нет папки — успех");
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn current_process_is_alive_and_unused_pid_is_not() {
        assert!(is_process_alive(std::process::id()));
        // Номера процессов кратны 4 на Windows и ограничены pid_max на Linux:
        // 4 000 000 001 не бывает ни там, ни там.
        assert!(!is_process_alive(4_000_000_001));
    }

    #[test]
    fn data_dir_removed_after_exit_without_browser() {
        let root = temp_root("session");
        let dir = WebviewDataDir {
            path: create_data_dir(&root, 9).unwrap(),
            browser_pid: Mutex::new(None),
        };
        std::fs::write(dir.path().join("Local State"), "{}").unwrap();
        dir.remove_after_exit(Duration::from_secs(1))
            .expect("удалено");
        assert!(!dir.path().exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
