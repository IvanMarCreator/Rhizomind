//! Команды группы «Файлы»: дерево и операции с ним.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::{CoreError, CoreResult, NameProblem};
use super::events::TreeChange;
use super::types::{EntryKind, VaultPath};
use super::{Core, blocking, check_entry_name, checked_path};

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
    /// Имя начинается с точки: элемент будет скрыт в дереве файлов (С3.2).
    Hidden,
}

impl NameWarning {
    /// Ключ словаря: `names.warning.<предупреждение>`.
    pub fn message_key(self) -> String {
        match self {
            Self::Unlinkable => "names.warning.unlinkable".to_owned(),
            Self::Hidden => "names.warning.hidden".to_owned(),
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
    blocking(move || {
        Ok(TreeListResponse {
            entries: open.vault.tree()?,
        })
    })
    .await
}

/// Новая заметка в папке.
#[tauri::command]
pub async fn note_create<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameInDirRequest,
) -> CoreResult<EntryPathResponse> {
    checked_path(&request.dir, "dir")?;
    check_entry_name(&request.name, "name")?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let path = open.vault.create_note(&request.dir, &request.name)?;
        open.apply_own(&[TreeChange::Created {
            path: path.clone(),
            entry_kind: EntryKind::Note,
        }]);
        Ok(EntryPathResponse { path })
    })
    .await
}

/// Новая папка.
#[tauri::command]
pub async fn folder_create<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameInDirRequest,
) -> CoreResult<EntryPathResponse> {
    checked_path(&request.dir, "dir")?;
    check_entry_name(&request.name, "name")?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let path = open.vault.create_folder(&request.dir, &request.name)?;
        Ok(EntryPathResponse { path })
    })
    .await
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
    checked_path(&request.path, "path")?;
    check_entry_name(&request.new_name, "new_name")?;
    let old_name = request
        .path
        .file_name()
        .ok_or_else(|| CoreError::invalid_argument("path"))?;
    if let Some(problem) = crate::names::check_rename_keeps_kind(old_name, &request.new_name) {
        return Err(CoreError::invalid_name(problem));
    }
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let path = open.vault.rename(&request.path, &request.new_name)?;
        open.apply_own(&[TreeChange::Renamed {
            from: request.path,
            to: path.clone(),
        }]);
        Ok(EntryPathResponse { path })
    })
    .await
}

/// Перемещение файла или папки в другую папку.
#[tauri::command]
pub async fn entry_move<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryMoveRequest,
) -> CoreResult<EntryPathResponse> {
    checked_path(&request.path, "path")?;
    checked_path(&request.new_dir, "new_dir")?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let path = open.vault.move_entry(&request.path, &request.new_dir)?;
        open.apply_own(&[TreeChange::Renamed {
            from: request.path,
            to: path.clone(),
        }]);
        Ok(EntryPathResponse { path })
    })
    .await
}

/// Удаление в корзину ОС (подтверждение обычного удаления — в интерфейсе по
/// настройке). На диске без корзины ядро само показывает окно
/// подтверждения безвозвратного удаления (`dialogs::confirm_permanent_delete`,
/// модально к окну вызова, общий замок диалогов ядра) и удаляет только при
/// согласии; отказ или занятое другим диалогом окно — `cancelled`.
#[tauri::command]
pub async fn entry_trash<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EntryPathRequest,
) -> CoreResult<TrashOutcome> {
    checked_path(&request.path, "path")?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let confirm = |path: &VaultPath| {
            let name = path.file_name().unwrap_or_default();
            let kind = open.vault.entry_kind(path).unwrap_or(EntryKind::Attachment);
            crate::dialogs::confirm_permanent_delete(&window, name, kind)
        };
        let outcome = open.vault.trash(&request.path, &confirm)?;
        if outcome != TrashOutcome::Cancelled {
            open.apply_own(&[TreeChange::Removed { path: request.path }]);
        }
        Ok(outcome)
    })
    .await
}

/// Проверка имени при вводе: правила имён и совпадение с соседями (при
/// переименовании — кроме самого элемента); для допустимого имени —
/// предупреждение, если на него нельзя будет сослаться. Причины отказа те
/// же, что у создания и переименования (`vault::entry_name_problem`).
#[tauri::command]
pub async fn name_check<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NameCheckRequest,
) -> CoreResult<NameCheck> {
    checked_path(&request.dir, "dir")?;
    if let Some(renaming) = &request.renaming {
        checked_path(renaming, "renaming")?;
    }
    // Длинное имя — не ошибка запроса, а причина `too_long` под полем ввода.
    if let Some(problem) = crate::vault::entry_name_problem(&request.name) {
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
    blocking(move || {
        let check =
            open.vault
                .check_new_name(&request.dir, &request.name, request.renaming.as_ref())?;
        Ok(match check {
            NameCheck::Ok { .. } => NameCheck::Ok {
                warning: crate::names::warning(&request.name),
            },
            rejected => rejected,
        })
    })
    .await
}
