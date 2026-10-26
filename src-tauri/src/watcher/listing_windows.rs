//! Список папки на Windows одним дескриптором самой папки (задача 0.10).
//!
//! На NTFS папку нельзя переименовать или перенести, пока открыт хоть один
//! дескриптор чего-то внутри неё — файла или вложенной папки, при любом
//! режиме совместного доступа (Windows отвечает «Отказано в доступе», а
//! Проводник — «Папка уже используется»). Проверено на раннере Windows
//! (`docs/reviews/2026-10-26-task-0.10.md`). Прежний обход открывал каждый
//! файл ради его идентификатора (`file_id::get_file_id`), пока читал папку, и
//! держал папку «занятой» всё это время. Здесь размер, дата изменения,
//! атрибуты и идентификатор каждого элемента приходят из списка папки
//! (`GetFileInformationByHandleEx`, класс `FileIdExtdDirectoryInfo`):
//! открыта только сама папка и только на время чтения списка. Перенос её
//! родителя в это мгновение всё ещё не удастся — как при работе индексатора
//! поиска или антивируса; окно сократилось с «по открытию на каждый файл» до
//! одного чтения списка.
//!
//! Файловая система без этого класса (FAT32, exFAT, часть сетевых) — `None`:
//! список читается прежним способом.

use std::fs;
use std::io;
use std::mem::size_of;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use file_id::FileId;
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, ERROR_NOT_SUPPORTED,
    HANDLE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES,
    FileIdExtdDirectoryInfo, FileIdExtdDirectoryRestartInfo, FileIdInfo,
    GetFileInformationByHandleEx,
};

/// Метка точки повторного разбора «суррогат имени»: символическая ссылка,
/// точка соединения. Так же их отличает `std` (`FileType::is_symlink`).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// Разница между началом отсчёта FILETIME (1601) и Unix (1970), в 100 нс.
const UNIX_EPOCH_FILETIME: i64 = 116_444_736_000_000_000;

/// Элемент списка папки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub id: FileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Folder,
    /// Символическая ссылка или точка соединения: за них модель не ходит.
    Link,
}

/// Список папки `dir`. `Ok(None)` — файловая система не отдаёт
/// идентификаторы в списке; читать прежним способом. Имена не в UTF-16
/// без пары (не выражаются в `String`) пропускаются, как и раньше.
pub(super) fn list(dir: &Path) -> io::Result<Option<Vec<Entry>>> {
    let folder = fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?;
    let handle = folder.as_raw_handle() as HANDLE;

    let mut volume: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: дескриптор открыт; `volume` — локальная структура своего размера.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut volume as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return unsupported_or(io::Error::last_os_error());
    }

    // Записи выровнены по 8 байт: буфер из u64.
    let mut buffer = vec![0u64; 8 * 1024];
    let buffer_bytes = (buffer.len() * size_of::<u64>()) as u32;
    let mut entries = Vec::new();
    let mut class = FileIdExtdDirectoryRestartInfo;
    loop {
        // SAFETY: дескриптор открыт; буфер принадлежит нам, размер передан.
        let ok = unsafe {
            GetFileInformationByHandleEx(handle, class, buffer.as_mut_ptr().cast(), buffer_bytes)
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                break;
            }
            if class == FileIdExtdDirectoryRestartInfo {
                return unsupported_or(error);
            }
            return Err(error);
        }
        class = FileIdExtdDirectoryInfo;
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            // SAFETY: ОС записала в буфер цепочку записей; `offset` — от
            // `NextEntryOffset` предыдущей записи, внутри записанного.
            let info = unsafe { &*base.add(offset).cast::<FILE_ID_EXTD_DIR_INFO>() };
            // SAFETY: имя лежит сразу в записи, длина — `FileNameLength` байт.
            let name = unsafe {
                std::slice::from_raw_parts(
                    std::ptr::addr_of!(info.FileName).cast::<u16>(),
                    info.FileNameLength as usize / 2,
                )
            };
            if let Some(entry) = entry(name, info, volume.VolumeSerialNumber) {
                entries.push(entry);
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
    }
    drop(folder);
    Ok(Some(entries))
}

fn entry(name: &[u16], info: &FILE_ID_EXTD_DIR_INFO, volume: u64) -> Option<Entry> {
    let name = String::from_utf16(name).ok()?;
    if name == "." || name == ".." {
        return None;
    }
    let attributes = info.FileAttributes;
    let link = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && info.ReparsePointTag & NAME_SURROGATE != 0;
    let kind = if link {
        Kind::Link
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Kind::Folder
    } else {
        Kind::File
    };
    Some(Entry {
        name,
        kind,
        size: u64::try_from(info.EndOfFile).unwrap_or(0),
        modified: filetime(info.LastWriteTime),
        id: FileId::new_high_res(volume, u128::from_le_bytes(info.FileId.Identifier)),
    })
}

/// FILETIME (100 нс с 1601 года) → `SystemTime`, то же значение, что даёт
/// `std::fs::Metadata::modified` на Windows.
fn filetime(value: i64) -> Option<SystemTime> {
    let from_unix = value.checked_sub(UNIX_EPOCH_FILETIME)?;
    let nanos = |intervals: i64| {
        intervals
            .unsigned_abs()
            .checked_mul(100)
            .map(Duration::from_nanos)
    };
    if from_unix >= 0 {
        UNIX_EPOCH.checked_add(nanos(from_unix)?)
    } else {
        UNIX_EPOCH.checked_sub(nanos(from_unix)?)
    }
}

fn unsupported_or<T>(error: io::Error) -> io::Result<Option<T>> {
    match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION) => Ok(None),
        _ => Err(error),
    }
}
