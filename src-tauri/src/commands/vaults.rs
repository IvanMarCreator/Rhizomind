//! Команды группы «Хранилища».

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State, Window};

use super::Core;
use super::error::{CoreError, CoreResult};
use super::types::{API_VERSION, VaultId, VaultSummary};
use crate::registry::{self, FolderPurpose};

/// Хранилище в окне «Управление хранилищами».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultListItem {
    /// Идентификатор в списке хранилищ.
    pub id: VaultId,
    /// Имя хранилища.
    pub name: String,
    /// Полный путь к папке — для показа.
    pub path: String,
    /// Открыто ли в каком-либо окне.
    pub is_open: bool,
}

/// Ответ `vault_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultListResponse {
    /// Хранилища в порядке списка.
    pub vaults: Vec<VaultListItem>,
    /// Последнее открытое хранилище.
    pub last_opened: Option<VaultId>,
}

/// Ответ `vault_current`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultCurrentResponse {
    /// Версия API ядра, сейчас [`API_VERSION`] = 1.
    pub api_version: u32,
    /// Хранилище этого окна; `null` — окно без хранилища (управление хранилищами).
    pub vault: Option<VaultSummary>,
}

/// Запрос `vault_create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultCreateRequest {
    /// Имя папки нового хранилища (правила имён С3.2).
    pub name: String,
}

/// Ответ `vault_create` и `vault_open_folder`: выбор папки в диалоге ядра.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultChooseOutcome {
    /// Хранилище добавлено в список и открыто в новом окне (или получило фокус).
    Opened {
        /// Хранилище.
        vault: VaultSummary,
    },
    /// Пользователь закрыл диалог выбора папки.
    Cancelled,
}

/// Запрос `vault_open` и `vault_remove_from_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultIdRequest {
    /// Идентификатор хранилища из `vault_list`.
    pub id: VaultId,
}

/// Ответ `vault_open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultOpenOutcome {
    /// Открыто новое окно.
    Opened,
    /// Хранилище уже было открыто — фокус перешёл в его окно.
    Focused,
}

/// Список хранилищ и последнее открытое.
#[tauri::command]
pub async fn vault_list(core: State<'_, Core>) -> CoreResult<VaultListResponse> {
    let entries = core.registry().entries()?;
    let vaults = entries
        .vaults
        .into_iter()
        .map(|vault| VaultListItem {
            id: vault.id,
            name: vault.name,
            path: vault.path,
            is_open: false, // окна — задача 1.3
        })
        .collect();
    Ok(VaultListResponse {
        vaults,
        last_opened: entries.last_opened,
    })
}

/// Версия API и хранилище этого окна. Интерфейс вызывает первой при запуске.
#[tauri::command]
pub async fn vault_current<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<VaultCurrentResponse> {
    Ok(VaultCurrentResponse {
        api_version: API_VERSION,
        vault: core
            .window_vault(window.label())
            .map(|open| open.summary.clone()),
    })
}

/// Создаёт хранилище: родительскую папку пользователь выбирает в диалоге ядра.
#[tauri::command]
pub async fn vault_create<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, Core>,
    request: VaultCreateRequest,
) -> CoreResult<VaultChooseOutcome> {
    if crate::names::check(&request.name)?.is_some() {
        return Err(CoreError::invalid_argument("name"));
    }
    let Some(parent) = registry::pick_folder(&app, FolderPurpose::NewVaultParent)? else {
        return Ok(VaultChooseOutcome::Cancelled);
    };
    let _vault = core.registry().add(&parent.join(&request.name))?;
    // Создание папки и открытие окна — задачи 1.2 и 1.3.
    Err(CoreError::not_implemented())
}

/// Открывает существующую папку как хранилище (диалог выбора папки в ядре).
#[tauri::command]
pub async fn vault_open_folder<R: Runtime>(
    app: AppHandle<R>,
    core: State<'_, Core>,
) -> CoreResult<VaultChooseOutcome> {
    let Some(root) = registry::pick_folder(&app, FolderPurpose::ExistingVault)? else {
        return Ok(VaultChooseOutcome::Cancelled);
    };
    let _vault = core.registry().add(&root)?;
    // Открытие окна — задача 1.3.
    Err(CoreError::not_implemented())
}

/// Открывает хранилище в новом окне или переводит фокус на уже открытое.
#[tauri::command]
pub async fn vault_open(
    core: State<'_, Core>,
    request: VaultIdRequest,
) -> CoreResult<VaultOpenOutcome> {
    let _vault = core.registry().find(&request.id)?;
    // Окна — задача 1.3.
    Err(CoreError::not_implemented())
}

/// Убирает хранилище из списка; файлы не удаляются.
#[tauri::command]
pub async fn vault_remove_from_list(
    core: State<'_, Core>,
    request: VaultIdRequest,
) -> CoreResult<()> {
    core.registry().remove(&request.id)
}
