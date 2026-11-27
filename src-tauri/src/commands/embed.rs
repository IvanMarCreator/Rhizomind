//! Локальное чтение для дерева и живого предпросмотра.
use super::{Core, CoreResult, VaultPath, blocking, checked_path};
use crate::links::parse_target;
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbedSource {
    Path { path: VaultPath },
    Target { from: VaultPath, target: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedRequest {
    pub source: EmbedSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedReason {
    Missing,
    Ambiguous,
    InvalidTarget,
    InvalidData,
    TooLarge,
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbedOutcome {
    Note {
        name: String,
        path: VaultPath,
        text: String,
    },
    Image {
        name: String,
        width: u32,
        height: u32,
        png: String,
    },
    File {
        name: String,
    },
    Unavailable {
        reason: EmbedReason,
    },
}

impl From<EmbedReason> for EmbedOutcome {
    fn from(reason: EmbedReason) -> Self {
        Self::Unavailable { reason }
    }
}

fn read_target(vault: &Vault, target: &str, note: Option<VaultPath>) -> CoreResult<EmbedOutcome> {
    let Ok(parsed) = parse_target(target) else {
        return Ok(EmbedReason::InvalidTarget.into());
    };
    let path = VaultPath::from_components(parsed.folders.iter().chain([&parsed.name]));
    if checked_path(&path, "target").is_err()
        || path.components().iter().any(|c| c.starts_with('.'))
    {
        return Ok(EmbedReason::InvalidTarget.into());
    }
    let literal = target
        .split(['|', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('\\')
        .trim();
    // Явный .md и имя без расширения используют тот же выбор заметки, что [[…]].
    if crate::names::is_note(literal) || !parsed.name.contains('.') && note.is_some() {
        return match note {
            Some(path) => vault.embed_path(&path),
            None => Ok(EmbedReason::Missing.into()),
        };
    }
    match vault.embed_attachment_target(&path, parsed.from_root)? {
        Ok(path) => vault.embed_path(&path),
        Err(EmbedReason::Missing) => match note {
            Some(path) => vault.embed_path(&path),
            None => Ok(EmbedReason::Missing.into()),
        },
        Err(reason) => Ok(reason.into()),
    }
}

#[tauri::command]
pub async fn embed_read<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: EmbedRequest,
) -> CoreResult<EmbedOutcome> {
    const MAX_TARGET_BYTES: usize =
        4 * super::limits::MAX_PATH_COMPONENTS * (super::limits::MAX_COMPONENT_UTF16 + 1);
    match &request.source {
        EmbedSource::Path { path } => checked_path(path, "path")?,
        EmbedSource::Target { from, target } => {
            checked_path(from, "from")?;
            if target.len() > MAX_TARGET_BYTES {
                return Ok(EmbedReason::InvalidTarget.into());
            }
        }
    }
    let open = core.require_vault(window.label())?;
    blocking(move || {
        // До чтения: даже несколько окон держат в памяти один декодируемый растр.
        static READER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = READER.lock().unwrap_or_else(|err| err.into_inner());
        let result = match request.source {
            EmbedSource::Path { path } => open.vault.embed_path(&path),
            EmbedSource::Target { from, target } => {
                let note = open.read_notes().links().resolve(&from, &target)?;
                read_target(&open.vault, &target, note)
            }
        };
        Ok(result.unwrap_or_else(|error| {
            match error.code {
                super::ErrorCode::NotFound => EmbedReason::Missing,
                super::ErrorCode::InvalidPath | super::ErrorCode::OutsideVault => {
                    EmbedReason::InvalidTarget
                }
                _ => EmbedReason::Unavailable,
            }
            .into()
        }))
    })
    .await
}
