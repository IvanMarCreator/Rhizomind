//! Команды группы «Заметка»: чтение и запись с версией файла.
//!
//! **Правило «байт в байт» (С3.3).** Интерфейс всегда работает с текстом,
//! где переносы строк — `\n`, а BOM нет. Всё остальное знает только ядро:
//! - `note_read` убирает BOM и приводит `\r\n` к `\n`;
//! - `note_write` берёт BOM и переносы из файла на диске в момент записи.
//!   Если файл на диске в ожидаемой версии и его текст после того же
//!   приведения равен присланному, ядро **файл не пишет** и отвечает
//!   `saved` с прежней версией: открыть и сохранить без правок — файл не
//!   меняется ни на байт (в том числе файл со смешанными переносами);
//! - при изменении текста переносы приводятся к преобладающим в файле, BOM
//!   сохраняется, если был;
//! - новая заметка — UTF-8 без BOM, переносы LF.
//!
//! **Удалённая заметка.** `note_write` по пути, которого уже нет на диске, —
//! `not_found`. Открытую заметку удалили снаружи (событие `note:deleted`) —
//! интерфейс закрывает её с уведомлением и отбрасывает несохранённый ввод:
//! внешнее действие считается действием пользователя (решение владельца,
//! С3.6, инвариант 4).

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::CoreResult;
use super::limits;
use super::types::{FileVersion, VaultPath};

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
    /// Текст без BOM, переносы строк — `\n`. Файл больше
    /// [`limits::MAX_NOTE_BYTES`] (100 МБ) не читается целиком: текст пустой,
    /// `read_only` — `too_large`.
    pub text: String,
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
    /// Текст с переносами `\n`, без BOM. Не больше
    /// [`limits::MAX_NOTE_BYTES`] (100 МБ); лимит из настроек хранилища
    /// действует при открытии заметки (`read_only`).
    pub text: String,
    /// Версия, от которой редактор начинал правки. После `note:renamed`
    /// интерфейс пишет по новому пути с прежней ожидаемой версией.
    pub expected_version: FileVersion,
}

/// Ответ `note_write`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteWriteResponse {
    /// Записано (или текст не изменился и файл не тронут); файл на диске был
    /// в ожидаемой версии.
    Saved {
        /// Версия файла после операции.
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
    limits::path(&request.path, "path")?;
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
    limits::path(&request.path, "path")?;
    limits::note_text(&request.text, limits::MAX_NOTE_BYTES)?;
    // Лимит из настроек действует при открытии (`read_only`, С3.3), здесь —
    // только жёсткий потолок: снижение лимита не роняет автосохранение.
    let open = core.require_vault(window.label())?;
    open.vault
        .write_note(&request.path, &request.text, &request.expected_version)
}
