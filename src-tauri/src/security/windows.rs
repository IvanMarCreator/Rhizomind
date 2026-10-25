//! Меры, которые есть только на Windows: SmartScreen в WebView2 и отчёты
//! Windows об ошибках (WER) для процесса `rhizomind.exe`.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Settings8,
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
