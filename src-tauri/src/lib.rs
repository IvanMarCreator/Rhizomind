//! Ядро Rhizomind.
//!
//! Главное окно создаётся здесь, в Rust, а не в `tauri.conf.json` (там окон
//! нет): только так движку до создания окна задаются временная папка данных,
//! режим InPrivate и флаги (ADR 0007). Меры защищённой оболочки — в
//! `security` и `webview_traces`.

// Сборка «разработки» (`cfg(dev)`) грузит интерфейс с сервера разработки
// `http://localhost:5173`. Она бывает только отладочной (`tauri dev`). Голый
// `cargo build --release` без tauri CLI тоже дал бы `cfg(dev)` — такую сборку
// запрещаем: выпуск собирается только через `npm run build` / `package:*`.
#[cfg(all(dev, not(debug_assertions)))]
compile_error!(
    "Выпускная сборка без tauri CLI грузила бы интерфейс с http://localhost:5173. \
     Собирайте через `npm run build` или `npm run package:*`."
);

pub mod security;
pub mod webview_traces;

use std::path::Path;

use tauri::{
    AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    webview::{NewWindowResponse, PermissionResponse},
};

use webview_traces::WebviewDataDir;

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

/// Пометка в заголовке окна тестовой сборки (функция `webdriver`). По этой же
/// строке `tools/package-portable` отказывается упаковывать тестовую сборку.
pub const WEBDRIVER_BUILD_MARKER: &str = "ТЕСТОВАЯ СБОРКА (webdriver)";

/// Заголовок главного окна. Пометка тестовой сборки — через `#[cfg]`, чтобы
/// её строки не было в выпускном исполняемом файле.
#[cfg(feature = "webdriver")]
pub fn main_window_title() -> String {
    format!("Rhizomind — {WEBDRIVER_BUILD_MARKER}")
}

/// Заголовок главного окна.
#[cfg(not(feature = "webdriver"))]
pub fn main_window_title() -> String {
    "Rhizomind".to_owned()
}

/// Настройки движка этого запуска: папка данных и строка флагов.
#[derive(Debug)]
pub struct Shell {
    data_dir: WebviewDataDir,
    browser_args: String,
    /// Тестовая сборка: папка данных msedgedriver вместо временной.
    driver_data_dir: Option<std::path::PathBuf>,
}

impl Shell {
    /// Готовит процесс до создания окна. Вызывается первым делом в `main`,
    /// пока в процессе один поток:
    /// 1. тестовая сборка читает порт отладки и папку данных msedgedriver;
    /// 2. из окружения удаляются `WEBVIEW2_*` (и `TAURI_WEBVIEW_AUTOMATION`
    ///    вне тестовой сборки);
    /// 3. Windows: ограничиваются отчёты WER;
    /// 4. убираются остатки прошлых запусков и создаётся временная папка данных.
    pub fn prepare() -> std::io::Result<Self> {
        let debug_port = webdriver_debug_port_from_env();
        let driver_data_dir = webdriver_user_data_folder_from_env();
        security::remove_webview_overrides(cfg!(feature = "webdriver"));
        #[cfg(windows)]
        security::windows::limit_error_reporting();
        let mut shell = Self::new(
            WebviewDataDir::prepare()?,
            security::browser_args_with_debug_port(debug_port),
        );
        shell.driver_data_dir = driver_data_dir;
        Ok(shell)
    }

    pub fn new(data_dir: WebviewDataDir, browser_args: String) -> Self {
        Self {
            data_dir,
            browser_args,
            driver_data_dir: None,
        }
    }

    /// Временная папка данных этого запуска.
    pub fn data_dir(&self) -> &Path {
        self.data_dir.path()
    }

    /// Папка, которую получает движок: временная, а в тестовой сборке под
    /// msedgedriver — папка драйвера (в ней он ищет `DevToolsActivePort`).
    pub fn engine_data_dir(&self) -> &Path {
        self.driver_data_dir
            .as_deref()
            .unwrap_or(self.data_dir.path())
    }

    pub fn browser_args(&self) -> &str {
        &self.browser_args
    }

    /// После выхода: дождаться процесса браузера и удалить папку данных.
    pub fn clean_up(&self) -> std::io::Result<()> {
        self.data_dir
            .remove_after_exit(webview_traces::EXIT_TIMEOUT)
    }
}

/// Порт отладки для msedgedriver — только в тестовой сборке (функция `webdriver`).
#[cfg(feature = "webdriver")]
fn webdriver_debug_port_from_env() -> Option<u16> {
    let value = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").ok();
    security::webdriver_debug_port(value.as_deref())
}

#[cfg(not(feature = "webdriver"))]
fn webdriver_debug_port_from_env() -> Option<u16> {
    None
}

/// Папка данных msedgedriver — только в тестовой сборке (функция `webdriver`).
#[cfg(feature = "webdriver")]
fn webdriver_user_data_folder_from_env() -> Option<std::path::PathBuf> {
    let folder = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
    // [DEBUG-t03] какие переменные WEBVIEW2_* передал драйвер (только имена).
    if let Some(dir) = std::env::var_os("RHIZOMIND_E2E_LOG_DIR") {
        let names: Vec<String> = std::env::vars_os()
            .filter_map(|(k, _)| k.into_string().ok())
            .filter(|k| k.to_ascii_uppercase().starts_with("WEBVIEW2_"))
            .collect();
        let _ = std::fs::write(
            std::path::Path::new(&dir).join(format!("webdriver-env-{}.txt", std::process::id())),
            format!("[DEBUG-t03] {names:?}\n"),
        );
    }
    security::webdriver_user_data_folder(folder.as_deref())
}

#[cfg(not(feature = "webdriver"))]
fn webdriver_user_data_folder_from_env() -> Option<std::path::PathBuf> {
    None
}

/// Создаёт главное окно со страницей интерфейса из сборки (`dist/index.html`)
/// и защищёнными настройками движка. `Shell` должен быть в состоянии приложения.
pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let shell = app.state::<Shell>();
    let dev_url = if cfg!(dev) {
        app.config().build.dev_url.clone()
    } else {
        None
    };

    let window =
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title(main_window_title())
            .inner_size(1200.0, 800.0)
            .min_inner_size(640.0, 480.0)
            // Кэш, cookies и хранилища страницы — только в памяти (InPrivate);
            // исключение — тестовая сборка Linux (security::incognito).
            .incognito(security::incognito())
            // Служебная папка движка — во временной папке ОС, не в %LOCALAPPDATA%.
            .data_directory(shell.engine_data_dir().to_path_buf())
            .additional_browser_args(shell.browser_args())
            // Автозаполнение форм (в Tauri по умолчанию включено).
            .general_autofill_enabled(false)
            // Переходы — только по адресам своего интерфейса.
            .on_navigation(move |url| security::is_allowed_navigation(url, dev_url.as_ref()))
            // Новые окна (`window.open`, `target=_blank`) — запрет.
            .on_new_window(|_url, _features| NewWindowResponse::Deny)
            // Загрузки файлов — запрет.
            .on_download(|_webview, _event| false)
            // Разрешения браузера: камера, микрофон, геолокация, уведомления,
            // буфер обмена, файловая система, MIDI и др. — всё отклоняется.
            .on_permission_request(|_webview, _kind| PermissionResponse::Deny)
            .build()?;

    harden_platform_webview(&window);
    Ok(window)
}

/// Настройки WebView2, которых нет в Tauri: SmartScreen выключается, номер
/// процесса браузера запоминается для удаления папки данных после выхода.
fn harden_platform_webview<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let app = window.app_handle().clone();
        let _ = window.with_webview(move |webview| {
            let controller = webview.controller();
            if let Err(error) = security::windows::disable_smartscreen(&controller) {
                eprintln!("Rhizomind: не удалось выключить SmartScreen WebView2: {error}");
            }
            if let Ok(pid) = security::windows::browser_process_id(&controller) {
                app.state::<Shell>().data_dir.set_browser_pid(pid);
            }
        });
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Обработка событий приложения: после выхода удаляется папка данных движка.
///
/// Приложение завершается закрытием окон, а не `AppHandle::exit`: при `exit`
/// событие `Exit` приходит, пока WebView и процесс браузера ещё живы, — папка
/// занята и останется до уборки остатков при следующем запуске. Будущая команда
/// «Выход» должна закрывать окна (проверка — `tests/webview2_runtime.rs`).
pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if let RunEvent::Exit = event
        && let Some(shell) = app.try_state::<Shell>()
    {
        // Не удалось — папку уберёт следующий запуск (уборка остатков).
        let _ = shell.clean_up();
    }
}

/// Конфигурация и встроенный интерфейс приложения.
pub fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

/// Запуск приложения.
pub fn run() {
    let shell = Shell::prepare().expect("Rhizomind: не удалось подготовить временную папку движка");
    tauri::Builder::default()
        .manage(shell)
        // Диалоги ядра — только из Rust: прав dialog:* у окна нет (ADR 0009).
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::Core::new(registry::Registry::from_profile()))
        .invoke_handler(commands::handler())
        .setup(|app| {
            create_main_window(app.handle())?;
            Ok(())
        })
        .build(context())
        .expect("не удалось запустить Rhizomind")
        .run(|app, event| handle_run_event(app, &event));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        let temp = std::env::temp_dir().join(format!("rhz-lib-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp).expect("временная папка теста");
        let data_dir = WebviewDataDir::prepare_in(&temp, std::process::id()).expect("папка данных");
        tauri::test::mock_builder()
            .manage(Shell::new(data_dir, security::browser_args()))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("тестовое приложение")
    }

    #[test]
    fn main_window_is_created_in_rust_with_label_from_capabilities() {
        let app = mock_app();
        let window = create_main_window(app.handle()).expect("главное окно");

        // Подменный runtime Tauri не хранит заголовок и настройки движка: их
        // проверяют сценарные тесты и тест следов (tests/e2e/).
        assert_eq!(window.label(), "main");
        assert!(app.get_webview_window(MAIN_WINDOW_LABEL).is_some());
    }

    #[test]
    fn exit_removes_webview_data_dir() {
        let app = mock_app();
        let dir = app.state::<Shell>().data_dir().to_path_buf();
        std::fs::write(dir.join("Local State"), "{}").unwrap();
        assert!(dir.is_dir());

        handle_run_event(app.handle(), &RunEvent::Exit);

        assert!(!dir.exists(), "папка данных осталась: {}", dir.display());
    }

    #[test]
    fn title_marks_webdriver_build_only() {
        if cfg!(feature = "webdriver") {
            assert_eq!(
                main_window_title(),
                "Rhizomind — ТЕСТОВАЯ СБОРКА (webdriver)"
            );
        } else {
            assert_eq!(main_window_title(), "Rhizomind");
        }
    }
}
