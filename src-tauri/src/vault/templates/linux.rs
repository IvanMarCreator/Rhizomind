//! Два разных свойства ядра: openat2 ограничивает разрешение пути при открытии,
//! а Landlock проверяет расположение обеих папок внутри операции renameat2.
use crate::commands::{CoreError, CoreResult, ErrorCode, VaultPath};
use crate::vault::{contour, io_error};
use linux_raw_sys::landlock::{
    LANDLOCK_ACCESS_FS_MAKE_REG, LANDLOCK_ACCESS_FS_REFER, LANDLOCK_ACCESS_FS_REMOVE_FILE,
    landlock_path_beneath_attr, landlock_rule_type::LANDLOCK_RULE_PATH_BENEATH,
};
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread;

#[cfg(test)]
thread_local! {
    static AFTER_PUBLISH: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

pub(super) struct Root {
    file: File,
    path: PathBuf,
}

pub(super) struct Dir {
    pub(super) file: File,
}

fn unavailable() -> CoreError {
    CoreError::new(ErrorCode::NotImplemented)
}

fn open_error(error: io::Error) -> CoreError {
    match error.raw_os_error() {
        Some(libc::ENOSYS) => unavailable(),
        Some(libc::EXDEV | libc::ELOOP) => contour::outside_vault(),
        _ => io_error(error),
    }
}

impl Root {
    pub(super) fn open(path: &Path) -> CoreResult<Self> {
        let file = File::options()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)
            .map_err(open_error)?;
        contour::check_opened_at(&file, path)?;
        Ok(Self {
            file,
            path: path.to_owned(),
        })
    }

    fn open_beneath(&self, relative: &Path, flags: i32) -> CoreResult<File> {
        if relative.as_os_str().is_empty() {
            return self.file.try_clone().map_err(io_error);
        }
        let name =
            CString::new(relative.as_os_str().as_bytes()).map_err(|_| contour::invalid_path())?;
        // SAFETY: open_how начинается полностью нулевым, неиспользуемые поля остаются нулём.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (flags | libc::O_CLOEXEC) as u64;
        how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_XDEV;
        // SAFETY: дескриптор корня, NUL-строка и структура живут до возврата из openat2.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                self.file.as_raw_fd(),
                name.as_ptr(),
                &how,
                std::mem::size_of_val(&how),
            )
        };
        if fd < 0 {
            return Err(open_error(io::Error::last_os_error()));
        }
        // SAFETY: успешный openat2 передал вызывающему владение новым fd.
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }

    pub(super) fn dir(&self, relative: &Path) -> CoreResult<Dir> {
        let file = self.open_beneath(relative, libc::O_RDONLY | libc::O_DIRECTORY)?;
        contour::check_opened_at(&file, &self.path.join(relative))?;
        Ok(Dir { file })
    }

    pub(super) fn open_note(&self, _parent: &Dir, relative: &Path) -> CoreResult<File> {
        self.open_beneath(relative, libc::O_RDONLY | libc::O_NONBLOCK)
    }

    pub(super) fn publish(
        &self,
        staging: &Dir,
        destination: &Dir,
        requested: &VaultPath,
        mut temp: tempfile::NamedTempFile,
    ) -> CoreResult<Option<VaultPath>> {
        let (name, parent) = requested
            .components()
            .split_last()
            .ok_or_else(contour::invalid_path)?;
        let parent: PathBuf = parent.iter().collect();
        let source = temp.path().file_name().ok_or_else(contour::invalid_path)?;
        let source = CString::new(source.as_bytes()).map_err(|_| contour::invalid_path())?;
        let target = CString::new(name.as_bytes()).map_err(|_| contour::invalid_path())?;
        // no_new_privs и Landlock необратимы для потока. Пул команд ядра продолжает
        // работать с другими хранилищами: поток публикации завершается сразу после rename.
        thread::scope(|scope| {
            let worker = thread::Builder::new()
                .spawn_scoped(scope, || {
                    restrict_publication(&self.file)?;
                    // SAFETY: имена — одиночные компоненты, дескрипторы открыты до конца вызова.
                    let result = unsafe {
                        libc::syscall(
                            libc::SYS_renameat2,
                            staging.file.as_raw_fd(),
                            source.as_ptr(),
                            destination.file.as_raw_fd(),
                            target.as_ptr(),
                            libc::RENAME_NOREPLACE,
                        )
                    };
                    if result == 0 {
                        return Ok(());
                    }
                    let error = io::Error::last_os_error();
                    // Сохраняем ошибку занятого имени из самой операции без перезаписи.
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        return Err(io_error(error));
                    }
                    contour::check_opened_at(&destination.file, &self.path.join(&parent))?;
                    match error.raw_os_error() {
                        Some(libc::ENOSYS | libc::EOPNOTSUPP | libc::EINVAL) => Err(unavailable()),
                        Some(libc::EROFS | libc::EACCES) => {
                            Err(CoreError::new(ErrorCode::ReadOnly))
                        }
                        _ => Err(io_error(error)),
                    }
                })
                .map_err(io_error)?;
            worker
                .join()
                .map_err(|_| CoreError::new(ErrorCode::IoFailed))?
        })?;
        temp.disable_cleanup(true);
        #[cfg(test)]
        if let Some(action) = AFTER_PUBLISH.with(|cell| cell.borrow_mut().take()) {
            action();
        }
        let _ = destination.file.sync_all();
        // Публикация состоялась. Дальнейшие действия пользователя определяют
        // лишь возможность немедленно открыть созданный объект.
        Ok(self.visible_note(temp.as_file()))
    }

    fn visible_note(&self, file: &File) -> Option<VaultPath> {
        let absolute = contour::final_path(file)?;
        let relative = absolute.strip_prefix(&self.path).ok()?;
        let parts = relative
            .iter()
            .map(|part| part.to_str())
            .collect::<Option<Vec<_>>>()?;
        if parts.iter().any(|part| part.starts_with('.')) {
            return None;
        }
        let path = VaultPath::from_components(parts);
        path.check().ok()?;
        crate::vault::require_note_name(&path).ok()?;
        let visible = self
            .open_beneath(relative, libc::O_PATH)
            .ok()?
            .metadata()
            .ok()?;
        let written = file.metadata().ok()?;
        ((visible.dev(), visible.ino()) == (written.dev(), written.ino())).then_some(path)
    }
}

impl Dir {
    pub(super) fn path_for_io(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}/.", self.file.as_raw_fd()))
    }
}

fn restrict_publication(root: &File) -> CoreResult<()> {
    let rights = u64::from(
        LANDLOCK_ACCESS_FS_REMOVE_FILE | LANDLOCK_ACCESS_FS_MAKE_REG | LANDLOCK_ACCESS_FS_REFER,
    );
    // В ABI 2 передаётся лишь первое 64-битное поле landlock_ruleset_attr.
    // SAFETY: указатель на поле прав валиден, размер соответствует переданному объекту.
    let ruleset = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &rights,
            std::mem::size_of_val(&rights),
            0u32,
        )
    };
    if ruleset < 0 {
        return Err(unavailable());
    }
    // SAFETY: успешный syscall вернул новый дескриптор.
    let ruleset = unsafe { OwnedFd::from_raw_fd(ruleset as i32) };
    let rule = landlock_path_beneath_attr {
        allowed_access: rights,
        parent_fd: root.as_raw_fd(),
    };
    // SAFETY: передана структура ядра, оба дескриптора живут до возврата.
    let add = unsafe {
        libc::syscall(
            libc::SYS_landlock_add_rule,
            ruleset.as_raw_fd(),
            LANDLOCK_RULE_PATH_BENEATH as u32,
            &rule,
            0u32,
        )
    };
    if add < 0 {
        return Err(unavailable());
    }
    // SAFETY: эти вызовы не используют указателей, меняют лишь права текущего потока.
    let zero: libc::c_ulong = 0;
    let one: libc::c_ulong = 1;
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, one, zero, zero, zero) } < 0
        || unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0u32) } < 0
    {
        return Err(unavailable());
    }
    Ok(())
}

#[test]
fn template_after_publication_returns_current_path_even_if_old_name_was_reused() {
    use crate::commands::Settings;
    use crate::vault::tests::{TestVault, vp};
    let t = TestVault::new();
    t.put("Образцы/План.md", b"original");
    t.put("Работа/Первая.md", b"existing");
    t.vault
        .write_settings(&Settings {
            template_folder: Some(vp("Образцы")),
            ..Default::default()
        })
        .unwrap();
    let old = t.file("Работа");
    let moved = t.file("Архив");
    AFTER_PUBLISH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(move || {
            std::fs::rename(&old, moved).unwrap();
            std::fs::create_dir(&old).unwrap();
            std::fs::write(old.join("Копия.md"), b"replacement").unwrap();
        }));
    });
    let note = t
        .vault
        .create_from_template(&vp("Образцы/План.md"), &vp("Работа"), "Копия.md")
        .unwrap();
    assert_eq!(note, Some(vp("Архив/Копия.md")));
    assert_eq!(t.bytes("Архив/Копия.md"), b"original");
    assert_eq!(t.bytes("Работа/Копия.md"), b"replacement");
    assert!(
        t.vault
            .own_writes
            .is_own(&vp("Архив/Копия.md"), &t.file("Архив/Копия.md"))
    );
}

#[test]
fn template_after_publication_reports_created_when_file_was_removed() {
    use crate::commands::Settings;
    use crate::vault::tests::{TestVault, vp};
    let t = TestVault::new();
    t.put("Образцы/План.md", b"original");
    t.vault
        .write_settings(&Settings {
            template_folder: Some(vp("Образцы")),
            ..Default::default()
        })
        .unwrap();
    let created = t.file("Копия.md");
    AFTER_PUBLISH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(move || {
            std::fs::remove_file(created).unwrap();
        }));
    });
    let result = t
        .vault
        .create_from_template(&vp("Образцы/План.md"), &vp(""), "Копия.md");
    assert_eq!(result.unwrap(), None);
    assert_eq!(t.bytes("Образцы/План.md"), b"original");
}

#[test]
fn template_publication_policy_only_permits_the_selected_vault() {
    let workspace = tempfile::tempdir().unwrap();
    let root = Root::open(workspace.path()).unwrap();
    let inbox = workspace.path().join("inbox");
    std::fs::create_dir(&inbox).unwrap();
    let staging = root.dir(Path::new("inbox")).unwrap();
    let other_vault = tempfile::tempdir().unwrap();
    let other = Root::open(other_vault.path())
        .unwrap()
        .dir(Path::new(""))
        .unwrap();
    let temp = tempfile::NamedTempFile::new_in(inbox).unwrap();
    let error = root
        .publish(
            &staging,
            &other,
            &VaultPath::from_components(["note.md"]),
            temp,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutsideVault);
    assert!(!other_vault.path().join("note.md").exists());
}
