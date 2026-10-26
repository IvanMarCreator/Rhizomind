//! Запись ввода при выходе из системы и выключении Windows (задача 0.11,
//! инвариант 4 «без потери данных»; ревью 0.7, W1).
//!
//! Tauri (tao 0.37) `WM_QUERYENDSESSION` не обрабатывает, а на
//! `WM_ENDSESSION` своего скрытого окна сразу вызывает `std::process::exit(0)`:
//! `CloseRequested` не приходит, и несохранённый ввод терялся. Поэтому у
//! программы своё скрытое окно верхнего уровня ([`windows`]) — Windows шлёт
//! эти сообщения всем окнам верхнего уровня, и скрытым тоже.
//!
//! 1. `WM_QUERYENDSESSION` (Windows спрашивает, можно ли завершить сеанс):
//!    причина «Rhizomind сохраняет заметки…» ([`BLOCK_REASON`],
//!    `ShutdownBlockReasonCreate`), затем [`save_all`]: каждому окну
//!    хранилища — то же событие `window:close-requested`, что при закрытии
//!    окна (задача 0.7); интерфейс записывает ввод всех заметок окна и
//!    отвечает `window_close_ready`/`window_close_cancel`. Ответы ждутся не
//!    дольше [`SESSION_END_TIMEOUT`] (Windows ждёт ответа ~5 с), очередь
//!    сообщений главного потока при этом разбирается — иначе движок и вызовы
//!    ядра из интерфейса стояли бы. Окна не скрываются и не закрываются:
//!    выход могут отменить. Ответ Windows — всегда «можно» (TRUE): программа
//!    выход пользователя не блокирует; причина снимается.
//! 2. `WM_ENDSESSION` с `wParam = TRUE` (сеанс завершается): [`on_end_session`]
//!    — признак выхода (новых окон нет) и завершение процесса браузера
//!    WebView2 (решение владельца 64: штатное закрытие движка загружает
//!    OneAuth). Потом tao вызывает `exit(0)`, но перед этим — `RunEvent::Exit`
//!    (`lib.rs::handle_run_event`): имя экземпляра освобождается, временная
//!    папка движка удаляется. Если Windows отдала `WM_ENDSESSION` окну tao
//!    раньше нашего, браузер завершается там же, в `RunEvent::Exit`
//!    ([`end_browser_if_ending`]).
//!    `wParam = FALSE` — выход отменён: программа работает дальше.
//!
//! Что не покрыто: аварийное завершение процесса («Диспетчер задач» →
//! «Снять задачу», `taskkill /f`, отключение питания) — ОС не спрашивает
//! программу, ввод последней секунды может потеряться.
//!
//! Без сети: только сообщения окон Windows в своём процессе.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime};

use crate::commands::{Core, WindowCloseRequestedPayload, event};
use crate::windows::closing::{self, Answer, Closing, Phase};

#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(crate) mod windows;

/// Сколько программа ждёт записи ввода после `WM_QUERYENDSESSION`: меньше
/// 5 с, которые Windows даёт программе до окна «Эти приложения мешают
/// завершению».
pub const SESSION_END_TIMEOUT: Duration = Duration::from_secs(4);

/// Причина, которую Windows показывает, пока программа записывает ввод.
/// Постоянная строка: без имён хранилищ и заметок.
pub const BLOCK_REASON: &str = "Rhizomind сохраняет заметки…";

/// Пауза между проверками ответов, пока разбирается очередь сообщений.
const POLL_STEP: Duration = Duration::from_millis(5);

/// Состояние конца сеанса (`Builder::manage`).
#[derive(Debug, Default)]
pub struct SessionEnd {
    /// Windows спросила о конце сеанса, и выход не отменён.
    ending: AtomicBool,
    /// Идёт [`save_all`]: вложенный запрос (из разбора очереди) не повторяет.
    saving: AtomicBool,
    /// Процесс браузера уже завершён концом сеанса.
    browser_ended: AtomicBool,
}

impl SessionEnd {
    /// Право завершить процесс браузера: `true` — только первому.
    pub fn claim_browser_end(&self) -> bool {
        !self.browser_ended.swap(true, Ordering::SeqCst)
    }
}

/// Чем кончилось ожидание записи.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Скольким окнам хранилищ ушёл запрос записи.
    pub asked: usize,
    /// Сколько из них ответили.
    pub answered: usize,
    /// Предел вышел раньше, чем ответили все.
    pub timed_out: bool,
}

/// Окно, которое ждёт ответа.
enum Pending {
    /// Запрос записи — от конца сеанса: ответ придёт сюда.
    Asked(String, Receiver<Answer>),
    /// Окно уже закрывалось пользователем (задача 0.7): ждать, пока его
    /// собственный цикл получит ответ.
    Closing(String),
}

/// `WM_QUERYENDSESSION`: записать ввод всех окон хранилищ, не дольше
/// `limit`. `pump` разбирает очередь сообщений главного потока (вызов — из
/// оконной процедуры, ответы интерфейса идут через ту же очередь).
pub fn save_all<R: Runtime>(
    app: &AppHandle<R>,
    limit: Duration,
    mut pump: impl FnMut(),
) -> Outcome {
    let deadline = Instant::now() + limit;
    let (Some(session), Some(closing)) =
        (app.try_state::<SessionEnd>(), app.try_state::<Closing>())
    else {
        return Outcome::default();
    };
    session.ending.store(true, Ordering::SeqCst);
    if session.saving.swap(true, Ordering::SeqCst) {
        return Outcome::default();
    }
    let mut pending = ask_every_vault_window(app, &closing);
    let asked = pending
        .iter()
        .filter(|entry| matches!(entry, Pending::Asked(..)))
        .count();
    let mut answered = 0;
    loop {
        pending.retain(|entry| match entry {
            Pending::Asked(label, answers) => match answers.try_recv() {
                Ok(_) => {
                    answered += 1;
                    closing.finished(label);
                    false
                }
                Err(TryRecvError::Disconnected) => {
                    closing.finished(label);
                    false
                }
                Err(TryRecvError::Empty) => true,
            },
            Pending::Closing(label) => closing.phase(label) == Some(Phase::Waiting),
        });
        if pending.is_empty() || Instant::now() >= deadline {
            break;
        }
        pump();
        std::thread::sleep(POLL_STEP);
    }
    let timed_out = !pending.is_empty();
    for entry in pending {
        if let Pending::Asked(label, _) = entry {
            closing.finished(&label);
        }
    }
    if timed_out {
        // Без имён и текста заметок (инвариант 3).
        eprintln!(
            "Rhizomind: конец сеанса Windows — не все окна подтвердили запись ввода за {} мс",
            limit.as_millis()
        );
    }
    session.saving.store(false, Ordering::SeqCst);
    Outcome {
        asked,
        answered,
        timed_out,
    }
}

/// Каждому окну с открытым хранилищем — запрос записи; окна, которые уже
/// закрываются, — только в ожидание.
fn ask_every_vault_window<R: Runtime>(app: &AppHandle<R>, closing: &Closing) -> Vec<Pending> {
    let core = app.try_state::<Core>();
    let mut pending = Vec::new();
    for label in app.webview_windows().into_keys() {
        if !core
            .as_ref()
            .is_some_and(|core| core.window_vault(&label).is_some())
        {
            continue;
        }
        match closing.phase(&label) {
            Some(Phase::Waiting) => pending.push(Pending::Closing(label)),
            Some(Phase::Destroying) => {}
            None => {
                let Some(answers) = closing.begin(&label) else {
                    continue;
                };
                let _ = app.emit_to(
                    EventTarget::webview_window(label.clone()),
                    event::WINDOW_CLOSE_REQUESTED,
                    WindowCloseRequestedPayload {},
                );
                pending.push(Pending::Asked(label, answers));
            }
        }
    }
    pending
}

/// `WM_ENDSESSION`: `ending = true` — сеанс завершается, процесс вот-вот
/// будет завершён: новых окон не создавать, процесс браузера завершить
/// (решение 64). `false` — выход отменён, программа работает дальше.
pub fn on_end_session<R: Runtime>(app: &AppHandle<R>, ending: bool) {
    let Some(session) = app.try_state::<SessionEnd>() else {
        return;
    };
    session.ending.store(ending, Ordering::SeqCst);
    if ending {
        closing::begin_exit(app);
        end_browser_if_ending(app);
    }
}

/// Сеанс Windows завершается (был `WM_QUERYENDSESSION`, выход не отменён).
pub fn is_ending<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<SessionEnd>()
        .is_some_and(|session| session.ending.load(Ordering::SeqCst))
}

/// Сеанс завершается — процесс браузера завершается один раз: из
/// [`on_end_session`] или из `RunEvent::Exit`, смотря какое окно Windows
/// известила первым.
pub fn end_browser_if_ending<R: Runtime>(app: &AppHandle<R>) {
    let Some(session) = app.try_state::<SessionEnd>() else {
        return;
    };
    if session.ending.load(Ordering::SeqCst) && session.claim_browser_end() {
        closing::end_browser(app);
    }
}

/// Windows: скрытое окно, которое получает `WM_QUERYENDSESSION` и
/// `WM_ENDSESSION`. Вызывать в главном потоке (в `setup`). Не создалось —
/// программа работает, как до задачи 0.11 (в stderr — строка).
pub fn listen<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(windows)]
    {
        let handle = app.clone();
        let started = windows::listen(Box::new(move |message| match message {
            windows::Message::QueryEndSession => {
                save_all(&handle, SESSION_END_TIMEOUT, windows::pump_messages);
            }
            windows::Message::EndSession(ending) => on_end_session(&handle, ending),
        }));
        if let Err(error) = started {
            eprintln!("Rhizomind: окно конца сеанса Windows не создано: {error}");
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}
