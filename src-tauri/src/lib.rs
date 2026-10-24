//! Ядро Rhizomind.
//!
//! Главное окно создаётся здесь, в Rust, а не в `tauri.conf.json` (там окон нет):
//! только так задача 0.3 сможет настроить движок до создания окна — папку
//! данных во временной папке, InPrivate и флаги (ADR 0007).

use tauri::{AppHandle, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Метка главного окна; на неё выдаются права в `capabilities/main.json`.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Создаёт главное окно со страницей интерфейса из сборки (`dist/index.html`).
pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("Rhizomind")
        .inner_size(1200.0, 800.0)
        .min_inner_size(640.0, 480.0)
        .build()
}

/// Запуск приложения.
pub fn run() {
    tauri::Builder::default()
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
}
