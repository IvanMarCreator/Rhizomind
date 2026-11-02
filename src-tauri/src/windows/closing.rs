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
//!
//! **Индекс хранилища** (задача 0.2-1.3, С2-4.1): после ответа
//! `window_close_ready` (заметки записаны) и до уничтожения окна пишется
//! `.rhizomind/index.json` — запись, которую окно зарегистрировало
//! ([`Closing::set_index_writer`], задача 0.2-1.4). Только если до предела
//! закрытия (решение 67, отсчёт от `CloseRequested`) остаётся не меньше
//! [`INDEX_CLOSE_RESERVE`]; запись дольше оставшегося времени окно не
//! держит. Иначе индекс не пишется: при следующем открытии сверка доберёт
//! изменения. Без ответа (предел вышел) и при отмене индекс не пишется.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tauri::{Emitter, EventTarget, Manager, Runtime};

use crate::commands::{Core, VaultId, WindowCloseRequestedPayload, event};

/// Сколько ядро ждёт ответа интерфейса после закрытия окна.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Запас до предела закрытия, без которого индекс при закрытии не пишется.
pub const INDEX_CLOSE_RESERVE: Duration = Duration::from_secs(2);

/// Запись индекса хранилища окна при закрытии: пишет сразу, без срока
/// (`index::store::IndexStore::write_now`); ошибки — молча.
pub type IndexWriter = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Успеет ли запись индекса: до предела закрытия `deadline` от `now`
/// остаётся не меньше [`INDEX_CLOSE_RESERVE`].
pub fn index_write_fits(now: Instant, deadline: Instant) -> bool {
    deadline.saturating_duration_since(now) >= INDEX_CLOSE_RESERVE
}

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
    /// Окна, уничтоженные после закрытия (метки окон не повторяются).
    gone: Mutex<std::collections::HashSet<String>>,
    /// Выход начат: последнее окно закрывается, процесс браузера завершён
    /// (решение 64) — новых окон не создавать (ревью 0.7, W6).
    exiting: std::sync::atomic::AtomicBool,
    /// Окно вышло из цикла закрытия (уничтожено или показано снова).
    changed: Condvar,
    /// Запись индекса хранилища по метке окна.
    index_writers: IndexWriters,
}

/// Записи индекса по метке окна.
#[derive(Default)]
struct IndexWriters(Mutex<HashMap<String, IndexWriter>>);

impl std::fmt::Debug for IndexWriters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let writers = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_set().entries(writers.keys()).finish()
    }
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
            gone: Mutex::default(),
            exiting: std::sync::atomic::AtomicBool::new(false),
            changed: Condvar::new(),
            index_writers: IndexWriters::default(),
        }
    }

    /// Запись индекса хранилища окна `label` при закрытии (окно открыло
    /// хранилище — задача 0.2-1.4). Окно уничтожено — запись забывается.
    pub fn set_index_writer(&self, label: &str, writer: IndexWriter) {
        self.index_writers
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_owned(), writer);
    }

    fn index_writer(&self, label: &str) -> Option<IndexWriter> {
        self.index_writers
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(label)
            .cloned()
    }

    /// После записи заметок: пишет индекс окна `label`, если он есть и
    /// успевает до `deadline` с запасом. Запись идёт в отдельном потоке;
    /// дольше `deadline` её не ждут (атомарная запись, оборванная выходом,
    /// оставляет только временный файл в `.rhizomind/tmp/`, его уберёт
    /// следующее открытие). `true` — запись начата.
    fn write_index(&self, label: &str, deadline: Instant) -> bool {
        let Some(writer) = self.index_writer(label) else {
            return false;
        };
        if !index_write_fits(Instant::now(), deadline) {
            return false;
        }
        let (done, wait) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("rhizomind-close-index".into())
            .spawn(move || {
                writer();
                let _ = done.send(());
            });
        if started.is_ok() {
            let _ = wait.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        }
        started.is_ok()
    }

    /// Предел ожидания ответа интерфейса.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn windows(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Начинает цикл закрытия окна `label`; окно уже закрывается — `None`.
    /// Конец сеанса Windows (`crate::session_end`) ждёт ответа так же, но
    /// окно не уничтожает: из ожидания его выводит [`Self::finished`].
    pub(crate) fn begin(&self, label: &str) -> Option<Receiver<Answer>> {
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

    /// Окно уничтожено (`Destroyed`): вышло из цикла закрытия навсегда.
    pub fn destroyed(&self, label: &str) {
        self.index_writers
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(label);
        self.gone
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_owned());
        self.finished(label);
    }

    fn is_gone(&self, label: &str) -> bool {
        self.gone
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(label)
    }

    /// Выход начат (последнее окно закрывается) — новых окон не будет.
    pub fn is_exiting(&self) -> bool {
        self.exiting.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn begin_exit(&self) {
        self.exiting
            .store(true, std::sync::atomic::Ordering::SeqCst);
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
                closing.destroyed(window.label());
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
        // Окно управления, окно без хранилища: записывать нечего. Последнее
        // окно программы — тоже через завершение браузера (решение 64).
        if !is_last_window(window.app_handle(), &label) {
            return false;
        }
        if closing.begin(&label).is_none() {
            return true;
        }
        closing.mark_destroying(&label);
        let _ = window.hide();
        let app = window.app_handle().clone();
        let closer_label = label.clone();
        let closer = std::thread::Builder::new()
            .name("rhizomind-close".into())
            .spawn(move || {
                if let Some(window) = app.get_webview_window(&closer_label) {
                    destroy(&app, &closer_label, &window);
                }
            });
        if closer.is_err() {
            destroy(window.app_handle(), &label, window);
        }
        return true;
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
        WindowCloseRequestedPayload { session_end: false },
    );
    let app = window.app_handle().clone();
    let timeout = closing.timeout();
    let deadline = Instant::now() + timeout;
    let waiter_label = label.clone();
    let waiter = std::thread::Builder::new()
        .name("rhizomind-close".into())
        .spawn(move || finish(&app, &waiter_label, &answers, deadline));
    if waiter.is_err() {
        // Поток не создать (нехватка ресурсов): ждать некому — закрыть сразу,
        // как раньше, а не держать скрытое окно вечно.
        destroy(window.app_handle(), &label, window);
    }
    true
}

/// Ждёт ответа интерфейса до `deadline` и закрывает окно (или показывает
/// его снова); после записи заметок — индекс, если успевает.
fn finish<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
    answers: &Receiver<Answer>,
    deadline: Instant,
) {
    let Some(closing) = app.try_state::<Closing>() else {
        return;
    };
    let timeout = deadline.saturating_duration_since(Instant::now());
    let answer = match answers.recv_timeout(timeout) {
        Ok(Answer::Ready) => {
            // Заметки записаны — индекс после них (С2-4.1, решение 67).
            closing.write_index(label, deadline);
            Answer::Ready
        }
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => {
            // Без имён и текста заметок (инвариант 3).
            eprintln!(
                "Rhizomind: окно хранилища не подтвердило запись ввода за {} с и закрыто без подтверждения",
                closing.timeout().as_secs()
            );
            Answer::Ready
        }
        Err(RecvTimeoutError::Disconnected) => Answer::Ready,
    };
    let window = app.get_webview_window(label);
    match (answer, window) {
        (Answer::Cancel, Some(window)) if let Some(name) = lost_storage(app, label) => {
            // Решение владельца 66: хранилище на внешнем носителе пропало —
            // окно не возвращается (ввод записать некуда), а закрывается с
            // системным сообщением; сначала сообщение, потом закрытие.
            crate::dialogs::tell_closed_unsaved(app, label, &name);
            destroy(app, label, &window);
        }
        (Answer::Cancel, Some(window)) => {
            // Сначала — вне цикла закрытия: показанное окно сразу можно
            // закрыть снова (ревью 0.7, W5).
            closing.finished(label);
            let _ = window.show();
            super::focus(&window);
        }
        (_, Some(window)) => destroy(app, label, &window),
        (_, None) => closing.finished(label),
    }
}

/// Уничтожает окно после записи ввода. Последнее окно программы — после
/// завершения процесса браузера WebView2 (решение владельца 64): штатное
/// закрытие загружает модуль OneAuth движка, и ОС обращается к службе входа
/// Microsoft; завершённый браузер этого не делает. Из цикла закрытия окно
/// выводит событие `Destroyed`.
fn destroy<R: Runtime, W: WindowLike>(app: &tauri::AppHandle<R>, label: &str, window: &W) {
    // Признак выхода — раньше, чем окно видно уничтожаемым: кто увидел окно
    // «уничтожается», видит и начатый выход (ревью 0.7, W6).
    let last = is_last_window(app, label);
    if let Some(closing) = app.try_state::<Closing>() {
        if last {
            closing.begin_exit();
        }
        closing.mark_destroying(label);
    }
    if last {
        end_browser(app);
    }
    if window.destroy_window().is_err()
        && let Some(closing) = app.try_state::<Closing>()
    {
        closing.finished(label);
    }
}

/// Окно, которое можно уничтожить (`Window` и `WebviewWindow`).
trait WindowLike {
    fn destroy_window(&self) -> tauri::Result<()>;
}

impl<R: Runtime> WindowLike for tauri::Window<R> {
    fn destroy_window(&self) -> tauri::Result<()> {
        self.destroy()
    }
}

impl<R: Runtime> WindowLike for tauri::WebviewWindow<R> {
    fn destroy_window(&self) -> tauri::Result<()> {
        self.destroy()
    }
}

/// Окно `label` — последнее окно программы: все другие уже уничтожаются или
/// уничтожены. Окно, которое ещё ждёт записи, — не уничтожаемое: последним
/// будет оно.
pub fn is_last_window<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) -> bool {
    let closing = app.try_state::<Closing>();
    app.webview_windows().keys().all(|other| {
        other == label
            || closing.as_ref().is_some_and(|closing| {
                closing.is_gone(other) || closing.phase(other) == Some(Phase::Destroying)
            })
    })
}

/// Завершает процесс браузера WebView2 (решение 64), если он известен.
pub(crate) fn end_browser<R: Runtime>(app: &tauri::AppHandle<R>) {
    // Тестовая сборка: контрольный прогон сценарного теста — штатное закрытие.
    #[cfg(feature = "webdriver")]
    if crate::windows::e2e::keep_browser() {
        return;
    }
    if let Some(shell) = app.try_state::<crate::Shell>() {
        shell.terminate_browser();
    }
}

/// Имя хранилища окна `label`, если оно было на внешнем носителе и стало
/// недоступно (решение владельца 66, `vault::fsops::storage_lost`).
fn lost_storage<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) -> Option<String> {
    let open = app.try_state::<Core>()?.window_vault(label)?;
    crate::vault::fsops::storage_lost(open.vault.root()).then(|| open.summary.name.clone())
}

/// Ответ интерфейса окна `window` (команды `window_close_ready`,
/// `window_close_cancel`): действует только на своё окно и только пока оно
/// ждёт ответа.
pub fn answer<R: Runtime>(window: &tauri::Window<R>, answer: Answer) {
    if let Some(closing) = window.try_state::<Closing>() {
        closing.answer(window.label(), answer);
    }
}

/// Выход начат (конец сеанса Windows, `crate::session_end`): новых окон не
/// создавать.
pub(crate) fn begin_exit<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(closing) = app.try_state::<Closing>() {
        closing.begin_exit();
    }
}

/// Выход начат: новых окон не создавать.
pub fn is_exiting<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.try_state::<Closing>()
        .is_some_and(|closing| closing.is_exiting())
}

/// Окно `label` закрывается (скрыто, пишет ввод)?
pub fn is_closing<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) -> bool {
    app.try_state::<Closing>()
        .is_some_and(|closing| closing.phase(label).is_some())
}

/// Ждёт конца закрытия окна `label`, в котором открыто хранилище `vault`
/// (ревью 0.7, W2): окно показано снова — или уничтожено и отвязано от
/// хранилища. Не дольше предела ожидания с запасом. Блокирует поток.
pub fn wait_closed<R: Runtime>(app: &tauri::AppHandle<R>, label: &str, vault: &VaultId) {
    let Some(closing) = app.try_state::<Closing>() else {
        return;
    };
    let deadline = Instant::now() + closing.timeout() + EXIT_GRACE;
    while Instant::now() < deadline {
        let settled = closing.phase(label).is_none()
            && (!closing.is_gone(label)
                || app
                    .try_state::<Core>()
                    .is_none_or(|core| core.window_of_vault(vault).as_deref() != Some(label)));
        if settled {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Выход из программы откладывается, пока какое-то окно ещё записывает ввод.
pub fn exit_must_wait<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.try_state::<Closing>()
        .is_some_and(|closing| !closing.is_idle())
}

/// Отложенный выход: дождаться всех закрытий (не дольше предела с запасом).
/// Явный выход (`code`) повторяется; выход после последнего окна (`None`)
/// наступит сам, когда уничтожится последнее окно (ревью 0.7, W3): `exit`
/// при ещё живом WebView оставил бы временную папку движка.
/// Блокирует поток — вызывать вне главного.
pub fn exit_after_closing<R: Runtime>(app: &tauri::AppHandle<R>, code: Option<i32>) {
    if let Some(closing) = app.try_state::<Closing>() {
        closing.wait_idle(closing.timeout() + EXIT_GRACE);
    }
    if let Some(code) = code {
        app.exit(code);
    }
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
