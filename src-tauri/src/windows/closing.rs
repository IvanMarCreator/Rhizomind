//! Закрытие окна хранилища после записи ввода (задача 0.7, решение
//! владельца 53, инвариант 4 «без потери данных»).
//!
//! Пользователь закрывает окно хранилища (`CloseRequested`):
//! 1. размер и положение окна запоминаются ([`super::remember_placement`]) —
//!    до скрытия, пока окно их сообщает;
//! 2. закрытие откладывается (`prevent_close`), окно **сразу скрывается**;
//! 3. окну уходит событие `window:close-requested` (только ему, `emit_to`);
//! 4. интерфейс записывает несохранённый ввод всех заметок окна и отвечает:
//!    - `window_close_ready` — ввод записан: окно уничтожается;
//!    - `window_close_cancel` — запись не удалась: окно снова показывается и
//!      не закрывается (текст остаётся в редакторе, уведомление объясняет);
//! 5. нет ответа за [`CLOSE_TIMEOUT`] (диск не отвечает, интерфейс завис) —
//!    окно уничтожается без ответа, в журнал (stderr) — строка без имён и
//!    текста заметок: программа не висит вечно.
//!
//! Повторный `CloseRequested`, пока окно ждёт ответа, второго цикла не
//! начинает. Окно без хранилища (окно «Управление хранилищами», окно с
//! ошибкой запуска) закрывается сразу: ему нечего записывать.
//!
//! Выход из программы: последнее окно уничтожается только после ответа (или
//! предела), поэтому Tauri просит выхода (`ExitRequested`), когда ждать уже
//! нечего; если выход попросили раньше (`AppHandle::exit`), он откладывается
//! до конца всех ожиданий ([`exit_must_wait`], `lib.rs::handle_run_event`), а
//! временная папка движка удаляется после выхода, как и раньше.
//!
//! Ответы интерфейса действуют только на своё окно: окно выбирается по метке
//! окна вызова, а без ожидания ответ ничего не делает.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tauri::{Emitter, EventTarget, Manager, Runtime};

use crate::commands::{Core, WindowCloseRequestedPayload, event};

/// Сколько ядро ждёт ответа интерфейса после закрытия окна.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Ответ интерфейса на `window:close-requested`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Ввод записан: окно можно уничтожить.
    Ready,
    /// Запись не удалась: окно показывается снова и не закрывается.
    Cancel,
}

/// Где окно в цикле закрытия.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Окно скрыто и ждёт ответа интерфейса.
    Waiting,
    /// Ответ получен (или вышел предел): окно уничтожается.
    Destroying,
}

#[derive(Debug)]
enum Entry {
    Waiting(Sender<Answer>),
    Destroying,
}

/// Окна, которые закрываются (`Builder::manage`).
#[derive(Debug)]
pub struct Closing {
    timeout: Duration,
    windows: Mutex<HashMap<String, Entry>>,
    /// Окно вышло из цикла закрытия (уничтожено или показано снова).
    changed: Condvar,
}

impl Default for Closing {
    fn default() -> Self {
        Self::new(CLOSE_TIMEOUT)
    }
}

impl Closing {
    /// Учёт закрытий с пределом ожидания `timeout` (в программе —
    /// [`CLOSE_TIMEOUT`], в тестах — короче).
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            windows: Mutex::default(),
            changed: Condvar::new(),
        }
    }

    /// Предел ожидания ответа интерфейса.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn windows(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Начинает цикл закрытия окна `label`; окно уже закрывается — `None`.
    fn begin(&self, label: &str) -> Option<Receiver<Answer>> {
        let mut windows = self.windows();
        if windows.contains_key(label) {
            return None;
        }
        let (sender, receiver) = mpsc::channel();
        windows.insert(label.to_owned(), Entry::Waiting(sender));
        Some(receiver)
    }

    /// Где окно `label` в цикле закрытия; `None` — не закрывается.
    pub fn phase(&self, label: &str) -> Option<Phase> {
        self.windows().get(label).map(|entry| match entry {
            Entry::Waiting(_) => Phase::Waiting,
            Entry::Destroying => Phase::Destroying,
        })
    }

    /// Передаёт ответ интерфейса окну `label`, если оно ждёт ответа.
    pub fn answer(&self, label: &str, answer: Answer) -> bool {
        match self.windows().get(label) {
            Some(Entry::Waiting(sender)) => sender.send(answer).is_ok(),
            _ => false,
        }
    }

    fn mark_destroying(&self, label: &str) {
        if let Some(entry) = self.windows().get_mut(label) {
            *entry = Entry::Destroying;
        }
    }

    /// Окно вышло из цикла закрытия: уничтожено (`Destroyed`) или снова
    /// показано.
    pub fn finished(&self, label: &str) {
        if self.windows().remove(label).is_some() {
            self.changed.notify_all();
        }
    }

    /// Ни одно окно не закрывается.
    pub fn is_idle(&self) -> bool {
        self.windows().is_empty()
    }

    /// Ждёт, пока все окна выйдут из цикла закрытия, не дольше `limit`;
    /// `true` — дождались.
    pub fn wait_idle(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut windows = self.windows();
        while !windows.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            windows = self
                .changed
                .wait_timeout(windows, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// События окна для цикла закрытия (`Builder::on_window_event`):
/// `CloseRequested` — [`on_close_requested`], `Destroyed` — окно вышло из
/// цикла.
pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if on_close_requested(window) {
                api.prevent_close();
            }
        }
        tauri::WindowEvent::Destroyed => {
            if let Some(closing) = window.try_state::<Closing>() {
                closing.finished(window.label());
            }
        }
        _ => {}
    }
}

/// Пользователь закрывает окно. `true` — закрытие отложено до записи ввода
/// (вызывающий делает `prevent_close`); `false` — окно закрывается сразу.
pub fn on_close_requested<R: Runtime>(window: &tauri::Window<R>) -> bool {
    let label = window.label().to_owned();
    let Some(closing) = window.try_state::<Closing>() else {
        super::remember_placement(window);
        return false;
    };
    if closing.phase(&label).is_some() {
        // Окно уже ждёт записи: второго цикла нет.
        return true;
    }
    let has_vault = window
        .try_state::<Core>()
        .is_some_and(|core| core.window_vault(&label).is_some());
    if !has_vault {
        // Окно управления, окно без хранилища: записывать нечего.
        return false;
    }
    // Размер и положение — до скрытия: скрытое окно их может не сообщить.
    super::remember_placement(window);
    let Some(answers) = closing.begin(&label) else {
        return true;
    };
    let _ = window.hide();
    let _ = window.emit_to(
        EventTarget::webview_window(label.clone()),
        event::WINDOW_CLOSE_REQUESTED,
        WindowCloseRequestedPayload {},
    );
    let app = window.app_handle().clone();
    let timeout = closing.timeout();
    let waiter_label = label.clone();
    let waiter = std::thread::Builder::new()
        .name("rhizomind-close".into())
        .spawn(move || finish(&app, &waiter_label, &answers, timeout));
    if waiter.is_err() {
        // Поток не создать (нехватка ресурсов): ждать некому — закрыть сразу,
        // как раньше, а не держать скрытое окно вечно.
        closing.mark_destroying(&label);
        if window.destroy().is_err() {
            closing.finished(&label);
        }
    }
    true
}

/// Ждёт ответа интерфейса и закрывает окно (или показывает его снова).
fn finish<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
    answers: &Receiver<Answer>,
    timeout: Duration,
) {
    let Some(closing) = app.try_state::<Closing>() else {
        return;
    };
    let window = app.get_webview_window(label);
    let answer = match answers.recv_timeout(timeout) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => {
            // Без имён и текста заметок (инвариант 3).
            eprintln!(
                "Rhizomind: окно хранилища не подтвердило запись ввода за {} с и закрыто без подтверждения",
                timeout.as_secs()
            );
            Answer::Ready
        }
        Err(RecvTimeoutError::Disconnected) => Answer::Ready,
    };
    match (answer, window) {
        (Answer::Cancel, Some(window)) => {
            let _ = window.show();
            super::focus(&window);
            closing.finished(label);
        }
        (_, Some(window)) => {
            closing.mark_destroying(label);
            // Из цикла закрытия окно выводит событие `Destroyed`.
            if window.destroy().is_err() {
                closing.finished(label);
            }
        }
        (_, None) => closing.finished(label),
    }
}

/// Ответ интерфейса окна `window` (команды `window_close_ready`,
/// `window_close_cancel`): действует только на своё окно и только пока оно
/// ждёт ответа.
pub fn answer<R: Runtime>(window: &tauri::Window<R>, answer: Answer) {
    if let Some(closing) = window.try_state::<Closing>() {
        closing.answer(window.label(), answer);
    }
}

/// Выход из программы откладывается, пока какое-то окно ещё записывает ввод.
pub fn exit_must_wait<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.try_state::<Closing>()
        .is_some_and(|closing| !closing.is_idle())
}

/// Отложенный выход: дождаться всех закрытий (не дольше предела с запасом) и
/// выйти снова. Блокирует поток — вызывать вне главного.
pub fn exit_after_closing<R: Runtime>(app: &tauri::AppHandle<R>, code: i32) {
    if let Some(closing) = app.try_state::<Closing>() {
        closing.wait_idle(closing.timeout() + EXIT_GRACE);
    }
    app.exit(code);
}

/// Запас сверх предела ожидания для выхода: уничтожение окна идёт через цикл
/// событий.
const EXIT_GRACE: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_begin_for_the_same_window_starts_no_new_cycle() {
        let closing = Closing::default();
        assert!(closing.begin("vault-a").is_some());
        assert!(closing.begin("vault-a").is_none());
        assert_eq!(closing.phase("vault-a"), Some(Phase::Waiting));
        assert!(
            closing.begin("vault-b").is_some(),
            "другое окно — свой цикл"
        );
    }

    #[test]
    fn answer_reaches_only_a_waiting_window() {
        let closing = Closing::default();
        let answers = closing.begin("vault-a").expect("цикл");
        assert!(
            !closing.answer("vault-b", Answer::Ready),
            "чужое окно не ждёт"
        );
        assert!(closing.answer("vault-a", Answer::Cancel));
        assert_eq!(answers.try_recv(), Ok(Answer::Cancel));
        closing.mark_destroying("vault-a");
        assert!(
            !closing.answer("vault-a", Answer::Ready),
            "после ответа — не ждёт"
        );
    }

    #[test]
    fn wait_idle_returns_when_every_window_finished_or_at_limit() {
        let closing = std::sync::Arc::new(Closing::default());
        assert!(closing.wait_idle(Duration::ZERO), "закрытий нет");
        let _answers = closing.begin("vault-a").expect("цикл");
        assert!(!closing.is_idle());
        assert!(!closing.wait_idle(Duration::from_millis(30)), "предел");

        let other = std::sync::Arc::clone(&closing);
        let finisher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            other.finished("vault-a");
        });
        assert!(closing.wait_idle(Duration::from_secs(5)));
        assert!(closing.is_idle());
        finisher.join().expect("поток");
    }
}
