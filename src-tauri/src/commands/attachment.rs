//! Вставка из явно выбранного файла открытого хранилища или системного буфера.
use super::{
    Core, CoreError, CoreResult, EntryKind, ErrorCode, TreeChange, VaultPath, blocking,
    check_entry_name, checked_path,
};
use crate::clipboard::{self, PasteData};
use crate::vault::AttachmentInput;
use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime, State, Window};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentSource {
    File { path: VaultPath },
    Clipboard {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRequest {
    pub note: VaultPath,
    pub source: AttachmentSource,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentOutcome {
    Created {
        path: Option<VaultPath>,
        target: Option<String>,
    },
    ExternalPath,
    Empty,
    NotFocused,
    TooLarge,
    Text {
        text: String,
    },
}

// Одиночный путь из «Копировать как путь» распознаётся только как строка.
fn looks_like_file_path(text: &str) -> bool {
    let path = text.trim().trim_matches('"');
    if path.contains(['\n', '\r']) {
        return false;
    }
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with(r"\\")
        || path
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

#[test]
fn attachment_literal_paths_are_detected_without_system_access() {
    for path in [
        r#""C:\Other\photo.png""#,
        r"\\server\share\photo.png",
        "file:///tmp/image.png",
        "/tmp/external.png",
    ] {
        assert!(looks_like_file_path(path));
    }
    assert!(!looks_like_file_path("См. /tmp/пример в тексте"));
    assert!(!looks_like_file_path("Заметка\nВторая строка"));
}

#[tauri::command]
pub async fn attachment_insert<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: AttachmentRequest,
) -> CoreResult<AttachmentOutcome> {
    checked_path(&request.note, "note")?;
    check_entry_name(&request.name, "name")?;
    if let AttachmentSource::File { path } = &request.source {
        checked_path(path, "source.path")?;
    }
    let open = core.require_vault(window.label())?;
    if matches!(request.source, AttachmentSource::Clipboard {})
        && !window.is_focused().unwrap_or(false)
    {
        return Ok(AttachmentOutcome::NotFocused);
    }
    let app = window.app_handle().clone();
    blocking(move || {
        let image;
        let input = match &request.source {
            AttachmentSource::File { path } => AttachmentInput::File(path),
            AttachmentSource::Clipboard {} => {
                let limit = open.vault.read_settings()?.max_edit_size_bytes;
                match clipboard::read_paste(&app, limit)
                    .map_err(|_| CoreError::new(ErrorCode::IoFailed))?
                {
                    PasteData::Png(bytes) => {
                        if !request.name.to_lowercase().ends_with(".png") {
                            return Err(CoreError::invalid_argument("name"));
                        }
                        image = bytes;
                        AttachmentInput::Png(&image)
                    }
                    PasteData::Text(text) => {
                        if looks_like_file_path(&text) {
                            return Ok(AttachmentOutcome::ExternalPath);
                        }
                        return Ok(AttachmentOutcome::Text { text });
                    }
                    PasteData::Empty => return Ok(AttachmentOutcome::Empty),
                    PasteData::ExternalPath => return Ok(AttachmentOutcome::ExternalPath),
                    PasteData::TooLarge => return Ok(AttachmentOutcome::TooLarge),
                }
            }
        };
        let result = open
            .vault
            .insert_attachment(&request.note, input, &request.name)?;
        if let AttachmentOutcome::Created {
            path: Some(path), ..
        } = &result
        {
            let entry_kind = if path.file_name().is_some_and(crate::names::is_note) {
                EntryKind::Note
            } else {
                EntryKind::Attachment
            };
            open.apply_own(&[TreeChange::Created {
                path: path.clone(),
                entry_kind,
            }]);
        }
        Ok(result)
    })
    .await
}
