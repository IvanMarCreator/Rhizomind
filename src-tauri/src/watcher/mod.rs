//! Слежение за папкой хранилища — интерфейс модуля, реализация в задаче 1.4
//! (ADR 0006: `notify` 9.0.0-rc.6 + `notify-debouncer-full` 0.8.0-rc.2).
//!
//! Один наблюдатель на хранилище, рекурсивно на корень. Наружу выходят уже
//! склеенные пачки изменений дерева [`VaultChangedPayload`]: события
//! `.rhizomind/` отброшены, свои записи ядра (по версии файла) не считаются
//! внешними, переименования распознаны по идентификатору файла, а при
//! переполнении буфера ОС приходит пачка с `rescan: true` после полной
//! сверки с диском. Запасной путь ADR 0006 (свой наблюдатель на
//! `ReadDirectoryChangesW`) меняет только реализацию этого модуля.

use std::path::Path;

mod own_writes;
mod tree;
#[cfg(test)]
mod tree_tests;

pub use own_writes::OwnWrites;

use crate::commands::{CoreError, CoreResult, FileVersion, VaultChangedPayload, VaultPath};

/// Получатель пачек изменений. Вызывается из потока наблюдателя.
pub type ChangeSink = Box<dyn Fn(VaultChangedPayload) + Send + Sync + 'static>;

/// Наблюдатель за одним хранилищем. Слежение останавливается при удалении
/// значения (`Drop`).
#[derive(Debug)]
pub struct VaultWatcher {
    _private: (),
}

impl VaultWatcher {
    /// Запускает слежение за корнем хранилища и сверку с диском при открытии.
    pub fn start(_root: &Path, _sink: ChangeSink) -> CoreResult<Self> {
        Err(CoreError::not_implemented())
    }

    /// Запоминает версию файла после своей записи: событие о файле с этой
    /// версией не будет внешним изменением.
    pub fn remember_own_write(&self, _path: &VaultPath, _version: &FileVersion) {}
}
