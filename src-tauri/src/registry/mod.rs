//! Список хранилищ (`vaults.json`) — интерфейс модуля, реализация в задаче 1.3.
//!
//! Единственный файл приложения в профиле ОС:
//! `dirs::config_dir()/Rhizomind/vaults.json` (спецификация 0.1, С4). В нём
//! только идентификаторы, имена и пути хранилищ, последнее открытое
//! хранилище и состояние окон — ни имён, ни содержимого заметок (инвариант 2).
//!
//! Открытие хранилища в окне и фокус на уже открытое — модуль окон
//! (`src-tauri/src/windows/`, задача 1.3); этот модуль только помнит.
//! Выбор папки — системный диалог ядра ([`pick_folder`]), а не интерфейса:
//! интерфейс не получает прав `dialog:*` (ADR 0009).

use std::path::{Path, PathBuf};

use crate::commands::{CoreError, CoreResult, VaultId, VaultSummary};

/// Список хранилищ в профиле ОС.
///
/// Потокобезопасен: команды разных окон обращаются к одному экземпляру.
/// Каждое изменение сразу записывается на диск атомарно.
#[derive(Debug, Default)]
pub struct Registry {
    /// Путь к `vaults.json`; `None` — у заглушки, пока нет задачи 1.3.
    #[allow(dead_code)]
    file: Option<PathBuf>,
}

/// Содержимое списка для окна «Управление хранилищами».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntries {
    /// Хранилища в порядке списка.
    pub vaults: Vec<VaultSummary>,
    /// Последнее открытое хранилище.
    pub last_opened: Option<VaultId>,
}

impl Registry {
    /// Список в профиле текущего пользователя ОС. Файл читается лениво; его
    /// отсутствие — пустой список (первый запуск).
    pub fn from_profile() -> Self {
        Self::default()
    }

    /// Все хранилища и последнее открытое.
    pub fn entries(&self) -> CoreResult<RegistryEntries> {
        Err(CoreError::not_implemented())
    }

    /// Хранилище по идентификатору; нет в списке — `vault_not_found`.
    pub fn find(&self, _id: &VaultId) -> CoreResult<VaultSummary> {
        Err(CoreError::not_implemented())
    }

    /// Добавляет папку как хранилище (или возвращает уже известное с тем же
    /// путём). Папку `.rhizomind/` создаёт модуль `vault` при открытии.
    pub fn add(&self, _root: &Path) -> CoreResult<VaultSummary> {
        Err(CoreError::not_implemented())
    }

    /// Убирает хранилище из списка. Файлы на диске не трогаются.
    pub fn remove(&self, _id: &VaultId) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }

    /// Запоминает хранилище как последнее открытое.
    pub fn mark_opened(&self, _id: &VaultId) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }
}

/// Цель системного диалога выбора папки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPurpose {
    /// Родительская папка для нового хранилища.
    NewVaultParent,
    /// Существующая папка, которая станет хранилищем.
    ExistingVault,
}

/// Системный диалог выбора папки, показанный ядром (`tauri-plugin-dialog`
/// только из Rust). `Ok(None)` — пользователь отменил выбор.
pub fn pick_folder<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _purpose: FolderPurpose,
) -> CoreResult<Option<PathBuf>> {
    Err(CoreError::not_implemented())
}
