//! Команды группы «Файлы»: дерево и операции с ним.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::{CoreError, CoreResult, NameProblem};
use super::limits;
use super::types::{EntryKind, VaultPath};

/// Элемент дерева файлов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    /// Путь от корня хранилища.
    pub path: VaultPath,
    /// Папка, заметка или вложение.
    pub kind: EntryKind,
}

/// Ответ `tree_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeListResponse {
    /// Все видимые папки и файлы, плоским списком без порядка.
    pub entries: Vec<TreeEntry>,
}

/// Запрос `note_create` и `folder_create`: имя в папке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameInDirRequest {
    /// Папка (`[]` — корень).
    pub dir: VaultPath,
    /// Полное имя элемента, как на диске: для заметки — с `.md`
    /// (`Без названия.md`); интерфейс показывает имя заметки без расширения.
    pub name: String,
}

/// Запрос `name_check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameCheckRequest {
    /// Папка (`[]` — корень).
    pub dir: VaultPath,
    /// Полное имя, как на диске.
    pub name: String,
    /// При переименовании — путь самого элемента: он не считается
    /// совпадением с собой, и смена только регистра (`план.md` → `План.md`)
    /// разрешена. Поле можно не передавать; `null` — новый элемент.
    #[serde(default)]
    pub renaming: Option<VaultPath>,
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

/// Предупреждение о допустимом имени (под полем ввода, ключ словаря
/// `names.warning.<предупреждение>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameWarning {
    /// На заметку с таким именем нельзя сослаться вики-ссылкой: `#`, `[`,
    /// `]` или пробел в начале (С3.2, решение владельца 25.10.2026).
    Unlinkable,
}

impl NameWarning {
    /// Ключ словаря: `names.warning.<предупреждение>`.
    pub fn message_key(self) -> String {
        match self {
            Self::Unlinkable => "names.warning.unlinkable".to_owned(),
        }
    }
}

/// Ответ `name_check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NameCheck {
    /// Имя подходит.
    Ok {
        /// Имя допустимо, но с предупреждением; `null` — без предупреждения.
        warning: Option<NameWarning>,
    },
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
    limits::path(&request.dir, "dir")?;
    limits::name(&request.name, "name")?;
    crate::names::require_valid(&request.name)?;
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
    limits::path(&request.dir, "dir")?;
    limits::name(&request.name, "name")?;
    crate::names::require_valid(&request.name)?;
    let open = core.require_vault(window.label())?;
    let path = open.vault.create_folder(&request.dir, &request.name)?;
    Ok(EntryPathResponse { path })
}

/// Переименование файла или папки. Ссылки в 0.1 не обновляются (С3.2).
/// Заметка не может сменить расширение `.md`, другой файл — стать заметкой
/// (`invalid_name` с причиной `note_extension`); смена только регистра
/// разрешена.
#[tauri::command]
pub async fn entry_rename<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryRenameRequest,
) -> CoreResult<EntryPathResponse> {
    limits::path(&request.path, "path")?;
    limits::name(&request.new_name, "new_name")?;
    crate::names::require_valid(&request.new_name)?;
    let old_name = request
        .path
        .file_name()
        .ok_or_else(|| CoreError::invalid_argument("path"))?;
    if let Some(problem) = crate::names::check_rename_keeps_kind(old_name, &request.new_name) {
        return Err(CoreError::invalid_name(problem));
    }
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
    limits::path(&request.path, "path")?;
    limits::path(&request.new_dir, "new_dir")?;
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
    limits::path(&request.path, "path")?;
    let open = core.require_vault(window.label())?;
    // Окно подтверждения безвозвратного удаления — задачи 1.2 и 1.7; до
    // него безвозвратное удаление всегда отклоняется.
    open.vault.trash(&request.path, &|_| false)
}

/// Проверка имени при вводе: правила имён и совпадение с соседями (при
/// переименовании — кроме самого элемента); для допустимого имени —
/// предупреждение, если на него нельзя будет сослаться.
#[tauri::command]
pub async fn name_check<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameCheckRequest,
) -> CoreResult<NameCheck> {
    limits::path(&request.dir, "dir")?;
    if let Some(renaming) = &request.renaming {
        limits::path(renaming, "renaming")?;
    }
    if let Some(problem) = crate::names::check(&request.name) {
        return Ok(NameCheck::rejected(problem));
    }
    if let Some(problem) = request
        .renaming
        .as_ref()
        .and_then(VaultPath::file_name)
        .and_then(|old| crate::names::check_rename_keeps_kind(old, &request.name))
    {
        return Ok(NameCheck::rejected(problem));
    }
    let open = core.require_vault(window.label())?;
    let check =
        open.vault
            .check_new_name(&request.dir, &request.name, request.renaming.as_ref())?;
    Ok(match check {
        NameCheck::Ok { .. } => NameCheck::Ok {
            warning: crate::names::warning(&request.name),
        },
        rejected => rejected,
    })
}
