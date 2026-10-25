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

/// Корзина ОС в отдельном потоке (COM на Windows, D-Bus-независимая
/// freedesktop-корзина на Linux). На Windows сначала проверяется тип тома:
/// корзина есть только на несъёмных локальных дисках — на флешке и в
/// сетевой папке оболочка удалила бы файл безвозвратно сама.
pub fn os_trash(path: &Path) -> Result<(), TrashFailure> {
    #[cfg(windows)]
    if !volume_has_recycle_bin(path) {
        return Err(TrashFailure::NoTrash);
    }
    let owned = path.to_path_buf();
    match std::thread::spawn(move || trash::delete(&owned)).join() {
        Ok(Ok(())) => Ok(()),
        _ => Err(TrashFailure::NoTrash),
    }
}

/// Том несъёмный локальный (`DRIVE_FIXED`): у таких томов есть корзина.
#[cfg(windows)]
fn volume_has_recycle_bin(path: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    const DRIVE_FIXED: u32 = 3;
    let wide = super::atomic::wide_path(path);
    let mut volume = vec![0u16; 1024];
    // SAFETY: строки с нулём в конце; буфер длиной `volume.len()`.
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) };
    if ok == 0 {
        return false;
    }
    // SAFETY: `volume` — строка с нулём в конце от GetVolumePathNameW.
    unsafe { GetDriveTypeW(volume.as_ptr()) == DRIVE_FIXED }
}
