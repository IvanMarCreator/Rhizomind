//! Команды группы «Хранилища».

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State, Window};

use super::error::{CoreError, CoreResult, NameProblem};
use super::types::{API_VERSION, CurrentVault, VaultId, VaultSummary};
use super::{Core, blocking, check_entry_name, limits};
use crate::dialogs::{self, FolderPurpose};

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
    /// Папка на месте (диск подключён); `false` — открыть сейчас нельзя
    /// (задача 2.8б: пометка в окне управления).
    pub available: bool,
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
    /// Родительская папка — ровно та, что вернул последний `folder_pick`
    /// этого окна (задача 2.8б, решение владельца 62).
    pub parent: String,
}

/// Ответ `folder_pick`: родительская папка нового хранилища.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum FolderPickOutcome {
    /// Папка выбрана и проверена: полный путь для показа и для `vault_create`.
    Picked {
        /// Полный путь к папке.
        path: String,
    },
    /// Пользователь закрыл окно выбора папки.
    Cancelled,
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
    let (entries, available) = blocking(move || {
        let entries = registry.entries()?;
        let available: Vec<bool> = entries
            .vaults
            .iter()
            .map(|vault| std::path::Path::new(&vault.path).is_dir())
            .collect();
        Ok((entries, available))
    })
    .await?;
    let vaults = entries
        .vaults
        .into_iter()
        .zip(available)
        .map(|(vault, available)| VaultListItem {
            is_open: core.window_of_vault(&vault.id).is_some(),
            id: vault.id,
            name: vault.name,
            path: vault.path,
            available,
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

/// Второй рубеж прав (задача 2.8б): команды создания, открытия папки,
/// выбора папки и «Убрать из списка» — только окну «Управление
/// хранилищами». Первый рубеж — capability `manager` (окна хранилищ этих
/// прав не имеют); ядро отказывает и окну, которому права выдали по ошибке.
fn require_manager<R: Runtime>(core: &Core, window: &Window<R>) -> CoreResult<()> {
    if core.is_manager(window.label()) {
        Ok(())
    } else {
        Err(CoreError::invalid_argument("window"))
    }
}

/// Известные хранилища — для проверки родительской папки.
fn known_roots(registry: &crate::registry::Registry) -> CoreResult<Vec<std::path::PathBuf>> {
    Ok(registry
        .entries()?
        .vaults
        .into_iter()
        .map(|vault| std::path::PathBuf::from(vault.path))
        .collect())
}

/// Выбор родительской папки нового хранилища в системном окне ядра (Э11в,
/// «Обзор…»; решение владельца 62). Папка проверяется сразу
/// ([`crate::vault::Vault::check_parent`]: есть, папка, не внутри хранилища —
/// иначе `nested_vault`) и запоминается за окном: `vault_create` примет
/// только её.
#[tauri::command]
pub async fn folder_pick<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<FolderPickOutcome> {
    require_manager(&core, &window)?;
    let registry = core.shared_registry();
    let picker = window.clone();
    let picked = blocking(move || {
        let Some(folder) = dialogs::pick_folder(&picker, FolderPurpose::NewVaultParent)? else {
            return Ok(None);
        };
        let parent = crate::vault::Vault::check_parent(&folder, &known_roots(&registry)?)?;
        let path = parent
            .to_str()
            .ok_or_else(|| CoreError::new(super::ErrorCode::InvalidPath))?
            .to_owned();
        Ok(Some((parent, path)))
    })
    .await?;
    let Some((parent, path)) = picked else {
        return Ok(FolderPickOutcome::Cancelled);
    };
    core.remember_picked_parent(window.label(), parent);
    Ok(FolderPickOutcome::Picked { path })
}

/// Создаёт хранилище `name` в родительской папке `parent`, выбранной
/// `folder_pick` этого окна (Э11в; решение владельца 62).
///
/// Порядок: окно управления → правила имён → пределы `parent` → `parent`
/// совпадает с выбранной в окне ядра папкой (иначе `invalid_argument`) →
/// повторная проверка папки (могла измениться) → компонент пути и
/// «`parent.join(name)` лежит прямо в `parent`» → создание папки
/// (`Vault::create`) → запись в список хранилищ → открытие
/// ([`open_in_window`]).
#[tauri::command]
pub async fn vault_create<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    core: State<'_, Core>,
    request: VaultCreateRequest,
) -> CoreResult<VaultChooseOutcome> {
    require_manager(&core, &window)?;
    check_entry_name(&request.name, "name")?;
    limits::folder_path(&request.parent, "parent")?;
    let parent = std::path::PathBuf::from(&request.parent);
    if core.picked_parent(window.label()).as_deref() != Some(parent.as_path()) {
        return Err(CoreError::invalid_argument("parent"));
    }
    let registry = core.shared_registry();
    let vault = blocking(move || {
        let parent = crate::vault::Vault::check_parent(&parent, &known_roots(&registry)?)?;
        crate::vault::check_component(&request.name)?;
        if parent.join(&request.name).parent() != Some(parent.as_path()) {
            return Err(CoreError::invalid_name(NameProblem::ForbiddenChars));
        }
        let root = crate::vault::Vault::create(&parent, &request.name)?;
        registry.add(&root)
    })
    .await?;
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
    require_manager(&core, &window)?;
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
pub async fn vault_remove_from_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: VaultIdRequest,
) -> CoreResult<()> {
    require_manager(&core, &window)?;
    let registry = core.shared_registry();
    blocking(move || registry.remove(&request.id)).await
}

/// Открывает окно «Управление хранилищами» (переключатель хранилищ, пункт
/// «Управление хранилищами…»); уже открытое окно получает фокус
/// ([`crate::windows::open_manager`], задача 2.8а).
#[tauri::command]
pub async fn vault_manager_open<R: Runtime>(app: AppHandle<R>) -> CoreResult<()> {
    crate::windows::open_manager(&app)
}

/// Открытие хранилища для команд группы «Хранилища» — одна функция для
/// `vault_create`, `vault_open_folder` и `vault_open` (ревью 0.4): уже
/// открытое хранилище получает фокус, окно без хранилища открывает его в
/// себе, иначе — новое окно ([`crate::windows::open_vault`], задача 1.3).
pub async fn open_in_window<R: Runtime>(
    app: AppHandle<R>,
    _core: &Core,
    window: &Window<R>,
    vault: VaultSummary,
) -> CoreResult<VaultOpenOutcome> {
    crate::windows::open_vault(&app, window.label(), vault).await
}
