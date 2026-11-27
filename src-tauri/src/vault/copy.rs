//! Записывает готовую копию под новым именем и регистрирует только успешную публикацию.
use super::{Vault, anchored, atomic, check_name, child, contour, io_error};
use crate::commands::{CoreError, CoreResult, ErrorCode, VaultPath};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(super) enum CopyInput<'a> {
    File(&'a VaultPath),
    Bytes(&'a [u8]),
}

impl Vault {
    pub(super) fn copy_user_file(
        &self,
        input: CopyInput<'_>,
        dir: &VaultPath,
        name: &str,
    ) -> CoreResult<Option<VaultPath>> {
        check_name(name)?;
        dir.check()?;
        if name.starts_with('.') || dir.components().iter().any(|part| part.starts_with('.')) {
            return Err(contour::invalid_path());
        }
        let root = anchored::Root::open(self.root())?;
        match input {
            CopyInput::Bytes(bytes) => self.publish_copy(&root, dir, name, bytes, |_| Ok(())),
            CopyInput::File(path) => {
                path.check()?;
                if path.components().iter().any(|part| part.starts_with('.')) {
                    return Err(contour::invalid_path());
                }
                let relative: PathBuf = path.components().iter().collect();
                let source = self.root().join(&relative);
                let parent = root.dir(relative.parent().ok_or_else(contour::invalid_path)?)?;
                let input = root.open_note(&parent, &relative)?;
                contour::check_opened_at(&input, &source)?;
                let initial = input.metadata().map_err(io_error)?;
                if !initial.is_file() {
                    return Err(CoreError::new(ErrorCode::NotANote));
                }
                self.publish_copy(&root, dir, name, (&input).take(initial.len()), |size| {
                    contour::check_opened_at(&input, &source)?;
                    let after = input.metadata().map_err(io_error)?;
                    if size != initial.len()
                        || after.len() != size
                        || initial.modified().ok() != after.modified().ok()
                    {
                        return Err(CoreError::new(ErrorCode::IoFailed));
                    }
                    Ok(())
                })
            }
        }
    }

    fn publish_copy(
        &self,
        root: &anchored::Root,
        dir: &VaultPath,
        name: &str,
        reader: impl Read,
        verify_source: impl FnOnce(u64) -> CoreResult<()>,
    ) -> CoreResult<Option<VaultPath>> {
        let path = child(dir, name);
        let dest_rel: PathBuf = dir.components().iter().collect();
        let destination = root.dir(&dest_rel)?;
        if self.sibling_taken(&destination.path_for_io(), name, None)? {
            return Err(CoreError::new(ErrorCode::AlreadyExists));
        }
        let tmp = self.tmp_dir();
        let staging = root.dir(
            tmp.strip_prefix(self.root())
                .map_err(|_| contour::outside_vault())?,
        )?;
        let mut temp = tempfile::Builder::new()
            .prefix(".svc-")
            .tempfile_in(staging.path_for_io())
            .map_err(io_error)?;
        let filename = temp.path().file_name().ok_or_else(contour::invalid_path)?;
        let expected_temp = tmp.join(filename);
        contour::check_opened_at(temp.as_file(), &expected_temp)?;

        let mut reader = BufReader::new(reader);
        let mut hash = blake3::Hasher::new();
        let mut size = 0;
        loop {
            let bytes = reader.fill_buf().map_err(io_error)?;
            if bytes.is_empty() {
                break;
            }
            temp.write_all(bytes).map_err(io_error)?;
            hash.update(bytes);
            size += bytes.len() as u64;
            let count = bytes.len();
            reader.consume(count);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o644))
                .map_err(io_error)?;
        }
        temp.as_file().sync_all().map_err(io_error)?;
        atomic::temp_written_hook();
        verify_source(size)?;
        contour::check_opened_at(&destination.file, &self.root().join(&dest_rel))?;
        contour::check_opened_at(&staging.file, &tmp)?;
        contour::check_opened_at(temp.as_file(), &expected_temp)?;
        self.own_writes
            .publish_created(&hash.finalize().to_hex(), size, || {
                root.publish(&staging, &destination, &path, temp)
            })
    }
}
