//! Запись ввода перед концом сеанса Windows (задача 0.11): логика без ОС —
//! подменный runtime Tauri, ответы интерфейса из другого потока.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewWindow};

use super::*;
use crate::commands::{Core, VaultId};
use crate::registry::Registry;
use crate::windows::closing::{self, Closing, Phase};

const LOCAL_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

fn app() -> (App<MockRuntime>, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("папка");
    let data_dir =
        crate::webview_traces::WebviewDataDir::prepare_in(temp.path(), std::process::id())
            .expect("папка данных");
    let app = mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .manage(crate::Shell::new(data_dir, crate::security::browser_args()))
        .manage(Core::new(Registry::default()))
        .manage(Closing::default())
        .manage(SessionEnd::default())
        .invoke_handler(crate::commands::handler())
        .on_window_event(crate::commands::on_window_event)
        .build(tauri::generate_context!(test = true))
        .expect("тестовое приложение");
    (app, temp)
}

fn call(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: LOCAL_URL.parse().expect("адрес"),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("ответ JSON"))
}

/// Окно с открытым хранилищем `name`.
fn vault_window(app: &App<MockRuntime>, parent: &Path, name: &str) -> WebviewWindow<MockRuntime> {
    let root = parent.join(name);
    std::fs::create_dir_all(&root).expect("папка");
    std::fs::write(root.join("Заметка.md"), "текст").expect("заметка");
    let id: VaultId = app
        .state::<Core>()
        .registry()
        .add(&root)
        .expect("в списке")
        .id;
    let window =
        crate::create_window(app.handle(), &crate::windows::new_label(), None).expect("окно");
    call(&window, "vault_open", json!({ "request": { "id": id.0 } })).expect("vault_open");
    window
}

/// Считает события `window:close-requested`, пришедшие окну.
fn close_events(window: &WebviewWindow<MockRuntime>) -> Arc<AtomicUsize> {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    window.listen(crate::commands::event::WINDOW_CLOSE_REQUESTED, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    count
}

/// Ответ интерфейса окна `label` из другого потока через `delay`.
fn answer_later(
    app: &App<MockRuntime>,
    label: &str,
    delay: Duration,
) -> std::thread::JoinHandle<()> {
    let handle = app.handle().clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        assert!(
            handle
                .state::<Closing>()
                .answer(&label, closing::Answer::Ready),
            "окно {label} ждёт ответа"
        );
    })
}

#[test]
fn every_vault_window_is_asked_and_awaited() {
    let (app, temp) = app();
    let first = vault_window(&app, temp.path(), "А");
    let second = vault_window(&app, temp.path(), "Б");
    let manager = crate::create_manager_window(app.handle(), "manager-test").expect("окно");
    let (first_events, second_events, manager_events) = (
        close_events(&first),
        close_events(&second),
        close_events(&manager),
    );
    let answers = [
        answer_later(&app, first.label(), Duration::from_millis(50)),
        answer_later(&app, second.label(), Duration::from_millis(150)),
    ];

    let started = Instant::now();
    let mut pumps = 0;
    let outcome = save_all(app.handle(), Duration::from_secs(3), || pumps += 1);
    for answer in answers {
        answer.join().expect("ответ");
    }

    assert_eq!(
        outcome,
        Outcome {
            asked: 2,
            answered: 2,
            timed_out: false
        }
    );
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "ждали второе окно"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "не ждали предела"
    );
    assert!(pumps > 0, "очередь сообщений разбиралась во время ожидания");
    assert_eq!(first_events.load(Ordering::SeqCst), 1);
    assert_eq!(second_events.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager_events.load(Ordering::SeqCst),
        0,
        "окну без хранилища писать нечего"
    );
    // Окна не закрываются: выход могут отменить.
    assert!(app.get_webview_window(first.label()).is_some());
    assert!(app.get_webview_window(second.label()).is_some());
    assert!(app.state::<Closing>().is_idle(), "окна вышли из ожидания");
}

#[test]
fn waiting_ends_at_the_limit_without_answers() {
    let (app, temp) = app();
    let first = vault_window(&app, temp.path(), "А");
    let events = close_events(&first);

    let started = Instant::now();
    let outcome = save_all(app.handle(), Duration::from_millis(200), || {});

    assert_eq!(
        outcome,
        Outcome {
            asked: 1,
            answered: 0,
            timed_out: true
        }
    );
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(200),
        "ждали до предела: {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(1),
        "не дольше предела: {waited:?}"
    );
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert!(app.state::<Closing>().is_idle());
    // Поздний ответ ничего не делает.
    assert!(
        !app.state::<Closing>()
            .answer(first.label(), closing::Answer::Ready)
    );
}

#[test]
fn window_already_closing_is_not_asked_again_but_awaited() {
    let (app, temp) = app();
    let first = vault_window(&app, temp.path(), "А");
    let events = close_events(&first);
    assert!(closing::on_close_requested(&first.as_ref().window()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while events.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "событие закрытия");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        app.state::<Closing>().phase(first.label()),
        Some(Phase::Waiting)
    );
    let answer = answer_later(&app, first.label(), Duration::from_millis(100));

    let started = Instant::now();
    let outcome = save_all(app.handle(), Duration::from_secs(3), || {});
    let waited = started.elapsed();
    answer.join().expect("ответ");

    assert_eq!(outcome.asked, 0, "второго запроса записи нет");
    assert!(!outcome.timed_out);
    assert!(
        waited >= Duration::from_millis(100),
        "дождались закрытия окна: {waited:?}"
    );
    assert_eq!(events.load(Ordering::SeqCst), 1);
}

#[test]
fn nested_request_while_saving_returns_at_once() {
    let (app, temp) = app();
    let first = vault_window(&app, temp.path(), "А");
    let events = close_events(&first);
    let handle = app.handle().clone();
    let mut nested = None;
    let mut answered = false;
    let label = first.label().to_owned();
    let outcome = save_all(app.handle(), Duration::from_secs(3), || {
        // Повторное WM_QUERYENDSESSION из вложенного разбора очереди.
        if nested.is_none() {
            nested = Some(save_all(&handle, Duration::from_secs(3), || {}));
        }
        if !answered {
            answered = handle
                .state::<Closing>()
                .answer(&label, closing::Answer::Ready);
        }
    });

    assert_eq!(nested.expect("вложенный вызов").asked, 0);
    assert_eq!(outcome.answered, 1);
    assert_eq!(events.load(Ordering::SeqCst), 1);
}

#[test]
fn session_end_marks_exit_and_browser_is_ended_once() {
    let (app, _temp) = app();
    assert!(!is_ending(app.handle()));
    let _ = save_all(app.handle(), Duration::from_millis(10), || {});
    assert!(is_ending(app.handle()), "после запроса — сеанс завершается");

    on_end_session(app.handle(), false);
    assert!(!is_ending(app.handle()), "выход отменён");
    assert!(!closing::is_exiting(app.handle()));

    let _ = save_all(app.handle(), Duration::from_millis(10), || {});
    on_end_session(app.handle(), true);
    assert!(closing::is_exiting(app.handle()), "новых окон не создавать");
    let session = app.state::<SessionEnd>();
    assert!(!session.claim_browser_end(), "браузер уже завершён");
}

#[test]
fn block_reason_is_a_constant_without_user_data() {
    assert_eq!(BLOCK_REASON, "Rhizomind сохраняет заметки…");
    assert!(SESSION_END_TIMEOUT < Duration::from_secs(5));
}
