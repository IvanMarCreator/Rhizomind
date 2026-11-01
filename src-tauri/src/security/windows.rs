//! Меры, которые есть только на Windows: SmartScreen, ускорители браузера и
//! меню движка в WebView2, отчёты Windows об ошибках (WER) для процесса
//! `rhizomind.exe`.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_13, ICoreWebView2Controller, ICoreWebView2Settings, ICoreWebView2Settings3,
    ICoreWebView2Settings8,
};
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode,
};
use windows::Win32::System::ErrorReporting::{
    WER_FAULT_REPORTING, WER_FAULT_REPORTING_FLAG_NOHEAP, WER_FAULT_REPORTING_NO_UI, WerSetFlags,
};
use windows::core::{BOOL, Interface};

/// Ограничивает отчёты Windows об ошибках (WER) для процесса приложения.
///
/// - `SetErrorMode(SEM_NOGPFAULTERRORBOX | …)`: по документации Microsoft
///   система не вызывает WER при необработанном исключении и не показывает
///   окна ошибок. Режим наследуют дочерние процессы, в том числе процессы
///   WebView2 (у них свой обработчик падений Crashpad, см. правку wry).
/// - `WerSetFlags(NOHEAP | NO_UI)`: если отчёт всё же создаётся (например,
///   `__fastfail`, который режим ошибок не перехватывает), в дамп не попадает
///   куча — там лежит текст заметок — и окно WER не показывается.
///
/// Реестр и файлы не трогаются: исключение приложения из WER целиком
/// (`WerAddExcludedApplication`) пишет в профиль пользователя, это политика
/// службы ИТ (`docs/SECURITY.md`).
pub fn limit_error_reporting() {
    // SAFETY: функции меняют только настройки текущего процесса.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        let _ = WerSetFlags(WER_FAULT_REPORTING(
            WER_FAULT_REPORTING_FLAG_NOHEAP.0 | WER_FAULT_REPORTING_NO_UI,
        ));
    }
}

/// Текущий режим ошибок процесса (для тестов).
pub fn error_mode() -> u32 {
    // SAFETY: GetErrorMode только читает настройку процесса.
    unsafe { windows::Win32::System::Diagnostics::Debug::GetErrorMode() }
}

fn settings8(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<ICoreWebView2Settings8> {
    // SAFETY: вызовы COM на главном потоке, где wry создал контроллер.
    unsafe { controller.CoreWebView2()?.Settings()?.cast() }
}

/// Выключает SmartScreen (проверку адресов на серверах Microsoft):
/// `IsReputationCheckingRequired = false`. В Tauri и wry этой настройки нет.
pub fn disable_smartscreen(controller: &ICoreWebView2Controller) -> windows::core::Result<()> {
    // SAFETY: см. settings8.
    unsafe { settings8(controller)?.SetIsReputationCheckingRequired(false) }
}

/// Значение `IsReputationCheckingRequired` (для интеграционного теста).
pub fn is_reputation_checking_required(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<bool> {
    let mut value = BOOL::default();
    // SAFETY: см. settings8; value — локальная переменная.
    unsafe { settings8(controller)?.IsReputationCheckingRequired(&mut value)? };
    Ok(value.as_bool())
}

fn settings(controller: &ICoreWebView2Controller) -> windows::core::Result<ICoreWebView2Settings> {
    // SAFETY: вызовы COM на главном потоке, где wry создал контроллер.
    unsafe { controller.CoreWebView2()?.Settings() }
}

/// Выключает ускорители браузера (решение владельца 94):
/// `ICoreWebView2Settings3::AreBrowserAcceleratorKeysEnabled = false`. По
/// документации Microsoft это сочетания возможностей браузера — печать
/// (Ctrl+P), поиск на странице (Ctrl+F, F3), перезагрузка (Ctrl+R, F5),
/// масштаб (Ctrl+плюс/минус), средства разработчика (F12, Ctrl+Shift+C),
/// клавиши «Назад», «Вперёд», «Поиск»; клавиши перемещения и правки текста
/// (Home/End, Ctrl+X/C/V, Ctrl+A, Ctrl+Z) остаются. Нажатие по-прежнему
/// доходит до страницы — команды реестра горячих клавиш работают. Tauri этой
/// настройки не открывает (у wry она есть, но Tauri её не передаёт).
/// Запасной рубеж в интерфейсе — `src/shell/engineShortcuts.ts`.
pub fn disable_browser_accelerator_keys(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<()> {
    // SAFETY: см. settings.
    unsafe {
        settings(controller)?
            .cast::<ICoreWebView2Settings3>()?
            .SetAreBrowserAcceleratorKeysEnabled(false)
    }
}

/// Выключает стандартное контекстное меню движка (решение 97):
/// `AreDefaultContextMenusEnabled = false`. В нём «Обновить», «Сохранить как»,
/// «Печать» — обход решений 94/96 и риск потери ввода. Tauri этой настройки не
/// передаёт (у wry `with_default_context_menus`, по умолчанию `true`). Своё
/// меню правки текста — `src/shell/textMenu.ts`, меню дерева —
/// `src/tree/FileTree.svelte`.
pub fn disable_default_context_menus(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<()> {
    // SAFETY: см. settings.
    unsafe { settings(controller)?.SetAreDefaultContextMenusEnabled(false) }
}

/// Значение `AreDefaultContextMenusEnabled` (для интеграционного теста).
pub fn are_default_context_menus_enabled(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<bool> {
    let mut value = BOOL::default();
    // SAFETY: см. settings; value — локальная переменная.
    unsafe { settings(controller)?.AreDefaultContextMenusEnabled(&mut value)? };
    Ok(value.as_bool())
}

/// Значение `AreBrowserAcceleratorKeysEnabled` (для интеграционного теста).
pub fn are_browser_accelerator_keys_enabled(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<bool> {
    let mut value = BOOL::default();
    // SAFETY: см. settings; value — локальная переменная.
    unsafe {
        settings(controller)?
            .cast::<ICoreWebView2Settings3>()?
            .AreBrowserAcceleratorKeysEnabled(&mut value)?
    };
    Ok(value.as_bool())
}

/// Значение `IsZoomControlEnabled` — масштаб страницы сочетаниями и
/// Ctrl+колесом (для интеграционного теста; выключает его
/// `zoom_hotkeys_enabled(false)` при создании окна).
pub fn is_zoom_control_enabled(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<bool> {
    let mut value = BOOL::default();
    // SAFETY: см. settings; value — локальная переменная.
    unsafe { settings(controller)?.IsZoomControlEnabled(&mut value)? };
    Ok(value.as_bool())
}

/// Включён ли режим InPrivate у профиля движка (`ICoreWebView2_13::Profile`,
/// `ICoreWebView2Profile::IsInPrivateModeEnabled`). Ошибка — движок слишком
/// старый, чтобы ответить (тогда и InPrivate wry включить не смог).
pub fn is_in_private_mode_enabled(
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<bool> {
    let mut value = BOOL::default();
    // SAFETY: COM-вызовы у живого контроллера в главном потоке (with_webview);
    // value — локальная переменная.
    unsafe {
        controller
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .IsInPrivateModeEnabled(&mut value)?
    };
    Ok(value.as_bool())
}

/// Системное окно с сообщением об ошибке (до открытия интерфейса).
pub fn show_error(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;
    // SAFETY: строки живут до конца вызова; окна-владельца нет.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from("Rhizomind"),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Номер процесса браузера WebView2 (`BrowserProcessId`): ядро ждёт его
/// завершения перед удалением папки данных.
pub fn browser_process_id(controller: &ICoreWebView2Controller) -> windows::core::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: вызов COM на главном потоке; pid — локальная переменная.
    unsafe { controller.CoreWebView2()?.BrowserProcessId(&mut pid)? };
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mode_suppresses_wer_and_error_boxes() {
        limit_error_reporting();
        let mode = error_mode();
        for flag in [
            SEM_FAILCRITICALERRORS,
            SEM_NOGPFAULTERRORBOX,
            SEM_NOOPENFILEERRORBOX,
        ] {
            assert_eq!(mode & flag.0, flag.0, "режим ошибок {mode:#x}");
        }
    }
}
