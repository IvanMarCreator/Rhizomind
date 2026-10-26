//! Окна хранилищ (задача 1.3, С3.1).
//!
//! - Каждое окно приложения — окно хранилища с меткой `vault-<16
//!   шестнадцатеричных цифр>` ([`new_label`]). Права окна выдаются по
//!   шаблону `vault-*` (`capabilities/main.json`), одни и те же у каждого
//!   окна. Случайная часть метки нужна для изоляции событий: интерфейс одного
//!   окна не знает метку другого и не может подписаться на его события
//!   (подписка без цели — `Any` — запрещена фильтром изолированного iframe,
//!   `src-isolation/index.js`).
//! - Другое хранилище открывается в новом окне; уже открытое хранилище
//!   получает фокус, второе окно не создаётся ([`open_vault`]). Окно без
//!   хранилища (первый запуск) открывает хранилище в себе.
//! - Каждое окно создаётся защищённой оболочкой 0.3
//!   ([`crate::create_window`]): InPrivate, общая временная папка данных
//!   движка этого запуска, флаги, запрет переходов, новых окон, загрузок и
//!   разрешений. Все окна одного запуска работают в одном процессе браузера
//!   WebView2 (одна папка данных и одни флаги — иначе WebView2 откажет).
//! - Размер и положение окна запоминаются в списке хранилищ при закрытии
//!   окна ([`remember_placement`]); при запуске открывается последнее
//!   хранилище ([`open_at_startup`]).

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::commands::{Core, CoreError, CoreResult, ErrorCode, VaultOpenOutcome, VaultSummary};
use crate::events::WindowEmitter;
use crate::registry::WindowState;

#[cfg(test)]
mod tests;

/// Начало метки каждого окна; на шаблон `vault-*` выданы права окна.
pub const LABEL_PREFIX: &str = "vault-";

/// Размер нового окна (логические пиксели).
pub const DEFAULT_SIZE: (u32, u32) = (1200, 800);

/// Наименьший размер окна.
pub const MIN_SIZE: (u32, u32) = (640, 480);

/// Новая уникальная метка окна: `vault-<16 шестнадцатеричных цифр>`.
pub fn new_label() -> String {
    format!("{LABEL_PREFIX}{:016x}", crate::webview_traces::random_u64())
}

/// Прямоугольник рабочей области экрана в логических пикселях.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Как поставить новое окно.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub width: f64,
    pub height: f64,
    /// Левый верхний угол; `None` — решает ОС.
    pub position: Option<(f64, f64)>,
    pub maximized: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            width: f64::from(DEFAULT_SIZE.0),
            height: f64::from(DEFAULT_SIZE.1),
            position: None,
            maximized: false,
        }
    }
}

/// Сколько от верхнего левого угла окна должно быть видно на экране, чтобы
/// окно можно было взять за заголовок (логические пиксели).
const GRAB_AREA: (f64, f64) = (120.0, 32.0);

/// Положение окна из списка хранилищ: размер — не меньше [`MIN_SIZE`];
/// положение — только если заголовок окна виден на одном из экранов
/// (экран отключили — окно ставит ОС).
pub fn placement(saved: Option<WindowState>, screens: &[Screen]) -> Placement {
    let Some(saved) = saved else {
        return Placement::default();
    };
    let width = f64::from(saved.width.max(MIN_SIZE.0));
    let height = f64::from(saved.height.max(MIN_SIZE.1));
    let (x, y) = (f64::from(saved.x), f64::from(saved.y));
    let visible = screens.iter().any(|screen| {
        x >= screen.x
            && y >= screen.y
            && x + GRAB_AREA.0 <= screen.x + screen.width
            && y + GRAB_AREA.1 <= screen.y + screen.height
    });
    Placement {
        width,
        height,
        position: visible.then_some((x, y)),
        maximized: saved.maximized,
    }
}

/// Рабочие области экранов в логических пикселях.
pub fn screens(monitors: &[tauri::Monitor]) -> Vec<Screen> {
    monitors
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor().max(0.1);
            let area = monitor.work_area();
            Screen {
                x: f64::from(area.position.x) / scale,
                y: f64::from(area.position.y) / scale,
                width: f64::from(area.size.width) / scale,
                height: f64::from(area.size.height) / scale,
            }
        })
        .collect()
}

/// Переводит фокус в окно: разворачивает свёрнутое и показывает его.
pub fn focus<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Открывает хранилище — одна функция для `vault_create`,
/// `vault_open_folder` и `vault_open`:
/// - хранилище уже открыто (или открывается) в каком-то окне — фокус на
///   него (`focused`), второе окно не создаётся;
/// - окно вызова без хранилища — хранилище открывается в нём;
/// - иначе — новое окно защищённой оболочки.
///
/// Хранилище привязывается к метке окна **до** создания нового окна:
/// интерфейс нового окна первой командой (`vault_current`) уже видит своё
/// хранилище. Не удалось создать окно — хранилище отвязывается.
pub async fn open_vault<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    vault: VaultSummary,
) -> CoreResult<VaultOpenOutcome> {
    let core = app.state::<Core>();
    let caller_is_empty = core.window_vault(caller).is_none();
    let label = if caller_is_empty {
        caller.to_owned()
    } else {
        new_label()
    };
    let claim = match core.claim_vault(&vault.id, &label) {
        Ok(claim) => claim,
        Err(open_in) => {
            if let Some(window) = app.get_webview_window(&open_in) {
                focus(&window);
            }
            return Ok(VaultOpenOutcome::Focused);
        }
    };
    let opening_app = app.clone();
    let opening_label = label.clone();
    let opening_vault = vault.clone();
    crate::commands::blocking(move || {
        bind_vault(&opening_app, &opening_label, opening_vault)?;
        drop(claim);
        Ok(())
    })
    .await?;
    if caller_is_empty {
        return Ok(VaultOpenOutcome::Opened);
    }
    let saved = core.registry().window(&vault.id);
    if let Err(error) = crate::create_window(app, &label, saved) {
        eprintln!("Rhizomind: не удалось создать окно: {error}");
        core.unbind_window(&label);
        return Err(CoreError::new(ErrorCode::IoFailed));
    }
    Ok(VaultOpenOutcome::Opened)
}

/// Открывает хранилище и привязывает его к окну `label` (обход папки,
/// наблюдатель, события только этому окну); запоминает его последним
/// открытым. Блокирует поток.
fn bind_vault<R: Runtime>(app: &AppHandle<R>, label: &str, vault: VaultSummary) -> CoreResult<()> {
    let core = app.state::<Core>();
    let emit = Arc::new(WindowEmitter::new(app.clone(), label.to_owned()));
    let root = PathBuf::from(&vault.path);
    core.open_vault(label, vault.clone(), &root, emit)?;
    // «Последнее открытое» не важно для открытия: ошибка списка хранилищ не
    // отменяет уже открытое хранилище.
    let _ = core.registry().mark_opened(&vault.id);
    Ok(())
}

/// Запуск приложения: последнее открытое хранилище — в новом окне с его
/// размером и положением; нет его или папка недоступна — окно без
/// хранилища. Обход папки идёт вне главного потока; окно создаётся после
/// привязки хранилища.
pub fn open_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let label = new_label();
        let core = app.state::<Core>();
        let registry = core.shared_registry();
        let last = crate::commands::blocking(move || Ok(registry.last_opened()))
            .await
            .ok()
            .flatten();
        let mut saved = None;
        if let Some(vault) = last {
            let id = vault.id.clone();
            let binding_app = app.clone();
            let binding_label = label.clone();
            let bound =
                crate::commands::blocking(move || bind_vault(&binding_app, &binding_label, vault))
                    .await;
            if bound.is_ok() {
                saved = core.registry().window(&id);
            }
        }
        if let Err(error) = crate::create_window(&app, &label, saved) {
            eprintln!("Rhizomind: не удалось создать окно: {error}");
            core.unbind_window(&label);
            // Окон нет — приложению нечего показывать.
            app.exit(1);
        }
    });
}

/// Запоминает размер и положение окна его хранилища (закрытие окна).
/// Развёрнутое окно сохраняет только признак разворота: размер до
/// разворота ОС не сообщает.
pub fn remember_placement<R: Runtime>(window: &tauri::Window<R>) {
    let Some(core) = window.try_state::<Core>() else {
        return;
    };
    let Some(open) = core.window_vault(window.label()) else {
        return;
    };
    let id = open.summary.id.clone();
    if window.is_minimized().unwrap_or(false) {
        // Свёрнутое окно Windows сообщает положение (-32000, -32000).
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let previous = core.registry().window(&id);
    let state = if maximized {
        let base = previous.unwrap_or(WindowState {
            width: DEFAULT_SIZE.0,
            height: DEFAULT_SIZE.1,
            x: 0,
            y: 0,
            maximized: true,
        });
        WindowState {
            maximized: true,
            ..base
        }
    } else {
        let (Ok(scale), Ok(position), Ok(size)) = (
            window.scale_factor(),
            window.outer_position(),
            window.inner_size(),
        ) else {
            return;
        };
        let logical_position = position.to_logical::<f64>(scale);
        let logical_size = size.to_logical::<f64>(scale);
        if logical_size.width < 1.0 || logical_size.height < 1.0 {
            return;
        }
        WindowState {
            width: logical_size.width.round() as u32,
            height: logical_size.height.round() as u32,
            x: logical_position.x.round() as i32,
            y: logical_position.y.round() as i32,
            maximized: false,
        }
    };
    let _ = core.registry().save_window(&id, state);
}

/// Тестовая сборка (функция `webdriver`): папка профиля и хранилище для
/// сценарных тестов из переменных окружения — чтобы открыть тестовое
/// хранилище без системного окна выбора папки и не трогать список хранилищ
/// пользователя. В выпуске функции нет вовсе (строк переменных в
/// исполняемом файле нет — `tests/e2e/release-binary.test.ts`).
#[cfg(feature = "webdriver")]
pub mod e2e {
    /// Папка, в которой тестовая сборка держит `vaults.json`.
    pub const PROFILE_ENV: &str = "RHIZOMIND_E2E_PROFILE_DIR";
    /// Папка хранилища, которое тестовая сборка открывает при запуске.
    pub const VAULT_ENV: &str = "RHIZOMIND_E2E_VAULT";

    /// Список хранилищ тестовой сборки: из [`PROFILE_ENV`], иначе — профиль ОС.
    pub fn registry() -> crate::registry::Registry {
        match std::env::var_os(PROFILE_ENV) {
            Some(dir) => crate::registry::Registry::at(
                std::path::PathBuf::from(dir).join(crate::registry::FILE_NAME),
            ),
            None => crate::registry::Registry::from_profile(),
        }
    }

    /// Хранилище из [`VAULT_ENV`] добавляется в список и становится последним
    /// открытым: при запуске оно открывается обычным путём.
    pub fn prepare(registry: &crate::registry::Registry) {
        if let Some(root) = std::env::var_os(VAULT_ENV) {
            match registry.add(std::path::Path::new(&root)) {
                Ok(vault) => {
                    let _ = registry.mark_opened(&vault.id);
                }
                Err(error) => eprintln!("Rhizomind (тестовая сборка): {VAULT_ENV}: {error}"),
            }
        }
    }
}

/// Список хранилищ этого запуска: профиль ОС; в тестовой сборке — папка и
/// хранилище из переменных окружения ([`e2e`]).
pub fn startup_registry() -> crate::registry::Registry {
    #[cfg(feature = "webdriver")]
    {
        let registry = e2e::registry();
        e2e::prepare(&registry);
        registry
    }
    #[cfg(not(feature = "webdriver"))]
    crate::registry::Registry::from_profile()
}
