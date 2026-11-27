//! Выбор папки и различимого имени для копии вложения внутри хранилища.
use super::{Vault, child, contour, fit_name, require_note_name};
use crate::commands::{AttachmentOutcome, CoreError, CoreResult, ErrorCode, VaultPath};
use crate::names;

#[derive(Clone, Copy)]
pub(crate) enum AttachmentInput<'a> {
    File(&'a VaultPath),
    Png(&'a [u8]),
}

fn visible_path(path: &VaultPath) -> CoreResult<()> {
    path.check()?;
    if path.components().is_empty() || path.components().iter().any(|part| part.starts_with('.')) {
        return Err(contour::invalid_path());
    }
    Ok(())
}

/// Кратчайший однозначный суффикс, с расширением. Если полный путь тоже
/// совпадает с хвостом более глубокого пути, ведущий '/' привязывает его к корню.
fn link_target(path: &VaultPath, entries: &[VaultPath]) -> Option<String> {
    let parts = path.components();
    let keys: Vec<_> = parts.iter().map(|part| names::key(part)).collect();
    let mut longest = 0;
    for other in entries.iter().filter(|entry| *entry != path) {
        let common = other
            .components()
            .iter()
            .rev()
            .zip(keys.iter().rev())
            .take_while(|(part, key)| &names::key(part) == *key)
            .count();
        longest = longest.max(common);
    }
    let start = parts.len().saturating_sub(longest + 1);
    if !parts[start..].iter().all(|part| names::is_linkable(part)) {
        return None;
    }
    let target = parts[start..].join("/");
    Some(if longest == parts.len() {
        format!("/{target}")
    } else {
        target
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn file_paths(root: &super::anchored::Root) -> CoreResult<Vec<VaultPath>> {
    use std::path::PathBuf;
    let mut pending = vec![VaultPath::root()];
    let mut files = Vec::new();
    while let Some(parent) = pending.pop() {
        let relative: PathBuf = parent.components().iter().collect();
        let dir = match root.dir(&relative) {
            Ok(dir) => dir,
            Err(error) if parent.components().is_empty() => return Err(error),
            Err(_) => continue, // Папку удалили, переместили или заменили ссылкой.
        };
        let entries = std::fs::read_dir(dir.path_for_io()).map_err(super::io_error)?;
        for entry in entries {
            let entry = entry.map_err(super::io_error)?;
            let Some((name, kind)) = super::fsops::visible(&entry) else {
                continue;
            };
            let path = child(&parent, &name);
            if kind.is_file() {
                files.push(path);
            } else if kind.is_dir()
                && path.components().len() < crate::commands::limits::MAX_PATH_COMPONENTS
            {
                pending.push(path);
            }
        }
    }
    Ok(files)
}

impl Vault {
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn insert_attachment(
        &self,
        note: &VaultPath,
        input: AttachmentInput<'_>,
        name: &str,
    ) -> CoreResult<AttachmentOutcome> {
        use super::{anchored, copy::CopyInput, io_error};
        use std::path::PathBuf;

        visible_path(note)?;
        require_note_name(note)?;
        super::check_name(name)?;
        if name.starts_with('.') || !names::is_linkable(name) {
            return Err(CoreError::invalid_argument("name"));
        }
        let source = match input {
            AttachmentInput::File(file) => {
                visible_path(file)?;
                CopyInput::File(file)
            }
            AttachmentInput::Png(bytes) => CopyInput::Bytes(bytes),
        };
        let relative: PathBuf = note.components().iter().collect();
        let root = anchored::Root::open(self.root())?;
        let note_dir = root.dir(relative.parent().ok_or_else(contour::invalid_path)?)?;
        let note_file = root.open_note(&note_dir, &relative)?;
        if !note_file.metadata().map_err(io_error)?.is_file() {
            return Err(CoreError::new(ErrorCode::NotANote));
        }
        drop(note_file);
        let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
        let dir = self
            .read_settings()?
            .attachment_folders
            .remove(&ext.to_lowercase())
            .unwrap_or_else(|| {
                VaultPath::from_components(&note.components()[..note.components().len() - 1])
            });
        self.check_selected_folder(&dir)?;
        // Проверка до записи нужна, только когда полный путь нельзя записать
        // в вики-ссылку, но уникальное короткое имя всё ещё возможно.
        let check_short = if dir.components().iter().all(|part| names::is_linkable(part)) {
            None
        } else {
            Some(file_paths(&root)?)
        };
        for attempt in 1..=1000 {
            let candidate = if attempt == 1 {
                name.to_owned()
            } else {
                fit_name(stem, &format!(" {attempt}"), ext)
            };
            let requested = child(&dir, &candidate);
            if let Some(entries) = &check_short
                && link_target(&requested, entries).is_none()
            {
                return Err(CoreError::invalid_argument("name"));
            }
            match self.copy_user_file(source, &dir, &candidate) {
                Ok(path) => {
                    // Пользователь мог добавить одноимённый файл во время копирования.
                    // Сбой чтения после публикации не превращает успех записи в ошибку.
                    let target = path.as_ref().and_then(|actual| match file_paths(&root) {
                        Ok(entries) => link_target(actual, &entries),
                        Err(_)
                            if actual
                                .components()
                                .iter()
                                .all(|part| names::is_linkable(part)) =>
                        {
                            Some(format!("/{}", actual.components().join("/")))
                        }
                        Err(_) => None,
                    });
                    return Ok(AttachmentOutcome::Created { path, target });
                }
                Err(error) if error.code == ErrorCode::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(CoreError::new(ErrorCode::AlreadyExists))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub(crate) fn insert_attachment(
        &self,
        _note: &VaultPath,
        _source: AttachmentInput<'_>,
        _name: &str,
    ) -> CoreResult<AttachmentOutcome> {
        Err(CoreError::new(ErrorCode::NotImplemented))
    }
}
