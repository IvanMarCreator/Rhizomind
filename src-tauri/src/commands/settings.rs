//! Команды групп «Настройки и раскладка» и «Внешние ссылки».

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::{CoreError, CoreResult, ErrorCode};
use super::limits;
use super::types::VaultPath;

/// Тема интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Как в системе (по умолчанию).
    System,
    /// Светлая.
    Light,
    /// Тёмная.
    Dark,
}

/// Режим редактора по умолчанию.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorMode {
    /// Живой предпросмотр.
    LivePreview,
    /// Режим исходного текста.
    Source,
}

/// Ширина текста заметки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextWidth {
    /// Колонка ограниченной ширины.
    Column,
    /// Во всю ширину панели.
    Full,
}

/// Где создавать новые заметки (Ctrl+N).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NewNoteLocation {
    /// В корне хранилища.
    VaultRoot,
    /// В папке текущей заметки.
    CurrentFolder,
    /// В указанной папке.
    Folder {
        /// Папка.
        path: VaultPath,
    },
}

/// Настройки хранилища, как их видит интерфейс (С3.1). Состав для 0.1 —
/// настройки, поведение которых есть в спецификации; окончательный состав
/// окна настроек утверждает владелец в задаче 2.8.
///
/// Это тип контракта, а не файла: на диске — `vault::SettingsFile` с
/// версией формата (её ставит ядро) и терпимым чтением: файл следующей
/// версии с новыми полями или неизвестными значениями читается, неизвестное
/// сохраняется при записи. Диапазоны значений — `commands::limits`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Тема.
    pub theme: Theme,
    /// Режим редактора по умолчанию.
    pub editor_mode: EditorMode,
    /// Ширина текста.
    pub text_width: TextWidth,
    /// Размер шрифта текста, пиксели.
    pub font_size: u16,
    /// Где создавать новые заметки.
    pub new_note_location: NewNoteLocation,
    /// Спрашивать подтверждение перед удалением в корзину.
    pub confirm_delete: bool,
    /// Максимальный размер файла для редактирования, байты (по умолчанию 10 МБ).
    pub max_edit_size_bytes: u64,
}

impl Default for Settings {
    /// Значения по умолчанию: тема «Как в системе» и 10 МБ — из спецификации
    /// (С3.8, С3.3); остальные — предварительные, до утверждения окна
    /// настроек (задача 2.8).
    fn default() -> Self {
        Self {
            theme: Theme::System,
            editor_mode: EditorMode::LivePreview,
            text_width: TextWidth::Column,
            font_size: 16,
            new_note_location: NewNoteLocation::VaultRoot,
            confirm_delete: true,
            max_edit_size_bytes: 10 * 1024 * 1024,
        }
    }
}

/// Ответ `settings_read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsReadResponse {
    /// Настройки (значения по умолчанию, если файла ещё нет).
    pub settings: Settings,
}

/// Запрос `settings_write`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsWriteRequest {
    /// Настройки целиком.
    pub settings: Settings,
}

/// Раскладка окна (`.rhizomind/workspace.json`). Модель раскладки v1
/// принадлежит интерфейсу (`src/core/layout/`, ADR 0003, С5.3): ядро хранит
/// JSON-объект как есть и проверяет только, что это объект.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Workspace(pub serde_json::Map<String, serde_json::Value>);

/// Ответ `workspace_read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadResponse {
    /// Раскладка; `null` — ещё не сохранялась.
    pub workspace: Option<Workspace>,
}

/// Запрос `workspace_write`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWriteRequest {
    /// Раскладка целиком.
    pub workspace: Workspace,
}

/// Запрос `open_external`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenExternalRequest {
    /// Адрес из ссылки заметки.
    pub url: String,
}

/// Ответ `open_external`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpenExternalOutcome {
    /// Пользователь подтвердил — адрес передан ОС.
    Opened,
    /// Пользователь отказался в окне подтверждения.
    Cancelled,
}

/// Настройки хранилища окна.
#[tauri::command]
pub async fn settings_read<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<SettingsReadResponse> {
    let open = core.require_vault(window.label())?;
    Ok(SettingsReadResponse {
        settings: open.vault.read_settings()?,
    })
}

/// Записывает настройки хранилища окна.
#[tauri::command]
pub async fn settings_write<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: SettingsWriteRequest,
) -> CoreResult<()> {
    limits::settings(&request.settings)?;
    let open = core.require_vault(window.label())?;
    open.vault.write_settings(&request.settings)
}

/// Раскладка окна.
#[tauri::command]
pub async fn workspace_read<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<WorkspaceReadResponse> {
    let open = core.require_vault(window.label())?;
    Ok(WorkspaceReadResponse {
        workspace: open.vault.read_workspace()?,
    })
}

/// Записывает раскладку окна.
#[tauri::command]
pub async fn workspace_write<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: WorkspaceWriteRequest,
) -> CoreResult<()> {
    let bytes = serde_json::to_vec(&request.workspace)
        .map_err(|_| CoreError::invalid_argument("workspace"))?;
    limits::workspace_bytes(bytes.len())?;
    let open = core.require_vault(window.label())?;
    open.vault.write_workspace(&request.workspace)
}

/// Открывает внешнюю ссылку после подтверждения в окне ядра.
#[tauri::command]
pub async fn open_external<R: Runtime>(
    window: Window<R>,
    request: OpenExternalRequest,
) -> CoreResult<OpenExternalOutcome> {
    limits::url(&request.url)?;
    // Окно подтверждения блокирует поток до ответа — не рабочий поток async.
    tauri::async_runtime::spawn_blocking(move || {
        crate::external_links::open_with_confirmation(&window, &request.url)
    })
    .await
    .map_err(|_| CoreError::new(ErrorCode::IoFailed))?
}
