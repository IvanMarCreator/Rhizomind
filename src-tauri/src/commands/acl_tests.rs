//! Права команд ядра (ADR 0009) на настоящей конфигурации приложения.
//!
//! Приложение собирается подменным runtime Tauri, но с настоящим контекстом
//! (`generate_context!`): в него входят манифест прав из `build.rs`
//! (`AppManifest::commands`) и файл разрешений `capabilities/main.json`.
//! Вызовы идут тем же путём, что из WebView (`Webview::on_message`), где
//! Tauri проверяет права до вызова команды.

use std::path::Path;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

use super::*;
use crate::links::Links;
use crate::switcher::Switcher;
use crate::vault::Vault;

/// Команда, которой нет в `AppManifest::commands`, но которая есть в
/// обработчике: так проверяется, что обработчика мало — нужно право.
#[tauri::command]
async fn contract_probe() -> CoreResult<()> {
    Ok(())
}

macro_rules! handler_with_probe {
    ($($command:ident),* $(,)?) => {
        tauri::generate_handler![$($command,)* contract_probe]
    };
}

fn app() -> App<MockRuntime> {
    mock_builder()
        .manage(Core::default())
        .invoke_handler(with_core_commands!(handler_with_probe))
        .build(tauri::generate_context!(test = true))
        .expect("тестовое приложение")
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("окно")
}

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    let url = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: url.parse().expect("адрес"),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("ответ JSON"))
}

/// Отказ проверки прав Tauri — строка, а не объект ошибки ядра.
fn is_acl_denial(error: &Value) -> bool {
    error
        .as_str()
        .is_some_and(|text| text.contains("not allowed"))
}

/// Тело снимка `contracts/v1/<имя>.snap` (после заголовка insta).
fn contract(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../contracts/v1")
        .join(format!("{name}.snap"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let body = text.splitn(3, "---").nth(2).expect("заголовок insta");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Аргументы вызова из примера запроса в снимке.
fn example_args(name: &str) -> Value {
    match contract(name)["request"].clone() {
        Value::Null => json!({}),
        request => json!({ "request": request }),
    }
}

fn bind_stub_vault(app: &App<MockRuntime>, label: &str) {
    use tauri::Manager;
    let core = app.state::<Core>();
    core.bind_window(
        label,
        OpenVault {
            summary: VaultSummary {
                id: VaultId("test".into()),
                name: "Тест".into(),
                path: "/tmp/Тест".into(),
            },
            vault: Vault::open(Path::new("/tmp/Тест")).expect("заглушка хранилища"),
            links: Links::default().into(),
            switcher: Switcher::default().into(),
        },
    );
}

#[test]
fn command_outside_app_manifest_is_denied_even_with_handler() {
    let app = app();
    let main = window(&app, crate::MAIN_WINDOW_LABEL);

    let error = invoke(&main, "contract_probe", json!({})).expect_err("вызов должен быть отклонён");

    assert!(is_acl_denial(&error), "ожидался отказ прав, получено {error}");
}

#[test]
fn listed_command_is_allowed_for_main_window() {
    let app = app();
    let main = window(&app, crate::MAIN_WINDOW_LABEL);

    let response = invoke(&main, "vault_current", json!({})).expect("vault_current разрешена");

    assert_eq!(response, json!({ "api_version": 1, "vault": null }));
}

#[test]
fn commands_are_denied_for_window_without_capability() {
    let app = app();
    let other = window(&app, "other");

    let error = invoke(&other, "vault_current", json!({})).expect_err("у окна нет прав");

    assert!(is_acl_denial(&error), "ожидался отказ прав, получено {error}");
}

/// Каждая команда из списка разрешена окну и доходит до модулей ядра:
/// ответ — результат или ошибка ядра, а не отказ прав и не «нет команды».
#[test]
fn every_listed_command_reaches_core_modules() {
    let app = app();
    let main = window(&app, crate::MAIN_WINDOW_LABEL);
    bind_stub_vault(&app, crate::MAIN_WINDOW_LABEL);

    for name in COMMAND_NAMES {
        match invoke(&main, name, example_args(name)) {
            Ok(_) => {}
            Err(error) => {
                let core_error: CoreError = serde_json::from_value(error.clone())
                    .unwrap_or_else(|_| panic!("{name}: не ошибка ядра: {error}"));
                assert_eq!(
                    core_error.code,
                    ErrorCode::NotImplemented,
                    "{name}: неожиданная ошибка {error}"
                );
            }
        }
    }
}

/// Запрос с лишним полем отклоняется до вызова модуля.
#[test]
fn request_with_unknown_field_is_rejected() {
    let app = app();
    let main = window(&app, crate::MAIN_WINDOW_LABEL);
    bind_stub_vault(&app, crate::MAIN_WINDOW_LABEL);

    let error = invoke(
        &main,
        "note_read",
        json!({ "request": { "path": ["a.md"], "unexpected": true } }),
    )
    .expect_err("лишнее поле");

    assert!(
        error.as_str().is_some_and(|text| text.contains("unknown field")),
        "ожидался отказ разбора, получено {error}"
    );
}

/// Манифест прав, который `build.rs` передал Tauri (`AppManifest::commands`),
/// перечисляет ровно команды обработчика `generate_handler!`: оба списка
/// строятся из `commands/list.rs`, а тест сверяет результат сборки.
#[test]
fn app_manifest_commands_match_handler() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("gen/schemas/acl-manifests.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let manifests: Value = serde_json::from_str(&text).expect("acl-manifests.json");
    let mut in_manifest: Vec<String> = manifests["__app-acl__"]["permissions"]
        .as_object()
        .expect("права приложения")
        .keys()
        .filter_map(|id| id.strip_prefix("allow-"))
        .map(|command| command.replace('-', "_"))
        .collect();
    in_manifest.sort();

    let mut in_handler: Vec<String> = COMMAND_NAMES.iter().map(|c| (*c).to_owned()).collect();
    in_handler.sort();
    assert_eq!(in_manifest, in_handler);

    // И каждая команда списка действительно есть в обработчике: вызов
    // несуществующей команды дал бы «not found».
    let app = app();
    let main = window(&app, crate::MAIN_WINDOW_LABEL);
    for name in COMMAND_NAMES {
        if let Err(error) = invoke(&main, name, example_args(name)) {
            assert!(
                !error.as_str().is_some_and(|text| text.contains("not found")),
                "{name} нет в обработчике: {error}"
            );
        }
    }
}

/// Окну выданы права ровно на команды из списка и на подписку на события.
#[test]
fn main_capability_allows_exactly_listed_commands_and_event_listening() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities/main.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let capability: Value = serde_json::from_str(&text).expect("main.json");
    let mut granted: Vec<String> = capability["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().expect("право строкой").to_owned())
        .collect();
    granted.sort();

    let mut expected: Vec<String> = COMMAND_NAMES
        .iter()
        .map(|c| format!("allow-{}", c.replace('_', "-")))
        .chain(["core:event:allow-listen".into(), "core:event:allow-unlisten".into()])
        .collect();
    expected.sort();
    assert_eq!(granted, expected);
}
