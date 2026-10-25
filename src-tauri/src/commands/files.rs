//! Команды группы «Файлы»: дерево и операции с ним.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::{CoreResult, NameProblem};
use super::types::{EntryKind, VaultPath};

/// Элемент дерева файлов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    /// Путь от корня хранилища.
    pub path: VaultPath,
    /// Папка, заметка или другой файл.
    pub kind: EntryKind,
}

/// Ответ `tree_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeListResponse {
    /// Все видимые папки и файлы, плоским списком без порядка.
    pub entries: Vec<TreeEntry>,
}

/// Запрос `note_create`, `folder_create` и `name_check`: имя в папке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameInDirRequest {
    /// Папка (`[]` — корень).
    pub dir: VaultPath,
    /// Полное имя элемента, как на диске: для заметки — с `.md`
    /// (`Без названия.md`); интерфейс показывает имя заметки без расширения.
    pub name: String,
}

/// Ответ команд, которые создают, переименовывают или перемещают элемент.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPathResponse {
    /// Путь элемента после операции.
    pub path: VaultPath,
}

/// Запрос `entry_rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryRenameRequest {
    /// Что переименовать.
    pub path: VaultPath,
    /// Новое полное имя, как на диске (для заметки — с `.md`).
    pub new_name: String,
}

/// Запрос `entry_move`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryMoveRequest {
    /// Что переместить.
    pub path: VaultPath,
    /// Папка назначения (`[]` — корень).
    pub new_dir: VaultPath,
}

/// Запрос `entry_trash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPathRequest {
    /// Файл или папка.
    pub path: VaultPath,
}

/// Ответ `entry_trash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrashOutcome {
    /// Перемещено в корзину ОС.
    Trashed,
    /// На томе без корзины: удалено безвозвратно после подтверждения в окне ядра.
    DeletedPermanently,
    /// На томе без корзины пользователь отказался удалять безвозвратно.
    Cancelled,
}

/// Ответ `name_check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NameCheck {
    /// Имя подходит.
    Ok,
    /// Имя не подходит; пояснение под полем ввода — по `message_key`.
    Rejected {
        /// Причина.
        problem: NameProblem,
        /// Ключ словаря, всегда `problem.message_key()`.
        message_key: String,
    },
}

impl NameCheck {
    /// Отказ с причиной и ключом словаря.
    pub fn rejected(problem: NameProblem) -> Self {
        Self::Rejected {
            problem,
            message_key: problem.message_key(),
        }
    }
}

/// Все видимые папки и файлы хранилища окна.
#[tauri::command]
pub async fn tree_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<TreeListResponse> {
    let open = core.require_vault(window.label())?;
    Ok(TreeListResponse {
        entries: open.vault.tree()?,
    })
}

/// Новая заметка в папке.
#[tauri::command]
pub async fn note_create<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameInDirRequest,
) -> CoreResult<EntryPathResponse> {
    let open = core.require_vault(window.label())?;
    let path = open.vault.create_note(&request.dir, &request.name)?;
    Ok(EntryPathResponse { path })
}

/// Новая папка.
#[tauri::command]
pub async fn folder_create<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameInDirRequest,
) -> CoreResult<EntryPathResponse> {
    let open = core.require_vault(window.label())?;
    let path = open.vault.create_folder(&request.dir, &request.name)?;
    Ok(EntryPathResponse { path })
}

/// Переименование файла или папки. Ссылки в 0.1 не обновляются (С3.2).
#[tauri::command]
pub async fn entry_rename<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryRenameRequest,
) -> CoreResult<EntryPathResponse> {
    let open = core.require_vault(window.label())?;
    let path = open.vault.rename(&request.path, &request.new_name)?;
    Ok(EntryPathResponse { path })
}

/// Перемещение файла или папки в другую папку.
#[tauri::command]
pub async fn entry_move<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryMoveRequest,
) -> CoreResult<EntryPathResponse> {
    let open = core.require_vault(window.label())?;
    let path = open.vault.move_entry(&request.path, &request.new_dir)?;
    Ok(EntryPathResponse { path })
}

/// Удаление в корзину ОС (подтверждение обычного удаления — в интерфейсе по
/// настройке; подтверждение безвозвратного удаления — окно ядра).
#[tauri::command]
pub async fn entry_trash<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryPathRequest,
) -> CoreResult<TrashOutcome> {
    let open = core.require_vault(window.label())?;
    // Окно подтверждения безвозвратного удаления — задачи 1.2 и 1.7; до
    // него безвозвратное удаление всегда отклоняется.
    open.vault.trash(&request.path, &|_| false)
}

/// Проверка имени при вводе: правила имён и совпадение с соседями.
#[tauri::command]
pub async fn name_check<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameInDirRequest,
) -> CoreResult<NameCheck> {
    let open = core.require_vault(window.label())?;
    open.vault.check_new_name(&request.dir, &request.name)
}
