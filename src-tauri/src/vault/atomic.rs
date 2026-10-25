//! Атомарная запись файлов хранилища (С4, ADR 0006).
//!
//! Новое содержимое пишется во временный файл в `.rhizomind/tmp/`
//! (`tempfile`). Сразу после создания и **до записи байтов** итоговый путь
//! временного файла проверяется по дескриптору: он внутри `.rhizomind/tmp/`
//! этого хранилища (папку могли подменить ссылкой после открытия). Затем
//! байты сбрасываются на диск (`sync_all`) и временный файл заменяет файл.
//! Сбой до замены оставляет прежний файл целым.
//!
//! Замена существующего файла:
//! - **Windows** — `ReplaceFileW` с резервным именем в `.rhizomind/tmp/`:
//!   сохраняет дату создания, атрибуты и права заменяемого файла. Ошибки
//!   1176 и 1177 (`ERROR_UNABLE_TO_MOVE_REPLACEMENT[_2]`) не теряют данные:
//!   [`recover`] переносит новый текст на место заметки или возвращает
//!   прежний; если не вышло ни то, ни другое, файлы остаются в `tmp` и
//!   восстанавливаются при следующем открытии хранилища;
//! - **другие ОС** — `rename` (атомарен в пределах тома); права файла
//!   переносятся на новый файл. Дата создания на Linux не переносится —
//!   известное ограничение (ADR 0006, уточнение 25.10.2026).
//!
//! Новый файл (копия при конфликте, служебный файл) появляется только если
//! такого имени ещё нет: `persist_noclobber` — без затирания.
//!
//! Имена временных файлов: `.note-*` — текст заметки, `.svc-*` — служебный
//! файл. Остатки `.note-*` после аварийного завершения не удаляются, а
//! восстанавливаются заметкой (`Vault::open`).

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::{Builder, TempPath};

use super::fsops;

/// Что во временном файле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Текст заметки — остаток восстанавливается.
    Note,
    /// Служебный файл `.rhizomind/` — остаток удаляется.
    Service,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Note => NOTE_PREFIX,
            Kind::Service => ".svc-",
        }
    }
}

/// Начало имени временного файла с текстом заметки.
pub const NOTE_PREFIX: &str = ".note-";

/// Временный файл оказался не там, где должен (папку подменили ссылкой).
#[derive(Debug)]
pub struct OutsideTmp;

impl std::fmt::Display for OutsideTmp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("временный файл вне .rhizomind/tmp")
    }
}

impl std::error::Error for OutsideTmp {}

/// Ошибка ввода-вывода — это [`OutsideTmp`].
pub fn is_outside(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|e| e.is::<OutsideTmp>())
}

#[cfg(test)]
thread_local! {
    static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static SIMULATED: std::cell::Cell<Option<ReplaceFailure>> = const { std::cell::Cell::new(None) };
    static AFTER_TEMP: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

/// Подмена для тестов: следующая запись в этом потоке оборвётся после `n`
/// байт — как сбой диска или завершение процесса посреди записи.
#[cfg(test)]
pub fn fail_next_write_after(n: usize) {
    FAIL_AFTER.with(|cell| cell.set(Some(n)));
}

/// Подменённый результат `ReplaceFileW` (тесты на всех ОС).
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub enum ReplaceFailure {
    /// `ERROR_UNABLE_TO_MOVE_REPLACEMENT`: замена не перенесена; заменяемый
    /// файл на месте или (по документации без резервного имени) удалён.
    Code1176 { target_removed: bool },
    /// `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`: заменяемый файл — под резервным
    /// именем, новый — под временным.
    Code1177,
}

/// Следующая замена в этом потоке завершится подменённой ошибкой.
#[cfg(test)]
pub fn simulate_replace_failure(failure: ReplaceFailure) {
    SIMULATED.with(|cell| cell.set(Some(failure)));
}

/// Вызвать `action` после записи временного файла, до замены (тесты).
#[cfg(test)]
pub fn after_temp_written(action: Box<dyn FnOnce()>) {
    AFTER_TEMP.with(|cell| *cell.borrow_mut() = Some(action));
}

/// Точка вызова подмены [`after_temp_written`].
pub fn temp_written_hook() {
    #[cfg(test)]
    if let Some(action) = AFTER_TEMP.with(|cell| cell.borrow_mut().take()) {
        action();
    }
}

/// Пишет `bytes` во временный файл в `tmp_dir` и сбрасывает на диск.
/// `inside` проверяет открытый файл по дескриптору до записи байтов;
/// `false` — файл удаляется, ошибка [`OutsideTmp`]. Файл закрыт; при
/// ошибке он уже удалён.
pub fn write_temp(
    tmp_dir: &Path,
    kind: Kind,
    bytes: &[u8],
    inside: &dyn Fn(&File, &Path) -> bool,
) -> io::Result<TempPath> {
    match std::fs::symlink_metadata(tmp_dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(io::Error::other(OutsideTmp));
        }
        _ => {}
    }
    let mut temp = Builder::new().prefix(kind.prefix()).tempfile_in(tmp_dir)?;
    if !inside(temp.as_file(), temp.path()) {
        return Err(io::Error::other(OutsideTmp));
    }
    #[cfg(test)]
    if let Some(n) = FAIL_AFTER.with(std::cell::Cell::take) {
        temp.write_all(&bytes[..n.min(bytes.len())])?;
        return Err(io::Error::other("сбой посреди записи (тест)"));
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(temp.into_temp_path())
}

/// Заменяет существующий `target` временным файлом. Резервное имя для
/// `ReplaceFileW` — рядом с временным файлом.
pub fn replace(temp: TempPath, target: &Path) -> io::Result<()> {
    let backup = backup_path(&temp);
    #[cfg(test)]
    if let Some(failure) = SIMULATED.with(std::cell::Cell::take) {
        let code = match failure {
            ReplaceFailure::Code1176 { target_removed } => {
                if target_removed {
                    std::fs::remove_file(target)?;
                }
                1176
            }
            ReplaceFailure::Code1177 => {
                std::fs::rename(target, &backup)?;
                1177
            }
        };
        return recover(code, temp, target, &backup);
    }
    #[cfg(windows)]
    {
        match replace_file_w(&temp, target, &backup) {
            Ok(()) => {
                // Временного файла под этим именем больше нет: не удалять.
                let _ = temp.keep();
                let _ = std::fs::remove_file(&backup);
                Ok(())
            }
            Err(error) => match error.raw_os_error() {
                Some(code @ (1176 | 1177)) => recover(code as u32, temp, target, &backup),
                _ => Err(error),
            },
        }
    }
    #[cfg(not(windows))]
    {
        let _ = backup;
        let permissions = std::fs::metadata(target)?.permissions();
        std::fs::set_permissions(&temp, permissions)?;
        temp.persist(target).map_err(|e| e.error)?;
        sync_parent(target);
        Ok(())
    }
}

/// Резервное имя для замены: `.note-bak-…` рядом с временным файлом.
fn backup_path(temp: &Path) -> PathBuf {
    let name = temp
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = name
        .strip_prefix(NOTE_PREFIX)
        .or_else(|| name.strip_prefix(".svc-"))
        .unwrap_or(&name);
    let prefix = if name.starts_with(NOTE_PREFIX) {
        ".note-bak-"
    } else {
        ".svc-bak-"
    };
    temp.with_file_name(format!("{prefix}{suffix}"))
}

/// Спасение после ошибки `ReplaceFileW` 1176/1177: новый текст — на место
/// `target`; не вышло — прежний файл на место, временный файл остаётся в
/// `tmp` (его восстановит следующее открытие). Ошибка — только после
/// попытки спасения.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn recover(code: u32, temp: TempPath, target: &Path, backup: &Path) -> io::Result<()> {
    let moved = if code == 1176 && std::fs::symlink_metadata(target).is_ok() {
        // Заменяемый файл на месте: обычная замена (дата создания может
        // смениться — редкий случай, данные важнее).
        std::fs::rename(&temp, target)
    } else {
        fsops::rename_noreplace(&temp, target)
    };
    match moved {
        Ok(()) => {
            let _ = temp.keep();
            let _ = std::fs::remove_file(backup);
            sync_parent(target);
            Ok(())
        }
        Err(error) => {
            // Не удалять ни новый текст, ни прежний файл.
            let _ = temp.keep();
            if std::fs::symlink_metadata(target).is_err()
                && std::fs::symlink_metadata(backup).is_ok()
            {
                let _ = fsops::rename_noreplace(backup, target);
            }
            Err(error)
        }
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
fn replace_file_w(temp: &Path, target: &Path, backup: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};

    let target = wide_path(target);
    let temp = wide_path(temp);
    let backup = wide_path(backup);
    // SAFETY: строки заканчиваются нулём и живут до конца вызова;
    // необязательные указатели — null.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temp.as_ptr(),
            backup.as_ptr(),
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
