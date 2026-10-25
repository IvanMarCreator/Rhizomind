//! Журнал своих записей ядра.

use crate::commands::VaultPath;

/// Журнал «путь → (хэш, размер)».
#[derive(Debug, Clone, Default)]
pub struct OwnWrites {}

impl OwnWrites {
    /// Пустой журнал.
    pub fn new() -> Self {
        Self::default()
    }

    /// Запоминает свою запись.
    pub fn record(&self, _path: &VaultPath, _hash: &str, _size: u64) {}
}
