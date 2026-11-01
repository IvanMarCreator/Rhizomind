//! Команда `clipboard_read_text` (решения 97, 98): текст системного буфера
//! обмена для пункта «Вставить» своего меню правки текста.
//!
//! Рубежи: право `allow-clipboard-read-text` — только окнам хранилищ
//! (`capabilities/main.json`); в ядре — только окно с открытым хранилищем
//! (`no_vault_open`) и только окно в фокусе (пользователь щёлкнул пункт
//! меню в этом окне; скрытое или фоновое окно буфер не читает). Предел —
//! настройка хранилища «предел размера редактируемого файла»: длиннее — не
//! вставляется. Ядро ничего не пишет и не хранит, текст не попадает в журнал.

use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime, State, Window};

use super::{Core, CoreError, CoreResult, ErrorCode, blocking};
use crate::clipboard::{self, ClipboardText};

/// Ответ `clipboard_read_text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipboardReadOutcome {
    /// Текст из буфера.
    Text { text: String },
    /// Текста в буфере нет.
    Empty,
    /// Текст длиннее предела хранилища (байты UTF-8) — не передаётся.
    TooLarge { limit_bytes: u64 },
    /// Окно не в фокусе: буфер не читается.
    NotFocused,
}

/// Текст системного буфера обмена. Только окно хранилища в фокусе.
#[tauri::command]
pub async fn clipboard_read_text<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<ClipboardReadOutcome> {
    let open = core.require_vault(window.label())?;
    if !window.is_focused().unwrap_or(false) {
        return Ok(ClipboardReadOutcome::NotFocused);
    }
    let app = window.app_handle().clone();
    blocking(move || {
        let limit_bytes = open.vault.read_settings()?.max_edit_size_bytes;
        let text = clipboard::read_text(&app, limit_bytes)
            .map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
        Ok(outcome(text, limit_bytes))
    })
    .await
}

fn outcome(text: ClipboardText, limit_bytes: u64) -> ClipboardReadOutcome {
    match text {
        ClipboardText::Text(text) => ClipboardReadOutcome::Text { text },
        ClipboardText::Empty => ClipboardReadOutcome::Empty,
        ClipboardText::TooLarge => ClipboardReadOutcome::TooLarge { limit_bytes },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_carries_text_or_limit() {
        assert_eq!(
            outcome(ClipboardText::Text("а".into()), 10),
            ClipboardReadOutcome::Text { text: "а".into() }
        );
        assert_eq!(
            outcome(ClipboardText::Empty, 10),
            ClipboardReadOutcome::Empty
        );
        assert_eq!(
            outcome(ClipboardText::TooLarge, 10),
            ClipboardReadOutcome::TooLarge { limit_bytes: 10 }
        );
    }
}
