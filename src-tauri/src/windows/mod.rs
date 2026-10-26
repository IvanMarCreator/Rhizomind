//! Окна хранилищ (задача 1.3, С3.1).
//!
//! - Каждое окно приложения — окно хранилища с меткой `vault-<16
//!   шестнадцатеричных цифр>` ([`new_label`]). Права окна выдаются по
//!   шаблону `vault-*` (`capabilities/main.json`), одни и те же у каждого
//!   окна. Изоляция событий между окнами держится на фильтре изолированного
//!   iframe (`src-isolation/index.js`): подписка — только на 7 событий ядра
//!   (`commands::EVENT_NAMES`) и только с целью «окно с меткой». Встроенные
//!   события Tauri (`tauri://window-created`, `tauri://webview-created`)
//!   рассылаются всем окнам и несут метку нового окна — подписка на них
//!   запрещена (ревью безопасности 1.3). Других путей узнать чужую метку у
//!   интерфейса нет: в ответах команд и событиях ядра меток нет, команды
//!   окон и WebView (`plugin:window|*`, `plugin:webview|*`) не выданы правами
//!   и не проходят фильтр, `__TAURI_INTERNALS__.metadata` содержит только
//!   свою метку. Случайная часть метки — второй рубеж: угадать 64 бита нельзя.
//! - Другое хранилище открывается в новом окне; уже открытое хранилище
//!   получает фокус, второе окно не создаётся ([`open_vault`]).
//! - Каждое окно создаётся защищённой оболочкой 0.3
//!   ([`crate::create_window`]): InPrivate, общая временная папка данных
//!   движка этого запуска, флаги, запрет переходов, новых окон, загрузок и
//!   разрешений. Все окна одного запуска работают в одном процессе браузера
//!   WebView2 (одна папка данных и одни флаги — иначе WebView2 откажет).
//! - Окно «Управление хранилищами» (задача 2.8а, решение владельца 58) —
//!   отдельное окно приложения 920 × 600 без хранилища, одно на программу
//!   ([`open_manager`]). Оно создаётся той же защищённой оболочкой, с меткой
//!   `manager-<16 шестнадцатеричных цифр>` и своими правами
//!   (`capabilities/manager.json`, задача 2.8б): создание, открытие папки,
//!   выбор папки и «Убрать из списка» есть только у него. Хранилище к нему не
//!   привязывается ([`Core::claim_vault`]): выбранное хранилище открывается в
//!   своём окне, а окно управления закрывается. Интерфейс узнаёт окно
//!   управления по ответу `vault_current` без хранилища.
//! - При запуске открывается последнее хранилище; нет его или папка
//!   недоступна — окно управления (Э11б при пустом списке).
//! - Размер и положение окна запоминаются в списке хранилищ при закрытии
//!   окна ([`remember_placement`]); при запуске открывается последнее
//!   хранилище ([`open_at_startup`]).
//! - Закрытие окна хранилища (задача 0.7, решение владельца 53): окно сразу
//!   скрывается, а уничтожается после того, как интерфейс записал ввод (или
//!   через предел ожидания) — [`closing`].

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::commands::{
    ClaimRefused, Core, CoreError, CoreResult, ErrorCode, ManagerSlot, VaultOpenOutcome,
    VaultSummary,
};
use crate::events::WindowEmitter;
use crate::registry::WindowState;

pub mod closing;
#[cfg(test)]
mod tests;

/// Начало метки окна хранилища; на шаблон `vault-*` выданы права окон
/// хранилищ (`capabilities/main.json`).
pub const LABEL_PREFIX: &str = "vault-";

/// Начало метки окна «Управление хранилищами»; на шаблон `manager-*` выданы
/// его права (`capabilities/manager.json`, задача 2.8б). Не начинается с
/// `vault-`: права окон хранилищ ему не достаются.
pub const MANAGER_LABEL_PREFIX: &str = "manager-";

/// Размер нового окна (логические пиксели).
pub const DEFAULT_SIZE: (u32, u32) = (1200, 800);

/// Наименьший размер окна.
pub const MIN_SIZE: (u32, u32) = (640, 480);

/// Размер окна «Управление хранилищами» (docs/design/tokens.md).
pub const MANAGER_SIZE: (u32, u32) = (920, 600);

/// Заголовок окна «Управление хранилищами» (макет Э11).
pub fn manager_title() -> String {
    format!("Управление хранилищами — {}", crate::main_window_title())
}

/// Наибольшая длина имени хранилища в заголовке окна (символов, вместе с
/// многоточием).
pub const VAULT_TITLE_MAX_CHARS: usize = 120;

/// Заголовок окна хранилища: «<имя хранилища> — Rhizomind» (задача 2.11,
/// находка QA Н2, вопрос 82 вариант А). Имя открытой заметки в заголовок
/// **не** попадает: заголовок видит Windows (панель задач, Alt+Tab,
/// журнал действий, специальные возможности), а имена заметок вне хранилища
/// не уходят (инвариант 2, docs/SECURITY.md). Имя хранилища — то, что
/// показывает интерфейс (`VaultSummary::name`, имя папки); из него убраны
/// управляющие символы, переводы строк и знаки направления текста (подмена
/// вида заголовка), длина ограничена [`VAULT_TITLE_MAX_CHARS`]. Пустое
/// имя — просто «Rhizomind».
pub fn vault_title(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control() && !is_direction_mark(*c))
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return crate::main_window_title();
    }
    let shown = if clean.chars().count() > VAULT_TITLE_MAX_CHARS {
        let mut cut: String = clean.chars().take(VAULT_TITLE_MAX_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        clean.to_owned()
    };
    format!("{shown} — {}", crate::main_window_title())
}

/// Невидимые знаки направления текста (LRM/RLM, встраивания, изоляции):
/// переставляют части заголовка при показе.
fn is_direction_mark(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Как поставить окно управления: 920 × 600, положение выбирает ОС.
pub fn manager_placement() -> Placement {
    Placement {
        width: f64::from(MANAGER_SIZE.0),
        height: f64::from(MANAGER_SIZE.1),
        position: None,
        maximized: false,
    }
}

/// Новая уникальная метка окна хранилища: `vault-<16 шестнадцатеричных цифр>`.
pub fn new_label() -> String {
    format!("{LABEL_PREFIX}{:016x}", crate::webview_traces::random_u64())
}

/// Новая уникальная метка окна управления: `manager-<16 шестнадцатеричных цифр>`.
pub fn new_manager_label() -> String {
    format!(
        "{MANAGER_LABEL_PREFIX}{:016x}",
        crate::webview_traces::random_u64()
    )
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
/// - окно вызова без хранилища (не окно управления) — хранилище
///   открывается в нём;
/// - иначе — новое окно защищённой оболочки.
///
/// Вызов из окна «Управление хранилищами» после открытия или фокуса
/// закрывает окно управления (как в Obsidian).
///
/// Хранилище привязывается к метке окна **до** создания нового окна:
/// интерфейс нового окна первой командой (`vault_current`) уже видит своё
/// хранилище. Не удалось создать окно — хранилище отвязывается.
pub async fn open_vault<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    vault: VaultSummary,
) -> CoreResult<VaultOpenOutcome> {
    let outcome = open_vault_in_window(app, caller, vault).await?;
    // Окно управления сделало своё дело: хранилище открыто или в фокусе.
    if app.state::<Core>().is_manager(caller) {
        close_manager(app, caller);
    }
    Ok(outcome)
}

async fn open_vault_in_window<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    vault: VaultSummary,
) -> CoreResult<VaultOpenOutcome> {
    let core = app.state::<Core>();
    // Окно этого хранилища закрывается (скрыто, пишет ввод): дождаться конца —
    // окно вернётся (фокус) или хранилище освободится (новое окно).
    if let Some(label) = core.window_of_vault(&vault.id)
        && closing::is_closing(app, &label)
    {
        let waiting_app = app.clone();
        let id = vault.id.clone();
        crate::commands::blocking(move || {
            closing::wait_closed(&waiting_app, &label, &id);
            Ok(())
        })
        .await?;
    }
    let focus_on = |label: &str| {
        if let Some(window) = app.get_webview_window(label) {
            focus(&window);
        }
        Ok(VaultOpenOutcome::Focused)
    };
    // Сначала — окно вызова; занято (своё хранилище или идёт открытие) —
    // новое окно.
    let (label, claim) = match core.claim_vault(&vault.id, caller) {
        Ok(claim) => (caller.to_owned(), claim),
        Err(ClaimRefused::OpenIn(open_in)) => return focus_on(&open_in),
        Err(ClaimRefused::WindowBusy) => {
            let label = new_label();
            match core.claim_vault(&vault.id, &label) {
                Ok(claim) => (label, claim),
                Err(ClaimRefused::OpenIn(open_in)) => return focus_on(&open_in),
                Err(ClaimRefused::WindowBusy) => return Err(CoreError::new(ErrorCode::IoFailed)),
            }
        }
    };
    let caller_is_empty = label == caller;
    let opening_app = app.clone();
    let opening_label = label.clone();
    let opening_vault = vault.clone();
    let title = vault_title(&vault.name);
    crate::commands::blocking(move || {
        bind_vault(&opening_app, &opening_label, opening_vault)?;
        drop(claim);
        Ok(())
    })
    .await?;
    if caller_is_empty {
        // Окно уже есть: заголовок меняет ядро (у интерфейса права на
        // заголовок нет).
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.set_title(&title);
        }
        return Ok(VaultOpenOutcome::Opened);
    }
    let saved = core.registry().window(&vault.id);
    if let Err(error) = crate::create_window(app, &label, &title, saved) {
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
/// размером и положением; нет его или папка недоступна — окно «Управление
/// хранилищами» (решение владельца 58). Обход папки идёт вне главного
/// потока; окно создаётся после привязки хранилища.
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
        let mut title = crate::main_window_title();
        if let Some(vault) = last {
            let id = vault.id.clone();
            let name = vault.name.clone();
            let binding_app = app.clone();
            let binding_label = label.clone();
            let bound =
                crate::commands::blocking(move || bind_vault(&binding_app, &binding_label, vault))
                    .await;
            if bound.is_ok() {
                title = vault_title(&name);
                saved = Some(core.registry().window(&id));
            }
        }
        let opened = match saved {
            Some(saved) => crate::create_window(&app, &label, &title, saved)
                .map(|_| ())
                .map_err(|error| {
                    eprintln!("Rhizomind: не удалось создать окно: {error}");
                    core.unbind_window(&label);
                }),
            None => open_manager(&app).map_err(|_| ()),
        };
        if opened.is_err() {
            // Окон нет — приложению нечего показывать.
            app.exit(1);
        }
    });
}

/// Открывает окно «Управление хранилищами»; уже открытое — получает фокус.
/// Одна функция для команды `vault_manager_open` (переключатель хранилищ),
/// запуска без хранилища ([`open_at_startup`]) и повторного запуска
/// программы (задача 0.5, решение владельца 58). Вызывать можно из любого
/// потока, в том числе из главного: замков на время создания окна не держит.
pub fn open_manager<R: Runtime>(app: &AppHandle<R>) -> CoreResult<()> {
    // Выход начат (последнее окно закрывается, браузер завершён): окно не
    // создаётся. Повторный запуск в эти доли секунды ничего не откроет —
    // программу запускают ещё раз после выхода.
    if closing::is_exiting(app) {
        return Ok(());
    }
    let core = app.state::<Core>();
    match core.reserve_manager(&new_manager_label()) {
        ManagerSlot::Existing(label) => {
            // Окно ещё создаётся или скрыто до проверки InPrivate (Windows) —
            // не трогаем: `unminimize` показал бы его раньше проверки;
            // фокус оно получит само при показе.
            if let Some(window) = app.get_webview_window(&label)
                && window.is_visible().unwrap_or(false)
            {
                focus(&window);
            }
            Ok(())
        }
        ManagerSlot::New(label) => match crate::create_manager_window(app, &label) {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("Rhizomind: не удалось создать окно управления хранилищами: {error}");
                core.unbind_window(&label);
                Err(CoreError::new(ErrorCode::IoFailed))
            }
        },
    }
}

/// Закрывает окно управления `label`: метка снимается сразу (следующий
/// вызов [`open_manager`] создаст новое окно), окно уничтожается.
fn close_manager<R: Runtime>(app: &AppHandle<R>, label: &str) {
    app.state::<Core>().unbind_window(label);
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
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
    /// Папка хранилища, которое тестовая сборка открывает при запуске. Можно
    /// несколько папок через разделитель путей ОС (`;` на Windows, `:` на
    /// Linux): все попадают в список хранилищ, последняя открывается.
    pub const VAULT_ENV: &str = "RHIZOMIND_E2E_VAULT";
    /// Ответ системного окна выбора папки (`vault_create`,
    /// `vault_open_folder`): папка, которую «выбрал» пользователь. Окно не
    /// показывается (задача 2.8а: сценарные тесты создания и открытия).
    pub const PICK_FOLDER_ENV: &str = "RHIZOMIND_E2E_PICK_FOLDER";
    /// Пауза перед каждой записью заметки (`note_write`), миллисекунды:
    /// «медленный диск» для сценарного теста предела ожидания при закрытии
    /// окна (задача 0.7).
    pub const SLOW_WRITE_ENV: &str = "RHIZOMIND_E2E_SLOW_WRITE_MS";

    /// Любая папка хранилища считается лежащей на внешнем носителе (флешка):
    /// сценарный тест решения владельца 66 (задача 0.7).
    pub const REMOVABLE_ENV: &str = "RHIZOMIND_E2E_REMOVABLE";

    /// Не завершать процесс браузера при закрытии последнего окна (решение
    /// 64): контрольный прогон сценарного теста — проверка, что тест видит
    /// загрузку OneAuth при штатном закрытии.
    pub const KEEP_BROWSER_ENV: &str = "RHIZOMIND_E2E_KEEP_BROWSER";

    /// Задана ли [`KEEP_BROWSER_ENV`].
    pub fn keep_browser() -> bool {
        std::env::var_os(KEEP_BROWSER_ENV).is_some()
    }

    /// Файл-признак «носитель вынут»: пока он есть, запись заметки
    /// отклоняется (`vault_unavailable`), а папка хранилища считается
    /// недоступной (решение 66). На Windows папку, за которой следит
    /// наблюдатель, не переименовать — поэтому подмена, а не переименование.
    pub const UNPLUG_FLAG_ENV: &str = "RHIZOMIND_E2E_UNPLUG_FLAG";

    /// Носитель «вынут»: файл из [`UNPLUG_FLAG_ENV`] существует.
    pub fn unplugged() -> bool {
        std::env::var_os(UNPLUG_FLAG_ENV).is_some_and(|flag| std::path::Path::new(&flag).exists())
    }

    /// Задана ли [`REMOVABLE_ENV`].
    pub fn removable_volume() -> bool {
        std::env::var_os(REMOVABLE_ENV).is_some()
    }

    /// Пауза из [`SLOW_WRITE_ENV`] перед записью заметки.
    pub fn slow_note_write() {
        let delay = std::env::var(SLOW_WRITE_ENV)
            .ok()
            .and_then(|ms| ms.parse::<u64>().ok());
        if let Some(ms) = delay {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    /// Папка из [`PICK_FOLDER_ENV`] вместо системного окна выбора папки.
    pub fn picked_folder() -> Option<std::path::PathBuf> {
        std::env::var_os(PICK_FOLDER_ENV).map(std::path::PathBuf::from)
    }

    /// Список хранилищ тестовой сборки: из [`PROFILE_ENV`], иначе — профиль ОС.
    pub fn registry() -> crate::registry::Registry {
        match std::env::var_os(PROFILE_ENV) {
            Some(dir) => crate::registry::Registry::at(
                std::path::PathBuf::from(dir).join(crate::registry::FILE_NAME),
            ),
            None => crate::registry::Registry::from_profile(),
        }
    }

    /// Хранилища из [`VAULT_ENV`] добавляются в список, последнее становится
    /// последним открытым: при запуске оно открывается обычным путём.
    pub fn prepare(registry: &crate::registry::Registry) {
        let Some(roots) = std::env::var_os(VAULT_ENV) else {
            return;
        };
        for root in std::env::split_paths(&roots) {
            match registry.add(&root) {
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
