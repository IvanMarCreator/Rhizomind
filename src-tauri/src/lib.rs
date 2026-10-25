//! Ядро Rhizomind.
//!
//! Главное окно создаётся здесь, в Rust, а не в `tauri.conf.json` (там окон нет):
//! только так задача 0.3 сможет настроить движок до создания окна — папку
//! данных во временной папке, InPrivate и флаги (ADR 0007).

use tauri::{AppHandle, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

// Модули ядра (спецификация 0.1, С5.2) и команды API v1 (С5.5) — задача 0.4.
pub mod commands;
pub mod external_links;
pub mod links;
pub mod names;
pub mod registry;
pub mod switcher;
pub mod vault;
pub mod watcher;

/// Метка главного окна; на неё выдаются права в `capabilities/main.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Флаги WebView2, которые wry 0.57 передаёт по умолчанию (`src/webview2/mod.rs`).
const WRY_DEFAULT_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Флаги движка для сборки сценарных тестов (функция `webdriver`).
///
/// msedgedriver просит WebView2 открыть порт отладки через переменную
/// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, но WebView2 её не применяет, если
/// приложение задаёт флаги само (wry задаёт всегда). Поэтому тестовая сборка
/// добавляет флаги драйвера к флагам wry. Выпускные сборки собираются без
/// функции `webdriver` и переменную не читают.
#[cfg_attr(not(all(windows, feature = "webdriver")), allow(dead_code))]
fn webdriver_browser_args(from_driver: Option<&str>) -> Option<String> {
    let extra = from_driver?.trim();
    if extra.is_empty() {
        return None;
    }
    Some(format!("{WRY_DEFAULT_BROWSER_ARGS} {extra}"))
}

/// Создаёт главное окно со страницей интерфейса из сборки (`dist/index.html`).
pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let builder =
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("Rhizomind")
            .inner_size(1200.0, 800.0)
            .min_inner_size(640.0, 480.0);

    #[cfg(all(windows, feature = "webdriver"))]
    let builder = match webdriver_browser_args(
        std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
            .ok()
            .as_deref(),
    ) {
        Some(args) => builder.additional_browser_args(&args),
        None => builder,
    };

    builder.build()
}

/// Запуск приложения.
pub fn run() {
    tauri::Builder::default()
        .manage(commands::Core::new(registry::Registry::from_profile()))
        .invoke_handler(commands::handler())
        .setup(|app| {
            create_main_window(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить Rhizomind");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    #[test]
    fn main_window_is_created_in_rust_with_label_from_capabilities() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("тестовое приложение");

        let window = create_main_window(app.handle()).expect("главное окно");

        // Подменный runtime Tauri не хранит заголовок окна: заголовок и страницу
        // проверяет сценарный тест (tests/e2e/).
        assert_eq!(window.label(), "main");
        assert!(app.get_webview_window(MAIN_WINDOW_LABEL).is_some());
    }

    #[test]
    fn webdriver_args_extend_wry_defaults_with_driver_args() {
        assert_eq!(
            webdriver_browser_args(Some("--remote-debugging-port=9222 --user-data-dir=C:\\t")),
            Some(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
                 --remote-debugging-port=9222 --user-data-dir=C:\\t"
                    .to_owned()
            )
        );
    }

    #[test]
    fn webdriver_args_absent_without_driver() {
        assert_eq!(webdriver_browser_args(None), None);
        assert_eq!(webdriver_browser_args(Some("  ")), None);
    }
}
