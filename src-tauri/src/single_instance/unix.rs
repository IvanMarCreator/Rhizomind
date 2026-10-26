//! Linux, macOS: файл-замок `instance.lock` рядом с `vaults.json` (`flock`).
//!
//! Файл пустой, создаётся первым запуском и не удаляется (удаление файла-замка
//! даёт гонки); замок снимает ОС при выходе или падении процесса. Дескриптор
//! открыт с `O_CLOEXEC` (по умолчанию в Rust): дочерние процессы (`xdg-open`
//! для внешних ссылок) замок не наследуют. Второй запуск файл не меняет.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Замок первого экземпляра: снимается, когда закрыт файл.
#[derive(Debug)]
pub struct LockGuard {
    _file: File,
}

/// Папка замка: `dirs::data_local_dir()/Rhizomind/` (там же `vaults.json`);
/// тестовая сборка — папка профиля сценарного теста, если она задана.
pub fn lock_dir() -> Option<PathBuf> {
    #[cfg(feature = "webdriver")]
    if let Some(dir) = std::env::var_os(crate::windows::e2e::PROFILE_ENV) {
        return Some(PathBuf::from(dir));
    }
    dirs::data_local_dir().map(|dir| dir.join(crate::registry::APP_DIR))
}

/// Занимает замок `path`. `Ok(None)` — его держит другой процесс.
pub fn try_acquire_at(path: &Path) -> io::Result<Option<LockGuard>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    // SAFETY: дескриптор принадлежит `file` и открыт на время вызова.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(LockGuard { _file: file }));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(error)
    }
}
