//! Команды ядра API v1 — контракт между ядром и интерфейсом (спецификация
//! 0.1, С5.5; ADR 0009).
//!
//! Каждая команда:
//! - перечислена в [`list.rs`](list.rs) — оттуда строятся и
//!   `AppManifest::commands` в `build.rs`, и обработчик [`handler`];
//! - разрешена главному окну в `capabilities/main.json` (`allow-<команда>`);
//! - описана снимком `contracts/v1/<команда>.snap` (запрос, ответ, ошибки),
//!   который проверяют и `cargo test`, и Vitest (`src/api/`);
//! - принимает параметры одним объектом `request` со строгой схемой
//!   (`deny_unknown_fields`): интерфейс вызывает `invoke("note_read",
//!   { request: { path } })`;
//! - работает только с хранилищем своего окна ([`Core`]);
//! - асинхронная: Tauri выполняет её вне главного потока, окно не замирает.
//!
//! Тела команд только вызывают модули ядра; модули в 0.4 — заглушки с
//! ошибкой `not_implemented`. Проверка границ параметров (длины, число
//! элементов) и события — задача 1.7.

mod error;
mod events;
mod files;
mod links;
#[macro_use]
mod list;
mod note;
mod settings;
mod types;
mod vaults;

#[cfg(test)]
mod acl_tests;
#[cfg(test)]
mod contract_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub use error::{CoreError, CoreResult, ErrorCode, NameProblem};
pub use events::{
    EVENT_NAMES, NoteChangedPayload, NoteConflictSavedPayload, NoteDeletedPayload,
    NoteRenamedPayload, TreeChange, VaultChangedPayload, event,
};
pub use files::*;
pub use links::*;
pub use note::*;
pub use settings::*;
pub use types::{API_VERSION, EntryKind, FileVersion, VaultId, VaultPath, VaultSummary};
pub use vaults::*;

use crate::links::Links;
use crate::registry::Registry;
use crate::switcher::Switcher;
use crate::vault::Vault;

/// Имена всех команд ядра — тот же список, что в `AppManifest::commands`.
pub const COMMAND_NAMES: &[&str] = with_core_commands!(core_command_names);

macro_rules! core_handler {
    ($($command:ident),* $(,)?) => {
        tauri::generate_handler![$($command),*]
    };
}

/// Обработчик всех команд ядра для `Builder::invoke_handler`.
pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    with_core_commands!(core_handler)
}

/// Хранилище, открытое в окне, и вычисляемые по нему данные.
#[derive(Debug)]
pub struct OpenVault {
    /// Идентификатор в списке хранилищ.
    pub summary: VaultSummary,
    /// Файлы и контур.
    pub vault: Vault,
    /// Разрешение ссылок.
    pub links: RwLock<Links>,
    /// Быстрый переход.
    pub switcher: RwLock<Switcher>,
}

/// Состояние ядра для команд (`Builder::manage`): список хранилищ и
/// хранилища, открытые в окнах. Окно видит только своё хранилище — по метке
/// окна, а не по параметру от интерфейса.
#[derive(Debug, Default)]
pub struct Core {
    registry: Registry,
    windows: Mutex<HashMap<String, Arc<OpenVault>>>,
}

impl Core {
    /// Состояние со списком хранилищ из профиля ОС.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            windows: Mutex::default(),
        }
    }

    /// Список хранилищ.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Связывает окно с открытым в нём хранилищем (модуль окон, задача 1.3).
    pub fn bind_window(&self, window_label: &str, vault: OpenVault) {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(window_label.to_owned(), Arc::new(vault));
    }

    /// Хранилище окна, если оно открыто.
    pub fn window_vault(&self, window_label: &str) -> Option<Arc<OpenVault>> {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(window_label)
            .cloned()
    }

    /// Хранилище окна; не открыто — `no_vault_open`.
    pub fn require_vault(&self, window_label: &str) -> CoreResult<Arc<OpenVault>> {
        self.window_vault(window_label)
            .ok_or_else(|| CoreError::new(ErrorCode::NoVaultOpen))
    }
}
