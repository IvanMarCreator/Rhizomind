//! Передача подтверждённого адреса ОС.
//!
//! - **Windows:** свой вызов `ShellExecuteExW` с классом протокола
//!   (`SEE_MASK_CLASSNAME`, `lpClass` = схема): адрес разрешается только
//!   через обработчик протокола `http`/`https`/`mailto` и никогда как путь к
//!   файлу; без PowerShell и `cmd.exe`, без окон оболочки
//!   (`SEE_MASK_FLAG_NO_UI`). Отказ — ошибка, и ответ команды тогда не
//!   `opened`.
//! - **macOS, Linux:** крейт `open` (`open` / `xdg-open` и др.).

use std::io;

/// Схема адреса — имя класса протокола для `ShellExecuteExW`. Адрес уже
/// проверен: схема — одна из [`super::ALLOWED_SCHEMES`].
pub(super) fn scheme_of(target: &str) -> io::Result<&str> {
    target
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))
}

/// Передаёт адрес обработчику протокола в ОС.
#[cfg(windows)]
pub(super) fn launch(target: &str) -> io::Result<()> {
    shell_execute(target, scheme_of(target)?)
}

/// Передаёт адрес обработчику протокола в ОС.
#[cfg(not(windows))]
pub(super) fn launch(target: &str) -> io::Result<()> {
    scheme_of(target)?;
    open::that_detached(target)
}

/// `ShellExecuteExW` с глаголом `open` и классом `class`. Вызывается в
/// блокирующем потоке (`spawn_blocking`); COM инициализируется на время
/// вызова, как требует документация `ShellExecuteEx`.
#[cfg(windows)]
pub(super) fn shell_execute(target: &str, class: &str) -> io::Result<()> {
    use std::ptr;
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_CLASSNAME, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };

    /// `SW_SHOWNORMAL` из `Win32_UI_WindowsAndMessaging` — без лишней функции крейта.
    const SW_SHOWNORMAL: i32 = 1;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    if target.contains('\0') || class.contains('\0') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let verb = wide("open");
    let file = wide(target);
    let class = wide(class);

    // SAFETY: обычная инициализация COM в текущем потоке; при успехе
    // (S_OK, S_FALSE) парный CoUninitialize ниже. RPC_E_CHANGED_MODE —
    // поток уже инициализирован иначе, тогда освобождать не нужно.
    let com = unsafe {
        CoInitializeEx(
            ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };

    // SAFETY: структура без указателей-владельцев; нули — допустимые
    // значения всех полей. Строки живут до конца функции.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI | SEE_MASK_CLASSNAME;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpClass = class.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    // SAFETY: `info` заполнена по документации, указатели действительны.
    let ok = unsafe { ShellExecuteExW(&mut info) } != 0;
    let result = if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    if com >= 0 {
        // SAFETY: парный вызов к успешному CoInitializeEx в этом потоке.
        unsafe { CoUninitialize() };
    }
    result
}
