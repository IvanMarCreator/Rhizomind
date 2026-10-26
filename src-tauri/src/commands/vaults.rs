//! Команды группы «Хранилища».

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State, Window};

use super::error::{CoreError, CoreResult, NameProblem};
use super::types::{API_VERSION, CurrentVault, VaultId, VaultSummary};
use super::{Core, blocking, check_entry_name};
use crate::dialogs::{self, FolderPurpose};
use crate::events::WindowEmitter;

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
    /// Хранилище этого окна (без пути к папке); `null` — окно без хранилища
    /// (управление хранилищами).
    pub vault: Option<CurrentVault>,
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
    let registry = core.shared_registry();
    let entries = blocking(move || registry.entries()).await?;
    let vaults = entries
        .vaults
        .into_iter()
        .map(|vault| VaultListItem {
            is_open: core.window_of_vault(&vault.id).is_some(),
            id: vault.id,
            name: vault.name,
            path: vault.path,
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
        vault: core.window_vault(window.label()).map(|open| CurrentVault {
            id: open.summary.id.clone(),
            name: open.summary.name.clone(),
        }),
    })
}

/// Создаёт хранилище: родительскую папку пользователь выбирает в диалоге ядра.
///
/// Порядок: правила имён (до диалога — чтобы не спрашивать папку зря) →
/// диалог (общий замок диалогов ядра; занято — `cancelled`) → компонент
/// пути и «`parent.join(name)` лежит прямо в `parent`» → создание папки
/// (`Vault::create`) → запись в список хранилищ → открытие
/// ([`open_in_window`]).
#[tauri::command]
pub async fn vault_create<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    core: State<'_, Core>,
    request: VaultCreateRequest,
) -> CoreResult<VaultChooseOutcome> {
    check_entry_name(&request.name, "name")?;
    let registry = core.shared_registry();
    let picker = window.clone();
    let summary = blocking(move || {
        let Some(parent) = dialogs::pick_folder(&picker, FolderPurpose::NewVaultParent)? else {
            return Ok(None);
        };
        crate::vault::check_component(&request.name)?;
        if parent.join(&request.name).parent() != Some(parent.as_path()) {
            return Err(CoreError::invalid_name(NameProblem::ForbiddenChars));
        }
        let root = crate::vault::Vault::create(&parent, &request.name)?;
        registry.add(&root).map(Some)
    })
    .await?;
    let Some(vault) = summary else {
        return Ok(VaultChooseOutcome::Cancelled);
    };
    open_in_window(app, &core, &window, vault.clone()).await?;
    Ok(VaultChooseOutcome::Opened { vault })
}

/// Открывает существующую папку как хранилище (диалог выбора папки в ядре).
#[tauri::command]
pub async fn vault_open_folder<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<VaultChooseOutcome> {
    let registry = core.shared_registry();
    let picker = window.clone();
    let summary = blocking(move || {
        let Some(root) = dialogs::pick_folder(&picker, FolderPurpose::ExistingVault)? else {
            return Ok(None);
        };
        registry.add(&root).map(Some)
    })
    .await?;
    let Some(vault) = summary else {
        return Ok(VaultChooseOutcome::Cancelled);
    };
    open_in_window(app, &core, &window, vault.clone()).await?;
    Ok(VaultChooseOutcome::Opened { vault })
}

/// Открывает хранилище в новом окне или переводит фокус на уже открытое.
#[tauri::command]
pub async fn vault_open<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    core: State<'_, Core>,
    request: VaultIdRequest,
) -> CoreResult<VaultOpenOutcome> {
    let registry = core.shared_registry();
    let vault = blocking(move || registry.find(&request.id)).await?;
    open_in_window(app, &core, &window, vault).await
}

/// Убирает хранилище из списка; файлы не удаляются.
#[tauri::command]
pub async fn vault_remove_from_list(
    core: State<'_, Core>,
    request: VaultIdRequest,
) -> CoreResult<()> {
    let registry = core.shared_registry();
    blocking(move || registry.remove(&request.id)).await
}

/// Открытие хранилища для команд группы «Хранилища» — одна функция для
/// `vault_create`, `vault_open_folder` и `vault_open` (ревью 0.4):
/// - хранилище уже открыто в каком-то окне — фокус на него (`focused`);
/// - окно вызова без хранилища — хранилище открывается в нём
///   ([`Core::open_vault`]: общий журнал своих записей, наблюдатель, события
///   только этому окну);
/// - иначе нужно новое окно — модуль окон, задача 1.3 (`not_implemented`).
pub async fn open_in_window<R: Runtime>(
    app: AppHandle<R>,
    core: &Core,
    window: &Window<R>,
    vault: VaultSummary,
) -> CoreResult<VaultOpenOutcome> {
    if let Some(label) = core.window_of_vault(&vault.id) {
        if let Some(open) = app.get_webview_window(&label) {
            let _ = open.set_focus();
        }
        return Ok(VaultOpenOutcome::Focused);
    }
    if core.window_vault(window.label()).is_some() {
        // Новое окно для другого хранилища — задача 1.3 (модуль окон).
        return Err(CoreError::not_implemented());
    }
    let label = window.label().to_owned();
    let registry = core.shared_registry();
    blocking(move || {
        let core = app.state::<Core>();
        let emit = Arc::new(WindowEmitter::new(app.clone(), label.clone()));
        let root = PathBuf::from(&vault.path);
        core.open_vault(&label, vault.clone(), &root, emit)?;
        // «Последнее открытое» не важно для открытия: ошибка списка
        // хранилищ не отменяет уже открытое хранилище.
        let _ = registry.mark_opened(&vault.id);
        Ok(VaultOpenOutcome::Opened)
    })
    .await
}
