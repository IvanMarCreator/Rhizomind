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
//! - работает только с хранилищем своего окна ([`Core`]): хранилище
//!   выбирается по метке окна, из которого пришёл вызов, а не по параметру
//!   от интерфейса — окно не может обратиться к хранилищу другого окна;
//! - асинхронная, а обращения к диску и тяжёлые вычисления — в отдельном
//!   потоке ([`blocking`], `spawn_blocking`): окно не замирает, рабочие
//!   потоки async не заняты.
//!
//! Порядок проверки в каждой команде (задача 1.7): типы и лишние поля —
//! при разборе (`deny_unknown_fields`, отказ строкой Tauri до вызова);
//! пределы длины и числа элементов — [`limits`]; каждый компонент пути —
//! [`checked_path`] (`invalid_path`); правила имён для нового имени —
//! [`check_entry_name`] (`invalid_name` с причиной, та же, что у
//! `name_check`); затем модуль ядра, который ещё раз проверяет контур по
//! диску и по дескриптору.

mod error;
mod events;
mod files;
mod index;
pub mod limits;
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
#[cfg(test)]
mod core_tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub use error::{CoreError, CoreResult, ErrorCode, NameProblem};
pub use events::{
    EVENT_NAMES, NoteChangedPayload, NoteConflictSavedPayload, NoteDeletedPayload,
    NoteRenamedPayload, TreeChange, VaultAvailabilityPayload, VaultChangedPayload, event,
};
pub use files::*;
pub use index::NoteIndex;
pub use links::*;
pub use note::*;
pub use settings::*;
pub use types::{
    API_VERSION, CurrentVault, EntryKind, FileVersion, VaultId, VaultPath, VaultSummary,
};
pub use vaults::*;

use crate::events::{Emit, VaultEvents};
use crate::own_writes::OwnWrites;
use crate::registry::Registry;
use crate::vault::{OpenOptions, TrashBackend, Vault};
use crate::watcher::VaultWatcher;

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
    /// Список заметок, разрешение ссылок и быстрый переход.
    index: RwLock<NoteIndex>,
}

impl OpenVault {
    /// Открытое хранилище со списком заметок с диска.
    pub fn new(summary: VaultSummary, vault: Vault) -> CoreResult<Self> {
        let index = NoteIndex::new(vault.notes()?);
        Ok(Self {
            summary,
            vault,
            index: RwLock::new(index),
        })
    }

    /// Список заметок для чтения (ссылки, быстрый переход).
    pub fn read_index(&self) -> RwLockReadGuard<'_, NoteIndex> {
        self.index
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Список заметок для изменения.
    pub fn write_index(&self) -> RwLockWriteGuard<'_, NoteIndex> {
        self.index
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Учитывает в списке заметок изменения своей команды окна сразу, не
    /// дожидаясь наблюдателя (повтор тем же событием наблюдателя безвреден).
    pub fn apply_own(&self, changes: &[TreeChange]) {
        let vault = &self.vault;
        self.write_index()
            .apply(changes, &|path| vault.note_created_ms(path));
    }
}

/// Хранилище окна и наблюдатель за его папкой. Наблюдатель останавливается,
/// когда запись удаляется из [`Core`] (окно закрыто).
struct WindowVault {
    open: Arc<OpenVault>,
    _watcher: Option<VaultWatcher>,
}

/// Состояние ядра для команд (`Builder::manage`): список хранилищ и
/// хранилища, открытые в окнах. Окно видит только своё хранилище — по метке
/// окна, а не по параметру от интерфейса.
#[derive(Default)]
pub struct Core {
    registry: Arc<Registry>,
    windows: Mutex<HashMap<String, WindowVault>>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").finish_non_exhaustive()
    }
}

impl Core {
    /// Состояние со списком хранилищ из профиля ОС.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry: Arc::new(registry),
            windows: Mutex::default(),
        }
    }

    /// Список хранилищ.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Список хранилищ для потока вне async ([`blocking`]).
    pub fn shared_registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    fn windows(&self) -> std::sync::MutexGuard<'_, HashMap<String, WindowVault>> {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Связывает окно с уже открытым хранилищем без наблюдателя (тесты).
    pub fn bind_window(&self, window_label: &str, vault: OpenVault) {
        self.windows().insert(
            window_label.to_owned(),
            WindowVault {
                open: Arc::new(vault),
                _watcher: None,
            },
        );
    }

    /// Открывает хранилище `root` в окне `window_label` — **одна функция
    /// открытия** для `vault_create`, `vault_open_folder` и `vault_open`
    /// (ревью 0.4). Зависимости передаются при открытии: общий журнал своих
    /// записей у хранилища и наблюдателя (`crate::own_writes`), события —
    /// только этому окну (`emit`). Блокирует поток (обход папки): вызывать
    /// через [`blocking`]. Прежнее хранилище окна закрывается.
    pub fn open_vault(
        &self,
        window_label: &str,
        summary: VaultSummary,
        root: &Path,
        emit: Arc<dyn Emit>,
    ) -> CoreResult<Arc<OpenVault>> {
        self.open_vault_with(window_label, summary, root, emit, None)
    }

    /// [`Core::open_vault`] с подменённой корзиной (тесты).
    pub(crate) fn open_vault_with(
        &self,
        window_label: &str,
        summary: VaultSummary,
        root: &Path,
        emit: Arc<dyn Emit>,
        trash: Option<TrashBackend>,
    ) -> CoreResult<Arc<OpenVault>> {
        let own_writes = OwnWrites::new();
        let vault = Vault::open_with(
            root,
            OpenOptions {
                own_writes: own_writes.clone(),
                trash,
            },
        )?;
        let open = Arc::new(OpenVault {
            summary,
            vault,
            index: RwLock::default(),
        });
        // Сначала слежение, потом список заметок: изменения между ними
        // придут событиями (повтор безвреден).
        let watcher = VaultWatcher::start_with(
            open.vault.root(),
            own_writes,
            VaultEvents::new(Arc::clone(&open), emit).into_sink(),
        )?;
        let notes = open.vault.notes()?;
        open.write_index().replace(notes);
        let previous = self.windows().insert(
            window_label.to_owned(),
            WindowVault {
                open: Arc::clone(&open),
                _watcher: Some(watcher),
            },
        );
        drop(previous);
        Ok(open)
    }

    /// Отвязывает окно от хранилища (окно закрыто): наблюдатель
    /// останавливается.
    pub fn unbind_window(&self, window_label: &str) {
        let removed = self.windows().remove(window_label);
        drop(removed);
    }

    /// Хранилище окна, если оно открыто.
    pub fn window_vault(&self, window_label: &str) -> Option<Arc<OpenVault>> {
        self.windows()
            .get(window_label)
            .map(|entry| Arc::clone(&entry.open))
    }

    /// Окно, в котором открыто хранилище `id`.
    pub fn window_of_vault(&self, id: &VaultId) -> Option<String> {
        self.windows()
            .iter()
            .find(|(_, entry)| entry.open.summary.id == *id)
            .map(|(label, _)| label.clone())
    }

    /// Хранилище окна; не открыто — `no_vault_open`.
    pub fn require_vault(&self, window_label: &str) -> CoreResult<Arc<OpenVault>> {
        self.window_vault(window_label)
            .ok_or_else(|| CoreError::new(ErrorCode::NoVaultOpen))
    }
}

/// Отвязка окна от хранилища при его закрытии (`Builder::on_window_event`).
/// Наблюдатель останавливается в отдельном потоке: остановка ждёт конца
/// текущей сверки, а главный поток ждать не должен.
pub fn on_window_event<R: tauri::Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    use tauri::Manager;
    if matches!(event, tauri::WindowEvent::Destroyed) {
        let app = window.app_handle().clone();
        let label = window.label().to_owned();
        std::thread::spawn(move || {
            if let Some(core) = app.try_state::<Core>() {
                core.unbind_window(&label);
            }
        });
    }
}

/// Выполняет блокирующую работу (диск, корзина, диалоги, поиск по большому
/// списку) в отдельном потоке (`spawn_blocking`), не занимая рабочие потоки
/// async.
pub async fn blocking<T, F>(work: F) -> CoreResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CoreResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| CoreError::new(ErrorCode::IoFailed))?
}

/// Путь от интерфейса: пределы ([`limits::path`], `invalid_argument` с
/// именем поля) и правила каждого компонента (`VaultPath::check`:
/// `..`, `.`, разделители, `:`, зарезервированные имена — `invalid_path`).
pub fn checked_path(path: &VaultPath, field: &str) -> CoreResult<()> {
    limits::path(path, field)?;
    path.check()
}

/// Новое имя элемента от интерфейса: предел длины и правила имён с той же
/// причиной, что у `name_check` (`vault::entry_name_problem`).
pub fn check_entry_name(name: &str, field: &str) -> CoreResult<()> {
    limits::name(name, field)?;
    match crate::vault::entry_name_problem(name) {
        Some(problem) => Err(CoreError::invalid_name(problem)),
        None => Ok(()),
    }
}
