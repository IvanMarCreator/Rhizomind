//! Операции ОС для дерева: переименование без затирания, обход папок,
//! корзина.

use std::io;
use std::path::Path;

use super::TrashFailure;
use super::contour::check_component;

/// Переименование или перемещение, которое **не затирает** существующий
/// элемент: занято — `AlreadyExists`.
/// - Windows: `MoveFileExW` без `MOVEFILE_REPLACE_EXISTING`;
/// - Linux: `renameat2(RENAME_NOREPLACE)`; ФС без него — проверка и `rename`;
/// - другие ОС — проверка и `rename` (между ними — короткое окно гонки).
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let from = super::atomic::wide_path(from);
        let to = super::atomic::wide_path(to);
        // SAFETY: строки с нулём в конце живут до конца вызова.
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_from = CString::new(from.as_os_str().as_bytes())?;
        let c_to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: строки C с нулём в конце живут до конца вызова.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                c_from.as_ptr(),
                libc::AT_FDCWD,
                c_to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINVAL) | Some(libc::ENOSYS) => checked_rename(from, to),
            _ => Err(error),
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        checked_rename(from, to)
    }
}

#[cfg(not(windows))]
fn checked_rename(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(from, to)
}

/// Видимый элемент дерева: имя в UTF-8, не с точки в начале (скрытые и
/// `.rhizomind/`), проходит [`check_component`] (иначе к нему не обратиться
/// командой), не ссылка и не точка соединения.
pub fn visible(entry: &std::fs::DirEntry) -> Option<(String, std::fs::FileType)> {
    let name = entry.file_name().into_string().ok()?;
    if name.starts_with('.') || check_component(&name).is_err() {
        return None;
    }
    let kind = entry.file_type().ok()?;
    (!kind.is_symlink()).then_some((name, kind))
}

/// Корзина ОС в отдельном потоке (COM на Windows, freedesktop-корзина в
/// файлах на Linux). На Windows сначала проверяется тип тома: корзина есть
/// только на несъёмных локальных дисках — на флешке и в сетевой папке
/// оболочка удалила бы файл безвозвратно сама (`NoTrash` → подтверждение).
pub fn os_trash(path: &Path) -> Result<(), TrashFailure> {
    #[cfg(windows)]
    match volume_has_recycle_bin(path) {
        Some(true) => {}
        Some(false) => return Err(TrashFailure::NoTrash),
        // Тип тома не узнать — это сбой, а не «корзины нет»: безвозвратное
        // удаление не предлагается (повторное ревью 1.2, п. 2).
        None => return Err(TrashFailure::Failed),
    }
    let owned = path.to_path_buf();
    match std::thread::spawn(move || trash::delete(&owned)).join() {
        Ok(Ok(())) => Ok(()),
        // Корзина есть, но отказала (файл занят, нет прав) — ошибка, без
        // безвозвратного удаления. На Linux и macOS отличить «нет корзины»
        // от отказа нельзя: безвозвратное удаление там не предлагается.
        _ => Err(TrashFailure::Failed),
    }
}

/// Хранилище на внешнем носителе стало недоступно (решение владельца 66):
/// папки хранилища нет, и она не на несъёмном локальном томе — флешку
/// вынули, внешний или сетевой диск отключился. Тот же признак тома, что у
/// «диска без корзины» (`DRIVE_FIXED`); на Linux и macOS типа тома нет —
/// признак один: папка недоступна.
pub fn storage_lost(root: &Path) -> bool {
    !root.is_dir() && !volume_is_fixed(root)
}

fn volume_is_fixed(root: &Path) -> bool {
    #[cfg(test)]
    if test_volume::is_removable(root) {
        return false;
    }
    // Тестовая сборка: «внешний носитель» для сценарного теста (задача 0.7).
    #[cfg(feature = "webdriver")]
    if crate::windows::e2e::removable_volume() {
        return false;
    }
    #[cfg(windows)]
    {
        // Том — по ближайшей существующей папке (сама папка хранилища
        // пропала). Нет ни одной (буква диска исчезла) или том не определился —
        // не несъёмный: несъёмные тома не пропадают.
        root.ancestors()
            .find(|path| path.is_dir())
            .is_some_and(|existing| volume_has_recycle_bin(existing) == Some(true))
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        false
    }
}

/// Подмена типа тома в тестах: папки «на внешнем носителе».
#[cfg(test)]
pub(crate) mod test_volume {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    static REMOVABLE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    /// Папка `root` считается лежащей на внешнем носителе.
    pub fn set_removable(root: &Path) {
        REMOVABLE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(root.to_path_buf());
    }

    pub(super) fn is_removable(root: &Path) -> bool {
        REMOVABLE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|path| path == root)
    }
}

/// Том несъёмный локальный (`DRIVE_FIXED`): у таких томов есть корзина.
/// `None` — том не определился (`GetVolumePathNameW` отказал).
#[cfg(windows)]
fn volume_has_recycle_bin(path: &Path) -> Option<bool> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    const DRIVE_FIXED: u32 = 3;
    let wide = super::atomic::wide_path(path);
    let mut volume = vec![0u16; 1024];
    // SAFETY: строки с нулём в конце; буфер длиной `volume.len()`.
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) };
    if ok == 0 {
        return None;
    }
    // SAFETY: `volume` — строка с нулём в конце от GetVolumePathNameW.
    Some(unsafe { GetDriveTypeW(volume.as_ptr()) } == DRIVE_FIXED)
}

/// Журнал двухшаговой смены регистра (`.rhizomind/rename.json`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameJournal {
    /// Прежнее имя.
    pub from: crate::commands::VaultPath,
    /// Промежуточное скрытое имя.
    pub step: crate::commands::VaultPath,
    /// Новое имя.
    pub to: crate::commands::VaultPath,
}

#[cfg(test)]
thread_local! {
    static CRASH_AFTER_FIRST_STEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Следующая смена только регистра в этом потоке пойдёт в два шага и
/// «упадёт» после первого — как завершение процесса (тесты).
#[cfg(test)]
pub fn crash_after_first_rename_step() {
    CRASH_AFTER_FIRST_STEP.with(|cell| cell.set(true));
}

/// Подмена [`crash_after_first_rename_step`] сработала (и сброшена).
pub fn take_crash_after_first_step() -> bool {
    #[cfg(test)]
    {
        CRASH_AFTER_FIRST_STEP.with(std::cell::Cell::take)
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[cfg(test)]
mod storage_lost_tests {
    use super::{storage_lost, test_volume};

    /// Решение 66: папка на месте — не «пропавший носитель» ни на каком томе.
    #[test]
    fn present_vault_is_never_lost() {
        let dir = tempfile::tempdir().expect("папка");
        test_volume::set_removable(dir.path());
        assert!(!storage_lost(dir.path()));
    }

    /// Папки нет на внешнем носителе — носитель пропал.
    #[test]
    fn missing_vault_on_removable_volume_is_lost() {
        let dir = tempfile::tempdir().expect("папка");
        let root = dir.path().join("Хранилище");
        test_volume::set_removable(&root);
        assert!(storage_lost(&root));
    }

    /// Папки нет на несъёмном томе Windows (переименовали, удалили) — не
    /// «пропавший носитель»: окно возвращается, как на внутреннем диске. На
    /// Linux и macOS типа тома нет — признак только «папка недоступна».
    #[test]
    fn missing_vault_on_fixed_volume_depends_on_os() {
        let dir = tempfile::tempdir().expect("папка");
        let root = dir.path().join("Переименовано");
        assert_eq!(storage_lost(&root), !cfg!(windows));
    }
}
