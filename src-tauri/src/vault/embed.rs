//! Чтение встраиваний через дескрипторы открытого хранилища.
use super::Vault;
#[cfg(any(windows, target_os = "linux"))]
use super::{anchored, attachment, contour, io_error};
use crate::commands::{CoreResult, EmbedOutcome, EmbedReason, VaultPath};
#[cfg(any(windows, target_os = "linux"))]
use crate::names;

#[cfg(any(windows, target_os = "linux"))]
mod image;

#[cfg(any(windows, target_os = "linux"))]
impl Vault {
    pub(crate) fn embed_attachment_target(
        &self,
        target: &VaultPath,
        from_root: bool,
    ) -> CoreResult<Result<VaultPath, EmbedReason>> {
        attachment::visible_path(target)?;
        let keys: Vec<_> = target.components().iter().map(|s| names::key(s)).collect();
        let root = anchored::Root::open(self.root())?;
        let mut found = None;
        for path in attachment::file_paths(&root)? {
            let parts = path.components();
            if parts.len() < keys.len() || from_root && parts.len() != keys.len() {
                continue;
            }
            if parts[parts.len() - keys.len()..]
                .iter()
                .zip(&keys)
                .all(|(s, k)| names::key(s) == *k)
            {
                if found.is_some() {
                    return Ok(Err(EmbedReason::Ambiguous));
                }
                found = Some(path);
            }
        }
        Ok(found.ok_or(EmbedReason::Missing))
    }

    pub(crate) fn embed_path(&self, path: &VaultPath) -> CoreResult<EmbedOutcome> {
        use std::io::Read;
        use std::path::PathBuf;
        const NOTE_BYTES: u64 = 256 * 1024;
        attachment::visible_path(path)?;
        let name = path
            .file_name()
            .ok_or_else(contour::invalid_path)?
            .to_owned();
        let root = anchored::Root::open(self.root())?;
        let relative: PathBuf = path.components().iter().collect();
        let expected = self.root().join(&relative);
        let parent = root.dir(relative.parent().ok_or_else(contour::invalid_path)?)?;
        let file = root.open_note(&parent, &relative)?;
        contour::check_opened_at(&file, &expected)?;
        let initial = file.metadata().map_err(io_error)?;
        if !initial.is_file() || !super::single_name(&file) {
            return Err(contour::outside_vault());
        }
        let extension = name
            .rsplit_once('.')
            .map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase());
        let limit = match extension.as_str() {
            "md" => NOTE_BYTES,
            "png" | "jpg" | "jpeg" => image::MAX_INPUT,
            _ => return Ok(EmbedOutcome::File { name }),
        };
        if initial.len() > limit {
            return Ok(EmbedReason::TooLarge.into());
        }
        let mut bytes = Vec::with_capacity(initial.len() as usize);
        (&file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        contour::check_opened_at(&file, &expected)?;
        let after = file.metadata().map_err(io_error)?;
        if !super::single_name(&file)
            || bytes.len() as u64 != initial.len()
            || after.len() != initial.len()
            || after.modified().ok() != initial.modified().ok()
        {
            return Ok(EmbedReason::Unavailable.into());
        }
        if extension == "md" {
            return Ok(match String::from_utf8(bytes) {
                Ok(text) => EmbedOutcome::Note {
                    name,
                    path: path.clone(),
                    text,
                },
                Err(_) => EmbedReason::InvalidData.into(),
            });
        }
        Ok(image::decode(&extension, &bytes, name).unwrap_or_else(Into::into))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
impl Vault {
    pub(crate) fn embed_path(&self, _: &VaultPath) -> CoreResult<EmbedOutcome> {
        Ok(EmbedReason::Unsupported.into())
    }
    pub(crate) fn embed_attachment_target(
        &self,
        _: &VaultPath,
        _: bool,
    ) -> CoreResult<Result<VaultPath, EmbedReason>> {
        Ok(Err(EmbedReason::Unsupported))
    }
}
