//! Создание самостоятельной Markdown-заметки из шаблона открытого хранилища.
use super::Vault;
use crate::commands::{CoreResult, VaultPath};

#[cfg(target_os = "linux")]
#[path = "templates/linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "templates/windows.rs"]
mod platform;

#[cfg(any(windows, target_os = "linux"))]
impl Vault {
    pub(super) fn check_template_folder(&self, path: &VaultPath) -> CoreResult<()> {
        path.check()?;
        if path.components().iter().any(|part| part.starts_with('.')) {
            return Err(super::contour::invalid_path());
        }
        let relative: std::path::PathBuf = path.components().iter().collect();
        platform::Root::open(self.root())?.dir(&relative)?;
        Ok(())
    }

    /// Исходник читается через открытый дескриптор, без разбора Markdown и
    /// загрузки всего файла в память. Имя получает лишь готовая копия.
    pub fn create_from_template(
        &self,
        template: &VaultPath,
        dir: &VaultPath,
        name: &str,
    ) -> CoreResult<Option<VaultPath>> {
        use super::{atomic, check_name, child, contour, io_error, require_note_name};
        use crate::commands::{CoreError, ErrorCode};
        use std::io::{BufRead, BufReader, Read, Write};
        use std::path::PathBuf;

        check_name(name)?;
        dir.check()?;
        template.check()?;
        if dir.components().iter().any(|part| part.starts_with('.')) || name.starts_with('.') {
            return Err(contour::invalid_path());
        }
        let folder = self
            .read_settings()?
            .template_folder
            .ok_or_else(contour::invalid_path)?;
        let parts = template.components();
        if parts.len() <= folder.components().len()
            || !parts.starts_with(folder.components())
            || parts.iter().any(|part| part.starts_with('.'))
        {
            return Err(contour::invalid_path());
        }
        require_note_name(template)?;
        let path = child(dir, name);
        require_note_name(&path)?;

        let root = platform::Root::open(self.root())?;
        let relative: PathBuf = parts.iter().collect();
        let source = self.root().join(&relative);
        let parent = relative.parent().ok_or_else(contour::invalid_path)?;
        let source_dir = root.dir(parent)?;
        let input = root.open_note(&source_dir, &relative)?;
        contour::check_opened_at(&input, &source)?;
        let initial = input.metadata().map_err(io_error)?;
        if !initial.is_file() {
            return Err(CoreError::new(ErrorCode::NotANote));
        }
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

        let mut reader = BufReader::new((&input).take(initial.len()));
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
        contour::check_opened_at(&input, &source)?;
        let after = input.metadata().map_err(io_error)?;
        if size != initial.len()
            || after.len() != size
            || initial.modified().ok() != after.modified().ok()
        {
            return Err(CoreError::new(ErrorCode::IoFailed));
        }
        contour::check_opened_at(&destination.file, &self.root().join(&dest_rel))?;
        contour::check_opened_at(&staging.file, &tmp)?;
        contour::check_opened_at(temp.as_file(), &expected_temp)?;
        self.own_writes
            .publish_created(&hash.finalize().to_hex(), size, || {
                root.publish(&staging, &destination, &path, temp)
            })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
impl Vault {
    pub(super) fn check_template_folder(&self, path: &VaultPath) -> CoreResult<()> {
        self.existing_dir(path).map(|_| ())
    }

    pub fn create_from_template(
        &self,
        _template: &VaultPath,
        _dir: &VaultPath,
        _name: &str,
    ) -> CoreResult<Option<VaultPath>> {
        Err(crate::commands::CoreError::new(
            crate::commands::ErrorCode::NotImplemented,
        ))
    }
}
