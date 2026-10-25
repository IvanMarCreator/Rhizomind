//! Тесты модуля `vault` через его открытый интерфейс ([`Vault`]): каждое
//! хранилище — во временной папке, рядом с ним — папка «снаружи» для
//! проверок контура.

mod contour;
mod read;
mod write;

use std::path::{Path, PathBuf};

use super::Vault;
use crate::commands::{CoreError, ErrorCode, VaultPath};

/// Хранилище во временной папке: `<tmp>/vault` — корень, `<tmp>/outside` —
/// папка за контуром с файлом `secret.md`.
pub struct TestVault {
    dir: tempfile::TempDir,
    pub vault: Vault,
}

impl TestVault {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::create_dir(dir.path().join("vault")).expect("корень");
        std::fs::create_dir(dir.path().join("outside")).expect("снаружи");
        std::fs::write(dir.path().join("outside").join("secret.md"), "секрет\n").expect("секрет");
        let vault = Vault::open(&dir.path().join("vault")).expect("хранилище открылось");
        Self { dir, vault }
    }

    /// Корень хранилища на диске.
    pub fn root(&self) -> PathBuf {
        self.dir.path().join("vault")
    }

    /// Папка за контуром.
    pub fn outside(&self) -> PathBuf {
        self.dir.path().join("outside")
    }

    /// Файл в хранилище по относительному пути `a/b.md`.
    pub fn file(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root(), |path, part| path.join(part))
    }

    /// Записывает файл в хранилище (с промежуточными папками).
    pub fn put(&self, rel: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.file(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("папки");
        }
        std::fs::write(&path, bytes).expect("запись");
        path
    }

    /// Байты файла хранилища.
    pub fn bytes(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.file(rel)).expect("чтение")
    }
}

/// Путь в хранилище из `a/b.md` (`""` — корень).
pub fn vp(rel: &str) -> VaultPath {
    if rel.is_empty() {
        VaultPath::root()
    } else {
        VaultPath::from_components(rel.split('/'))
    }
}

/// Код ошибки результата.
pub fn code<T: std::fmt::Debug>(result: Result<T, CoreError>) -> ErrorCode {
    result.expect_err("ожидалась ошибка").code
}

/// Символическая ссылка на папку (на Windows нужны права — у windows-latest
/// они есть).
pub fn symlink_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("ссылка на папку");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).expect("ссылка на папку");
}

/// Символическая ссылка на файл.
pub fn symlink_file(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("ссылка на файл");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).expect("ссылка на файл");
}
