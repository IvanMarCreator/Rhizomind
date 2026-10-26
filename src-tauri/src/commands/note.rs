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
use tauri::{Manager, Runtime, State, Window};

use super::error::CoreResult;
use super::events::{NoteConflictSavedPayload, TreeChange};
use super::limits;
use super::types::{EntryKind, FileVersion, VaultPath};
use super::{Core, blocking, checked_path};

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

/// Читает заметку. Лимит размера для редактирования — из настроек
/// хранилища в момент открытия (С3.3: новое значение действует при
/// следующем открытии файла).
#[tauri::command]
pub async fn note_read<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NoteReadRequest,
) -> CoreResult<NoteReadResponse> {
    checked_path(&request.path, "path")?;
    let open = core.require_vault(window.label())?;
    blocking(move || open.vault.read_note(&request.path)).await
}

/// Записывает заметку с проверкой ожидаемой версии. Конфликт решается
/// только сравнением содержимого на диске с `expected_version` (хэш и
/// размер; дата изменения не важна), а не по событиям наблюдателя: копия
/// версии с диска сохраняется рядом, окну уходит `note:conflict-saved`.
#[tauri::command]
pub async fn note_write<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: NoteWriteRequest,
) -> CoreResult<NoteWriteResponse> {
    checked_path(&request.path, "path")?;
    // Лимит из настроек действует при открытии (`read_only`, С3.3), здесь —
    // только жёсткий потолок: снижение лимита не роняет автосохранение.
    limits::note_text(&request.text, limits::MAX_NOTE_BYTES)?;
    let open = core.require_vault(window.label())?;
    let path = request.path.clone();
    let response = blocking(move || {
        // Тестовая сборка: медленный диск для сценарного теста предела
        // ожидания при закрытии окна (задача 0.7).
        #[cfg(feature = "webdriver")]
        crate::windows::e2e::slow_note_write();
        let response =
            open.vault
                .write_note(&request.path, &request.text, &request.expected_version)?;
        if let NoteWriteResponse::ConflictSaved { copy_path, .. } = &response {
            open.apply_own(&[TreeChange::Created {
                path: copy_path.clone(),
                entry_kind: EntryKind::Note,
            }]);
        }
        Ok(response)
    })
    .await?;
    if let NoteWriteResponse::ConflictSaved { copy_path, .. } = &response {
        crate::events::send(
            &crate::events::WindowEmitter::new(window.app_handle().clone(), window.label()),
            super::events::event::NOTE_CONFLICT_SAVED,
            &NoteConflictSavedPayload {
                path,
                copy_path: copy_path.clone(),
            },
        );
    }
    Ok(response)
}
