//! Команды группы «Заметка»: чтение и запись с версией файла.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::CoreResult;
use super::types::{FileVersion, VaultPath};

/// Перенос строк в файле.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    /// `\n` (новые заметки).
    Lf,
    /// `\r\n`.
    Crlf,
}

/// Как заметка лежит на диске — чтобы записать её байт в байт (С3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteFormat {
    /// Есть ли BOM UTF-8 в начале файла. В `text` BOM никогда нет.
    pub bom: bool,
    /// Преобладающий перенос строк. Текст передаётся с переносами как в
    /// файле; редактор возвращает его с этими же переносами (задача 2.4).
    pub line_ending: LineEnding,
}

/// Почему заметка открыта только для чтения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadOnlyReason {
    /// Файл больше лимита из настроек хранилища.
    TooLarge {
        /// Лимит в байтах на момент открытия.
        limit_bytes: u64,
    },
    /// Файл не в UTF-8: показан с заменой неверных байтов, запись запрещена.
    InvalidUtf8,
}

/// Запрос `note_read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteReadRequest {
    /// Заметка.
    pub path: VaultPath,
}

/// Ответ `note_read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteReadResponse {
    /// Текст без BOM, переносы строк как в файле.
    pub text: String,
    /// BOM и переносы строк файла.
    pub format: NoteFormat,
    /// Версия файла — вернуть в `note_write` как ожидаемую.
    pub version: FileVersion,
    /// `null` — можно редактировать; иначе причина «только чтение».
    pub read_only: Option<ReadOnlyReason>,
}

/// Запрос `note_write`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteWriteRequest {
    /// Заметка.
    pub path: VaultPath,
    /// Новый текст без BOM.
    pub text: String,
    /// Формат записи (обычно — из `note_read`).
    pub format: NoteFormat,
    /// Версия, от которой редактор начинал правки.
    pub expected_version: FileVersion,
}

/// Ответ `note_write`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteWriteResponse {
    /// Записано; файл на диске был в ожидаемой версии.
    Saved {
        /// Новая версия файла.
        version: FileVersion,
    },
    /// Файл изменился снаружи после `expected_version` (конфликт изменений):
    /// текст из редактора записан в заметку, версия с диска сохранена
    /// копией рядом. Ядро также отправляет событие `note:conflict-saved`.
    ConflictSaved {
        /// Новая версия заметки.
        version: FileVersion,
        /// Копия с версией с диска.
        copy_path: VaultPath,
    },
}

/// Читает заметку.
#[tauri::command]
pub async fn note_read<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NoteReadRequest,
) -> CoreResult<NoteReadResponse> {
    let open = core.require_vault(window.label())?;
    open.vault.read_note(&request.path)
}

/// Записывает заметку с проверкой ожидаемой версии.
#[tauri::command]
pub async fn note_write<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NoteWriteRequest,
) -> CoreResult<NoteWriteResponse> {
    let open = core.require_vault(window.label())?;
    open.vault.write_note(
        &request.path,
        &request.text,
        &request.format,
        &request.expected_version,
    )
}
