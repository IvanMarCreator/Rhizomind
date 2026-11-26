use crate::commands::{CoreError, CoreResult, ErrorCode, VaultPath};
use crate::vault::{atomic, contour, io_error};
use std::fs::File;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(super) struct Root {
    file: File,
    path: PathBuf,
}

pub(super) struct Dir {
    pub(super) file: File,
    path: PathBuf,
    // Windows проверяет FILE_SHARE_DELETE также для переименования родителей.
    _parents: Vec<File>,
}

fn open_component(path: &Path, directory: bool) -> CoreResult<File> {
    let mut options = File::options();
    options.read(true);
    if directory {
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    } else {
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(io_error)?;
    let meta = file.metadata().map_err(io_error)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(contour::outside_vault());
    }
    if directory && !meta.is_dir() {
        return Err(CoreError::new(ErrorCode::NotFound));
    }
    contour::check_opened_at(&file, path)?;
    Ok(file)
}

impl Root {
    pub(super) fn open(path: &Path) -> CoreResult<Self> {
        Ok(Self {
            file: open_component(path, true)?,
            path: path.to_owned(),
        })
    }

    pub(super) fn dir(&self, relative: &Path) -> CoreResult<Dir> {
        let mut current = self.path.clone();
        let mut file = self.file.try_clone().map_err(io_error)?;
        let mut parents = Vec::new();
        for part in relative.components() {
            parents.push(file);
            current.push(part);
            file = open_component(&current, true)?;
        }
        Ok(Dir {
            file,
            path: current,
            _parents: parents,
        })
    }

    pub(super) fn open_note(&self, parent: &Dir, relative: &Path) -> CoreResult<File> {
        let name = relative.file_name().ok_or_else(contour::invalid_path)?;
        open_component(&parent.path.join(name), false)
    }

    pub(super) fn publish(
        &self,
        _staging: &Dir,
        destination: &Dir,
        path: &VaultPath,
        temp: tempfile::NamedTempFile,
    ) -> CoreResult<Option<VaultPath>> {
        let name = path.components().last().ok_or_else(contour::invalid_path)?;
        atomic::create_new(temp.into_temp_path(), &destination.path.join(name), None)
            .map_err(io_error)?;
        Ok(Some(path.clone()))
    }
}

impl Dir {
    pub(super) fn path_for_io(&self) -> PathBuf {
        self.path.clone()
    }
}
