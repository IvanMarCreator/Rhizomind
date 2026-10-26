//! Ядро Rhizomind.
//!
//! Окна создаются здесь, в Rust, а не в `tauri.conf.json` (там окон
//! нет): только так движку до создания окна задаются временная папка данных,
//! режим InPrivate и флаги (ADR 0007). Меры защищённой оболочки — в
//! `security` и `webview_traces`, окна хранилищ — в `windows` (задача 1.3).

// Сборка «разработки» (`cfg(dev)`) грузит интерфейс с сервера разработки
// `http://localhost:5173`. Она бывает только отладочной (`tauri dev`). Голый
// `cargo build --release` без tauri CLI тоже дал бы `cfg(dev)` — такую сборку
// запрещаем: выпуск собирается только через `npm run build` / `package:*`.
// Модульные тесты в выпускном профиле (`cargo test --release --lib`, замеры
// скорости) — не программа: окна не открывают, поэтому `cfg(test)` пропускается.
#[cfg(all(dev, not(debug_assertions), not(test)))]
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
pub mod dialogs;
pub mod events;
pub mod external_links;
pub mod links;
pub mod names;
pub mod own_writes;
pub mod registry;
pub mod single_instance;
pub mod switcher;
pub mod vault;
pub mod watcher;
pub mod windows;

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
        let (browser_args, driver_data_dir) = webdriver_overrides_from_env();
        security::remove_webview_overrides(cfg!(feature = "webdriver"));
        #[cfg(windows)]
        security::windows::limit_error_reporting();
        let mut shell = Self::new(WebviewDataDir::prepare()?, browser_args);
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

    /// Завершить процесс браузера WebView2 (решение владельца 64): перед
    /// закрытием последнего окна, после записи ввода.
    pub fn terminate_browser(&self) -> bool {
        self.data_dir.terminate_browser()
    }

    /// После выхода: дождаться процесса браузера и удалить папку данных.
    pub fn clean_up(&self) -> std::io::Result<()> {
        self.data_dir
            .remove_after_exit(webview_traces::EXIT_TIMEOUT)
    }
}

/// Тестовая сборка (функция `webdriver`): флаги движка с портом отладки и
/// папка данных msedgedriver — читаются до удаления `WEBVIEW2_*`.
#[cfg(feature = "webdriver")]
fn webdriver_overrides_from_env() -> (String, Option<std::path::PathBuf>) {
    let args = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").ok();
    let port = security::webdriver_debug_port(args.as_deref());
    let folder = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
    (
        security::browser_args_with_debug_port(port),
        security::webdriver_user_data_folder(folder.as_deref()),
    )
}

/// Выпуск: защищённый набор флагов, переменные окружения не читаются.
#[cfg(not(feature = "webdriver"))]
fn webdriver_overrides_from_env() -> (String, Option<std::path::PathBuf>) {
    (security::browser_args(), None)
}

/// Создаёт окно хранилища `label` со страницей интерфейса из сборки
/// (`dist/index.html`) и защищёнными настройками движка — так создаётся
/// **каждое** окно приложения (задача 1.3: окна хранилищ — `crate::windows`;
/// задача 2.8а: окно управления хранилищами — [`create_manager_window`]).
/// `Shell` должен быть в состоянии приложения: у всех окон одного запуска одна
/// временная папка данных и одни флаги движка, поэтому они работают в одном
/// процессе браузера WebView2.
///
/// `saved` — размер и положение окна хранилища из списка хранилищ:
/// положение задаётся, только если заголовок окна виден на одном из
/// экранов (`windows::placement`); экраны известны только у созданного
/// окна, поэтому положение — сразу после создания, до показа окна.
pub fn create_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    saved: Option<registry::WindowState>,
) -> tauri::Result<WebviewWindow<R>> {
    let size = windows::placement(saved, &[]);
    build_window(app, label, &main_window_title(), size, saved)
}

/// Создаёт окно «Управление хранилищами» (920 × 600, без хранилища) той же
/// защищённой оболочкой, что окна хранилищ: те же настройки движка, метка
/// `vault-*` и права не шире ([`create_window`]).
pub fn create_manager_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> tauri::Result<WebviewWindow<R>> {
    build_window(
        app,
        label,
        &windows::manager_title(),
        windows::manager_placement(),
        None,
    )
}

fn build_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    title: &str,
    size: windows::Placement,
    saved: Option<registry::WindowState>,
) -> tauri::Result<WebviewWindow<R>> {
    let shell = app.state::<Shell>();
    let dev_url = if cfg!(dev) {
        app.config().build.dev_url.clone()
    } else {
        None
    };

    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(size.width, size.height)
        .min_inner_size(
            f64::from(windows::MIN_SIZE.0),
            f64::from(windows::MIN_SIZE.1),
        );
    let window = builder
        // Кэш, cookies и хранилища страницы — только в памяти (InPrivate);
        // исключение — тестовая сборка Linux (security::incognito).
        .incognito(security::incognito())
        // Служебная папка движка — во временной папке ОС, не в %LOCALAPPDATA%.
        .data_directory(shell.engine_data_dir().to_path_buf())
        .additional_browser_args(shell.browser_args())
        // Автозаполнение форм (в Tauri по умолчанию включено).
        .general_autofill_enabled(false)
        // Windows: окно показывается только после проверки InPrivate у
        // живого движка (harden_platform_webview).
        .visible(!cfg!(windows))
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

    let screens = windows::screens(&window.available_monitors().unwrap_or_default());
    if let Some((x, y)) = windows::placement(saved, &screens).position {
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }
    harden_platform_webview(&window, size.maximized);
    Ok(window)
}

/// Настройки WebView2, которых нет в Tauri: SmartScreen выключается, номер
/// процесса браузера запоминается для удаления папки данных после выхода;
/// окно показывается, только если движок подтвердил InPrivate; развёрнутое
/// окно разворачивается после показа (скрытое окно Windows при развороте
/// показалось бы до проверки).
fn harden_platform_webview<R: Runtime>(window: &WebviewWindow<R>, maximize: bool) {
    #[cfg(windows)]
    {
        let app = window.app_handle().clone();
        let window_to_show = window.clone();
        let _ = window.with_webview(move |webview| {
            let controller = webview.controller();
            if let Err(error) = security::windows::disable_smartscreen(&controller) {
                eprintln!("Rhizomind: не удалось выключить SmartScreen WebView2: {error}");
            }
            if let Ok(pid) = security::windows::browser_process_id(&controller) {
                app.state::<Shell>().data_dir.set_browser_pid(pid);
            }
            let checked = security::windows::is_in_private_mode_enabled(&controller).ok();
            match security::in_private_check(checked) {
                Ok(()) => {
                    let _ = window_to_show.show();
                    if maximize {
                        let _ = window_to_show.maximize();
                    }
                }
                Err(message) => {
                    // Журнала ошибок ещё нет (выпуск 0.5): сообщение — в stderr и окном.
                    eprintln!("Rhizomind: {message}");
                    security::windows::show_error(&message);
                    // Закрытие окна (а не exit): WebView уничтожается, папка
                    // данных удаляется при выходе (handle_run_event).
                    let _ = window_to_show.destroy();
                }
            }
        });
    }
    #[cfg(not(windows))]
    if maximize {
        let _ = window.maximize();
    }
}

/// Обработка событий приложения: выход откладывается, пока окна записывают
/// ввод (задача 0.7); после выхода удаляется папка данных движка.
///
/// Приложение завершается закрытием окон, а не `AppHandle::exit`: при `exit`
/// событие `Exit` приходит, пока WebView и процесс браузера ещё живы, — папка
/// занята и останется до уборки остатков при следующем запуске. Будущая команда
/// «Выход» должна закрывать окна (проверка — `tests/webview2_runtime.rs`).
pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if let RunEvent::ExitRequested { api, code, .. } = event {
        // Обычно сюда не попасть, пока окно ждёт записи: оно ещё существует
        // (скрыто). Но выход могли попросить и явно (`AppHandle::exit`).
        if windows::closing::exit_must_wait(app) {
            api.prevent_exit();
            let app = app.clone();
            let code = *code;
            // Поток не создать — выход остаётся отложенным: окна сами
            // закроются по пределу ожидания, и выход наступит обычным путём.
            let _ = std::thread::Builder::new()
                .name("rhizomind-exit".into())
                .spawn(move || windows::closing::exit_after_closing(&app, code));
        }
        return;
    }
    if !matches!(event, RunEvent::Exit) {
        return;
    }
    // Имя экземпляра — первым, до ожидания процесса браузера: новый запуск
    // сразу после закрытия окна не попадает на завершающийся экземпляр.
    if let Some(instance) = app.try_state::<single_instance::Instance>() {
        instance.release();
    }
    if let Some(shell) = app.try_state::<Shell>() {
        // Не удалось — папку уберёт следующий запуск (уборка остатков).
        let _ = shell.clean_up();
    }
}

/// События окон: учёт активного окна (один экземпляр), запись ввода перед
/// закрытием окна и привязка окна к хранилищу.
fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    single_instance::on_window_event(window, event);
    commands::on_window_event(window, event);
}

/// Конфигурация и встроенный интерфейс приложения.
pub fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

/// Запуск приложения.
pub fn run() {
    // Один экземпляр (задача 0.5) — до всего остального: второй процесс
    // будит первый и выходит, не создав временной папки движка, не прочитав
    // список хранилищ и не запустив WebView2.
    let Some(instance) = single_instance::claim() else {
        return;
    };
    let shell = Shell::prepare().expect("Rhizomind: не удалось подготовить временную папку движка");
    tauri::Builder::default()
        .manage(shell)
        .manage(instance)
        .manage(single_instance::Recent::default())
        // Окна, которые записывают ввод перед закрытием (задача 0.7).
        .manage(windows::closing::Closing::default())
        // Диалоги ядра — только из Rust: прав dialog:* у окна нет (ADR 0009).
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::Core::new(windows::startup_registry()))
        .invoke_handler(commands::handler())
        // Учёт активного окна (один экземпляр); закрытое окно отвязывается от
        // хранилища: наблюдатель останавливается.
        .on_window_event(on_window_event)
        .setup(|app| {
            // Сигнал второго экземпляра → точка «повторный запуск» (решение 58):
            // окно «Управление хранилищами» (задача 2.8б). Сигнал приходит
            // в `window_proc` в главном потоке, а `run_on_main_thread` из
            // главного потока выполнил бы замыкание сразу, внутри обработки
            // `WM_COPYDATA` (второй процесс ждал бы ответа). Поэтому вызов
            // ставится в очередь цикла событий из другого потока: `window_proc`
            // отвечает сразу, `on_second_instance` идёт потом в главном потоке.
            let handle = app.handle().clone();
            app.state::<single_instance::Instance>()
                .listen(Box::new(move || {
                    let handle = handle.clone();
                    // Builder, а не thread::spawn: нехватка ресурсов ОС не
                    // должна паниковать в `window_proc` (обзор 0.5); не
                    // удалось — сигнал теряется, первый экземпляр живёт.
                    let _ = std::thread::Builder::new().spawn(move || {
                        let target = handle.clone();
                        let _ = handle.run_on_main_thread(move || {
                            single_instance::on_second_instance(&target)
                        });
                    });
                }));
            // Последнее хранилище — в окне с его размером и положением (С3.1).
            windows::open_at_startup(app.handle());
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
    fn window_is_created_in_rust_with_label_from_capabilities() {
        let app = mock_app();
        let label = windows::new_label();
        let window = create_window(app.handle(), &label, None).expect("окно");

        // Подменный runtime Tauri не хранит заголовок и настройки движка: их
        // проверяют сценарные тесты и тест следов (tests/e2e/).
        assert_eq!(window.label(), label);
        assert!(label.starts_with(windows::LABEL_PREFIX));
        assert!(app.get_webview_window(&label).is_some());
    }

    /// Задача 2.8б: метка окна управления не попадает под шаблон прав окон
    /// хранилищ (`vault-*`), а метка окна хранилища — под шаблон окна
    /// управления (`manager-*`).
    #[test]
    fn manager_label_prefix_is_separate_from_vault_windows() {
        let manager = windows::new_manager_label();
        let vault = windows::new_label();
        assert!(manager.starts_with(windows::MANAGER_LABEL_PREFIX));
        assert!(!manager.starts_with(windows::LABEL_PREFIX));
        assert!(!vault.starts_with(windows::MANAGER_LABEL_PREFIX));
        assert!(!windows::MANAGER_LABEL_PREFIX.starts_with(windows::LABEL_PREFIX));
        assert!(!windows::LABEL_PREFIX.starts_with(windows::MANAGER_LABEL_PREFIX));
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
