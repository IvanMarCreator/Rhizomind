//! Атомарная запись файлов хранилища (С4, ADR 0006).
//!
//! Новое содержимое пишется во временный файл в `.rhizomind/tmp/`
//! (`tempfile`), сбрасывается на диск (`sync_all`) и только потом заменяет
//! файл. Сбой до замены оставляет прежний файл целым; временный файл
//! удаляется сразу, а после аварийного завершения — при следующем открытии
//! хранилища (`Vault::open`).
//!
//! Замена существующего файла:
//! - **Windows** — `ReplaceFileW`: сохраняет дату создания, атрибуты и права
//!   заменяемого файла (дата создания — источник «самой старой» заметки,
//!   С3.4);
//! - **другие ОС** — `rename` (атомарен в пределах тома); права файла
//!   переносятся на новый файл. Дата создания на Linux не переносится —
//!   известное ограничение (ADR 0006, уточнение 25.10.2026).
//!
//! Новый файл (копия при конфликте, служебный файл) появляется только если
//! такого имени ещё нет: `persist_noclobber` — без затирания.

use std::io::{self, Write};
use std::path::Path;

use tempfile::{NamedTempFile, TempPath};

#[cfg(test)]
thread_local! {
    static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Подмена для тестов: следующая запись в этом потоке оборвётся после `n`
/// байт — как сбой диска или завершение процесса посреди записи.
#[cfg(test)]
pub fn fail_next_write_after(n: usize) {
    FAIL_AFTER.with(|cell| cell.set(Some(n)));
}

/// Пишет `bytes` во временный файл в `tmp_dir` и сбрасывает на диск.
/// Файл закрыт; при ошибке он уже удалён.
pub fn write_temp(tmp_dir: &Path, bytes: &[u8]) -> io::Result<TempPath> {
    let mut temp = NamedTempFile::new_in(tmp_dir)?;
    #[cfg(test)]
    if let Some(n) = FAIL_AFTER.with(std::cell::Cell::take) {
        temp.write_all(&bytes[..n.min(bytes.len())])?;
        return Err(io::Error::other("сбой посреди записи (тест)"));
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(temp.into_temp_path())
}

/// Заменяет существующий `target` временным файлом.
pub fn replace(temp: TempPath, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        replace_file_w(&temp, target)?;
        // Временного файла под этим именем больше нет: не удалять.
        let _ = temp.keep();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let permissions = std::fs::metadata(target)?.permissions();
        std::fs::set_permissions(&temp, permissions)?;
        temp.persist(target).map_err(|e| e.error)?;
        sync_parent(target);
        Ok(())
    }
}

/// Переносит временный файл в `target`, которого ещё нет; есть —
/// `AlreadyExists`, файл не затирается. `like` — файл, чьи права взять
/// (на Unix временный файл создаётся с правами 0600).
pub fn create_new(temp: TempPath, target: &Path, like: Option<&Path>) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = match like.map(std::fs::metadata) {
            Some(Ok(meta)) => meta.permissions(),
            _ => std::fs::Permissions::from_mode(0o644),
        };
        std::fs::set_permissions(&temp, permissions)?;
    }
    #[cfg(not(unix))]
    let _ = like;
    temp.persist_noclobber(target).map_err(|e| e.error)?;
    sync_parent(target);
    Ok(())
}

/// Сбрасывает на диск запись о переименовании в папке (Unix).
fn sync_parent(target: &Path) {
    #[cfg(unix)]
    if let Some(parent) = target.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = target;
}

#[cfg(windows)]
fn replace_file_w(temp: &Path, target: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};

    let target = wide_path(target);
    let temp = wide_path(temp);
    // SAFETY: строки заканчиваются нулём и живут до конца вызова;
    // необязательные указатели — null.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temp.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Путь для функций Win32 с префиксом `\\?\` (длинные пути), с нулём в конце.
#[cfg(windows)]
pub fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let text = path.as_os_str();
    let lossy = text.to_string_lossy();
    let mut wide: Vec<u16> = if lossy.starts_with(r"\\?\") {
        Vec::new()
    } else if let Some(unc) = lossy.strip_prefix(r"\\") {
        let mut prefix: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        prefix.extend(unc.encode_utf16());
        prefix.push(0);
        return prefix;
    } else if path.is_absolute() {
        r"\\?\".encode_utf16().collect()
    } else {
        Vec::new()
    };
    wide.extend(text.encode_wide());
    wide.push(0);
    wide
}

/// Подменённый результат `ReplaceFileW` (тесты).
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub enum ReplaceFailure {
    /// `ERROR_UNABLE_TO_MOVE_REPLACEMENT`.
    Code1176 { target_removed: bool },
    /// `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`.
    Code1177,
}

/// Следующая замена в этом потоке завершится подменённой ошибкой.
#[cfg(test)]
pub fn simulate_replace_failure(_failure: ReplaceFailure) {}

/// Вызвать `action` после записи временного файла, до замены (тесты).
#[cfg(test)]
pub fn after_temp_written(_action: Box<dyn FnOnce()>) {}
