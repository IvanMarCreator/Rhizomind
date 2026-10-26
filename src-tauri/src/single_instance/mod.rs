//! Один экземпляр программы (задача 0.5, решения владельца 57 и 58).
//!
//! Повторный запуск не открывает второй процесс: он будит уже работающий
//! экземпляр и выходит, ничего не записав. Проверка — первой строкой
//! [`crate::run`], до временной папки WebView2, до чтения `vaults.json` и до
//! создания движка.
//!
//! - **Windows** ([`windows`]): именованный мьютекс `Local\Rhizomind.SingleInstance`
//!   (объект ядра в пространстве имён сессии, не файл) и скрытое окно «только
//!   для сообщений» первого экземпляра; второй отдаёт первому право на
//!   передний план и шлёт ему `WM_COPYDATA` с меткой протокола.
//! - **Linux, macOS** ([`unix`]): файл-замок `instance.lock` рядом с
//!   `vaults.json` (`flock`); связи с первым нет — второй пишет «Rhizomind уже
//!   запущен» в stderr и выходит.
//!
//! Без сети: ни сокетов, ни D-Bus, ни новых крейтов (плагин
//! `tauri-plugin-single-instance` включает `tokio/net`, на Linux — `zbus` с
//! TCP-транспортом; `docs/research/2026-10-26-single-instance.md`).
//!
//! **Точка «повторный запуск»** — [`on_second_instance`]: первый экземпляр
//! вызывает её в главном потоке, когда его разбудил второй. Она открывает
//! окно «Управление хранилищами» (решение 58, задача 2.8б). Путь хранилища
//! из аргументов, когда он появится, придёт сюда же тем же сообщением.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};

#[cfg(test)]
mod tests;

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

/// Имя мьютекса (без `Local\`) и класса окна для сообщений. Постоянное: без
/// путей, имён хранилищ и пользователя. Тестовая сборка — своё имя, чтобы не
/// мешать выпускной на той же машине.
pub const NAME: &str = if cfg!(feature = "webdriver") {
    "Rhizomind.SingleInstance.webdriver"
} else {
    "Rhizomind.SingleInstance"
};

/// Имя файла-замка в папке профиля (Linux, macOS).
pub const LOCK_FILE: &str = if cfg!(feature = "webdriver") {
    "instance-webdriver.lock"
} else {
    "instance.lock"
};

/// Сколько второй экземпляр пытается достучаться до первого, который ещё
/// запускается или уже завершается (Windows).
pub const CLAIM_TIMEOUT: Duration = Duration::from_secs(5);

/// Linux, macOS: связи нет; ждём только освобождения замка завершающимся
/// экземпляром.
#[cfg(unix)]
const UNIX_CLAIM_TIMEOUT: Duration = Duration::from_secs(1);

/// Сообщение второму запуску, когда первый экземпляр не ответил (Windows —
/// системным окном) или когда связи с ним нет (Linux, macOS — в stderr).
pub const ALREADY_RUNNING: &str = "Rhizomind уже запущен";

/// Пауза между попытками.
const RETRY_STEP: Duration = Duration::from_millis(100);

/// Чем кончилась проверка экземпляра.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<G> {
    /// Этот процесс — первый. `None` — проверку провести не удалось
    /// (программа запускается без защиты от второго экземпляра).
    Primary(Option<G>),
    /// Работающий экземпляр разбужен; этот процесс выходит.
    Notified,
    /// Экземпляр есть, но не ответил за отведённое время; этот процесс выходит.
    GaveUp,
}

/// Логика проверки без ОС: занять имя; занято — разбудить владельца; не
/// ответил — повторять до `timeout` (владелец запускается или завершается).
pub fn claim_with<G>(
    mut try_acquire: impl FnMut() -> std::io::Result<Option<G>>,
    mut notify: impl FnMut() -> bool,
    timeout: Duration,
    elapsed: impl Fn() -> Duration,
    mut sleep: impl FnMut(Duration),
) -> Outcome<G> {
    loop {
        match try_acquire() {
            Ok(Some(guard)) => return Outcome::Primary(Some(guard)),
            Ok(None) => {}
            Err(error) => {
                eprintln!("Rhizomind: проверка второго экземпляра не удалась: {error}");
                return Outcome::Primary(None);
            }
        }
        if notify() {
            return Outcome::Notified;
        }
        if elapsed() >= timeout {
            return Outcome::GaveUp;
        }
        sleep(RETRY_STEP);
    }
}

#[cfg(windows)]
type Guard = windows::MutexGuard;
#[cfg(unix)]
type Guard = unix::LockGuard;

/// Первый экземпляр: держит мьютекс или замок до выхода (состояние
/// приложения).
#[derive(Default)]
pub struct Instance {
    guard: Mutex<Option<Guard>>,
    #[cfg(windows)]
    listener: Mutex<Option<windows::Listener>>,
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Instance")
    }
}

impl Instance {
    /// Начинает принимать сигналы второго экземпляра. Windows — создаёт окно
    /// для сообщений; вызывать в главном потоке (его очередь сообщений
    /// разбирает цикл событий Tauri). Linux, macOS — ничего.
    pub fn listen(&self, on_signal: Box<dyn Fn() + Send + Sync + 'static>) {
        #[cfg(windows)]
        match windows::Listener::start_named(NAME, on_signal) {
            Ok(listener) => {
                *self.listener.lock().unwrap_or_else(PoisonError::into_inner) = Some(listener);
            }
            Err(error) => eprintln!("Rhizomind: окно для сообщений не создано: {error}"),
        }
        #[cfg(not(windows))]
        drop(on_signal);
    }

    /// Освобождает имя: при выходе, до ожидания процесса браузера — новый
    /// запуск сразу после закрытия окна не попадает на завершающийся
    /// экземпляр. Windows: в главном потоке (окно уничтожает его владелец).
    pub fn release(&self) {
        #[cfg(windows)]
        drop(
            self.listener
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        drop(
            self.guard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
}

/// Проверка экземпляра при запуске. `None` — уже работает другой экземпляр:
/// он разбужен (Windows) или пользователь предупреждён; процессу надо выйти,
/// ничего не создавая.
pub fn claim() -> Option<Instance> {
    let start = Instant::now();
    #[cfg(windows)]
    let outcome = claim_with(
        || windows::try_acquire_named(NAME),
        || windows::notify_named(NAME),
        CLAIM_TIMEOUT,
        || start.elapsed(),
        std::thread::sleep,
    );
    #[cfg(unix)]
    let outcome = claim_with(
        || match unix::lock_dir() {
            Some(dir) => unix::try_acquire_at(&dir.join(LOCK_FILE)),
            None => Err(std::io::Error::other("нет папки профиля")),
        },
        || false,
        UNIX_CLAIM_TIMEOUT,
        || start.elapsed(),
        std::thread::sleep,
    );
    match outcome {
        Outcome::Primary(guard) => {
            let instance = Instance::default();
            *instance
                .guard
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = guard;
            Some(instance)
        }
        Outcome::Notified => None,
        Outcome::GaveUp => {
            eprintln!("{ALREADY_RUNNING}");
            // Окно не нашлось или не ответило: без сообщения пользователь не
            // понял бы, почему ничего не открылось. Системное окно ничего не пишет.
            #[cfg(windows)]
            crate::security::windows::show_error(ALREADY_RUNNING);
            None
        }
    }
}

/// Порядок окон по последней активности: нужен [`on_second_instance`].
#[derive(Debug, Default)]
pub struct RecentWindows {
    /// Метки окон, последнее активное — в конце.
    order: Vec<String>,
}

impl RecentWindows {
    /// Окно получило фокус.
    pub fn focused(&mut self, label: &str) {
        self.order.retain(|l| l != label);
        self.order.push(label.to_owned());
    }

    /// Окно закрыто.
    pub fn closed(&mut self, label: &str) {
        self.order.retain(|l| l != label);
    }

    /// Окно, которое выводить вперёд: последнее активное из показанных;
    /// истории нет — первое показанное. Скрытое окно (Windows: до проверки
    /// InPrivate) не выбирается никогда.
    pub fn target(&self, visible: &[String]) -> Option<String> {
        self.order
            .iter()
            .rev()
            .find(|label| visible.contains(label))
            .or_else(|| visible.first())
            .cloned()
    }
}

/// Состояние приложения: порядок окон по активности.
#[derive(Debug, Default)]
pub struct Recent(Mutex<RecentWindows>);

/// Учёт фокуса и закрытия окон (из `on_window_event`).
pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    let Some(recent) = window.try_state::<Recent>() else {
        return;
    };
    let mut recent = recent.0.lock().unwrap_or_else(PoisonError::into_inner);
    match event {
        tauri::WindowEvent::Focused(true) => recent.focused(window.label()),
        tauri::WindowEvent::Destroyed => recent.closed(window.label()),
        _ => {}
    }
}

/// **Точка «повторный запуск»** (решение 58): первый экземпляр получил
/// сигнал второго. Вызывается в главном потоке.
///
/// Открывает окно «Управление хранилищами» (задача 2.8б; уже открытое —
/// получает фокус, скрытое до проверки InPrivate не трогается). Не удалось
/// создать окно — выводит вперёд последнее активное показанное окно.
pub fn on_second_instance<R: Runtime>(app: &AppHandle<R>) {
    if crate::windows::open_manager(app).is_ok() {
        return;
    }
    focus_recent(app);
}

/// Выводит вперёд последнее активное показанное окно (свёрнутое
/// разворачивается). Скрытые окна не трогаются: `unminimize` показал бы окно
/// Windows до проверки InPrivate.
fn focus_recent<R: Runtime>(app: &AppHandle<R>) {
    let visible: Vec<String> = app
        .webview_windows()
        .into_iter()
        .filter(|(_, window)| window.is_visible().unwrap_or(false))
        .map(|(label, _)| label)
        .collect();
    let target = app.try_state::<Recent>().and_then(|recent| {
        recent
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .target(&visible)
    });
    if let Some(window) = target.and_then(|label| app.get_webview_window(&label)) {
        crate::windows::focus(&window);
    }
}
