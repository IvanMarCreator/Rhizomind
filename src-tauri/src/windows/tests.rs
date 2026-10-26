//! Окна хранилищ: метки, права, фокус на уже открытое, положение окна.

use std::path::Path;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow};

use super::*;
use crate::commands::{VaultId, VaultListResponse};
use crate::registry::Registry;

const LOCAL_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

/// Приложение как в `lib.rs::run`: оболочка 0.3, ядро со списком хранилищ
/// в памяти, права окон — из `capabilities/main.json`.
fn app() -> (App<MockRuntime>, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("папка");
    let data_dir =
        crate::webview_traces::WebviewDataDir::prepare_in(temp.path(), std::process::id())
            .expect("папка данных");
    let app = mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .manage(crate::Shell::new(data_dir, crate::security::browser_args()))
        .manage(Core::new(Registry::default()))
        .invoke_handler(crate::commands::handler())
        .build(tauri::generate_context!(test = true))
        .expect("тестовое приложение");
    (app, temp)
}

fn window(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    crate::create_window(app.handle(), &new_label(), None).expect("окно")
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

fn vault(app: &App<MockRuntime>, parent: &Path, name: &str) -> VaultId {
    let root = parent.join(name);
    std::fs::create_dir_all(&root).expect("папка");
    std::fs::write(root.join("Заметка.md"), "текст").expect("заметка");
    app.state::<Core>()
        .registry()
        .add(&root)
        .expect("в списке")
        .id
}

fn open(window: &WebviewWindow<MockRuntime>, id: &VaultId) -> Value {
    call(window, "vault_open", json!({ "request": { "id": id.0 } })).expect("vault_open")
}

fn window_count(app: &App<MockRuntime>) -> usize {
    app.webview_windows().len()
}

// ---- Метки и права ------------------------------------------------------------

#[test]
fn labels_are_unique_and_match_capability_pattern() {
    let labels: std::collections::BTreeSet<String> = (0..100).map(|_| new_label()).collect();
    assert_eq!(labels.len(), 100);
    for label in &labels {
        let random = label.strip_prefix(LABEL_PREFIX).expect("начало vault-");
        assert_eq!(random.len(), 16, "{label}");
        assert!(random.chars().all(|c| c.is_ascii_hexdigit()), "{label}");
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities/main.json");
    let capability: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("файл")).expect("JSON");
    assert_eq!(capability["windows"], json!([format!("{LABEL_PREFIX}*")]));
}

/// Новое окно получает те же права, что и первое: команды ядра работают, а
/// окно с чужой меткой (не `vault-*`) — нет.
#[test]
fn every_vault_window_gets_the_same_capability() {
    let (app, _temp) = app();
    let first = window(&app);
    let second = window(&app);
    for w in [&first, &second] {
        let list: VaultListResponse =
            serde_json::from_value(call(w, "vault_list", json!({})).expect("права окна"))
                .expect("ответ");
        assert!(list.vaults.is_empty());
    }
    let stranger = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .expect("окно");
    let error = call(&stranger, "vault_list", json!({})).expect_err("без прав");
    assert!(error.to_string().contains("not allowed"), "{error}");
}

// ---- Открытие и фокус -----------------------------------------------------------

/// Критерий плана 1.3: повторное открытие хранилища даёт фокус, а не второе
/// окно — откуда бы ни пришёл вызов.
#[test]
fn reopening_an_open_vault_focuses_its_window_instead_of_a_second_one() {
    let (app, temp) = app();
    let a = vault(&app, temp.path(), "А");
    let b = vault(&app, temp.path(), "Б");
    let first = window(&app);

    // Окно без хранилища открывает хранилище в себе.
    assert_eq!(open(&first, &a), json!({ "outcome": "opened" }));
    assert_eq!(window_count(&app), 1);
    let core = app.state::<Core>();
    assert_eq!(core.window_of_vault(&a).as_deref(), Some(first.label()));

    // Другое хранилище — в новом окне с меткой `vault-*`.
    assert_eq!(open(&first, &b), json!({ "outcome": "opened" }));
    assert_eq!(window_count(&app), 2);
    let second = core.window_of_vault(&b).expect("Б открыто");
    assert_ne!(second, first.label());
    assert!(second.starts_with(LABEL_PREFIX), "{second}");
    let second = app.get_webview_window(&second).expect("окно Б");

    // Повторно — фокус, окон не прибавляется: из того же окна, из окна
    // этого хранилища и из окна другого хранилища.
    for (from, id) in [(&first, &b), (&second, &b), (&second, &a), (&first, &a)] {
        assert_eq!(open(from, id), json!({ "outcome": "focused" }));
        assert_eq!(window_count(&app), 2);
    }
    assert_eq!(core.window_of_vault(&a).as_deref(), Some(first.label()));
    assert_eq!(core.window_of_vault(&b).as_deref(), Some(second.label()));

    // Хранилище нового окна видно его интерфейсу первой командой.
    let current = call(&second, "vault_current", json!({})).expect("vault_current");
    assert_eq!(current["vault"]["id"], json!(b.0));

    // Последнее открытое — Б; список знает, где что открыто.
    let list: VaultListResponse =
        serde_json::from_value(call(&first, "vault_list", json!({})).expect("список"))
            .expect("ответ");
    assert_eq!(list.last_opened, Some(b.clone()));
    assert!(list.vaults.iter().all(|v| v.is_open));
}

/// Два одновременных открытия одного хранилища: одно окно, второй вызов —
/// фокус.
#[test]
fn concurrent_opens_of_one_vault_create_one_window() {
    let (app, temp) = app();
    let a = vault(&app, temp.path(), "А");
    let b = vault(&app, temp.path(), "Б");
    let first = window(&app);
    open(&first, &a);

    let callers: Vec<_> = (0..4)
        .map(|_| {
            let w = first.clone();
            let id = b.clone();
            std::thread::spawn(move || open(&w, &id))
        })
        .collect();
    let outcomes: Vec<Value> = callers
        .into_iter()
        .map(|t| t.join().expect("поток"))
        .collect();
    let opened = outcomes
        .iter()
        .filter(|o| **o == json!({ "outcome": "opened" }))
        .count();
    assert_eq!(opened, 1, "{outcomes:?}");
    assert_eq!(window_count(&app), 2);
}

/// Хранилище, которого нет в списке, и недоступная папка — ошибка, окно не
/// создаётся.
#[test]
fn unknown_or_unavailable_vault_opens_no_window() {
    let (app, temp) = app();
    let a = vault(&app, temp.path(), "А");
    let gone = vault(&app, temp.path(), "Пропало");
    std::fs::remove_dir_all(temp.path().join("Пропало")).expect("удалено");
    let first = window(&app);
    open(&first, &a);

    let error = call(
        &first,
        "vault_open",
        json!({ "request": { "id": "ffffffffffffffff" } }),
    )
    .expect_err("нет в списке");
    assert_eq!(error["code"], json!("vault_not_found"));
    let error =
        call(&first, "vault_open", json!({ "request": { "id": gone.0 } })).expect_err("нет папки");
    assert_eq!(error["code"], json!("vault_unavailable"));
    assert_eq!(window_count(&app), 1);
    assert!(app.state::<Core>().window_of_vault(&gone).is_none());
}

#[test]
fn remove_from_list_keeps_files_and_open_window() {
    let (app, temp) = app();
    let a = vault(&app, temp.path(), "А");
    let first = window(&app);
    open(&first, &a);

    call(
        &first,
        "vault_remove_from_list",
        json!({ "request": { "id": a.0 } }),
    )
    .expect("убрано");

    let list: VaultListResponse =
        serde_json::from_value(call(&first, "vault_list", json!({})).expect("список"))
            .expect("ответ");
    assert!(list.vaults.is_empty());
    assert!(temp.path().join("А").join("Заметка.md").is_file());
    // Окно хранилища остаётся открытым; открыть по старому идентификатору
    // нельзя — его нет в списке.
    assert_eq!(
        app.state::<Core>().window_of_vault(&a).as_deref(),
        Some(first.label())
    );
    let error =
        call(&first, "vault_open", json!({ "request": { "id": a.0 } })).expect_err("нет в списке");
    assert_eq!(error["code"], json!("vault_not_found"));
}

// ---- Положение окна ---------------------------------------------------------------

const SCREEN: Screen = Screen {
    x: 0.0,
    y: 0.0,
    width: 1920.0,
    height: 1040.0,
};

fn state(x: i32, y: i32, width: u32, height: u32) -> WindowState {
    WindowState {
        width,
        height,
        x,
        y,
        maximized: false,
    }
}

#[test]
fn new_window_without_saved_state_gets_default_size() {
    assert_eq!(placement(None, &[SCREEN]), Placement::default());
    assert_eq!(Placement::default().width, 1200.0);
    assert_eq!(Placement::default().position, None);
}

#[test]
fn saved_window_is_restored_on_a_visible_screen() {
    let p = placement(Some(state(100, 50, 1000, 700)), &[SCREEN]);
    assert_eq!(
        p,
        Placement {
            width: 1000.0,
            height: 700.0,
            position: Some((100.0, 50.0)),
            maximized: false
        }
    );
    // Второй экран слева.
    let left = Screen {
        x: -1280.0,
        y: 0.0,
        width: 1280.0,
        height: 1000.0,
    };
    let p = placement(Some(state(-1200, 10, 800, 600)), &[SCREEN, left]);
    assert_eq!(p.position, Some((-1200.0, 10.0)));
}

#[test]
fn window_from_a_disconnected_screen_is_placed_by_os() {
    let p = placement(Some(state(-1200, 10, 800, 600)), &[SCREEN]);
    assert_eq!(p.position, None);
    assert_eq!((p.width, p.height), (800.0, 600.0));
    let p = placement(Some(state(1900, 1030, 800, 600)), &[SCREEN]);
    assert_eq!(p.position, None);
    // Экранов не знаем — положение не задаём.
    assert_eq!(placement(Some(state(10, 10, 800, 600)), &[]).position, None);
}

#[test]
fn saved_size_is_not_smaller_than_minimum_and_maximized_is_kept() {
    let mut saved = state(10, 10, 10, 10);
    saved.maximized = true;
    let p = placement(Some(saved), &[SCREEN]);
    assert_eq!((p.width, p.height), (640.0, 480.0));
    assert!(p.maximized);
}

/// Окно без размера (свёрнутое Windows сообщает нулевой размер, подменный
/// runtime — тоже) не затирает запомненное. Настоящие размер и положение
/// проверяются вручную на Windows (список проверки в обзоре задачи).
#[test]
fn window_without_size_does_not_overwrite_remembered_placement() {
    let (app, temp) = app();
    let a = vault(&app, temp.path(), "А");
    let first = window(&app);
    open(&first, &a);
    let core = app.state::<Core>();
    let saved = state(10, 20, 900, 700);
    core.registry().save_window(&a, saved).expect("запомнено");

    remember_placement(&first.as_ref().window());

    assert_eq!(core.registry().window(&a), Some(saved));
}
