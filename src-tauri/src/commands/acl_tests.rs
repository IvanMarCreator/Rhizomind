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
        .plugin(tauri_plugin_dialog::init())
        .manage(Core::default())
        .invoke_handler(with_core_commands!(handler_with_probe))
        .build(tauri::generate_context!(test = true))
        .expect("тестовое приложение")
}

/// Окно хранилища: метка по шаблону `vault-*` из `capabilities/main.json`.
const MAIN: &str = "vault-0123456789abcdef";

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("окно")
}

/// Адрес своего интерфейса (как у WebView на этой ОС).
const LOCAL_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

fn invoke_from(
    window: &WebviewWindow<MockRuntime>,
    url: &str,
    command: &str,
    args: Value,
) -> Result<Value, Value> {
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

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    invoke_from(window, LOCAL_URL, command, args)
}

/// Точный текст отказа проверки прав Tauri 2.12 (отладочная сборка, в
/// которой идут тесты): `Webview::on_message` →
/// `RuntimeAuthority::resolve_access_message`.
fn denial(command: &str, reason: &str) -> Value {
    Value::String(format!("{command} not allowed. {reason}"))
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

/// Привязывает к окну заглушку хранилища во временной папке; папка живёт,
/// пока жив возвращённый `TempDir`.
fn bind_stub_vault(app: &App<MockRuntime>, label: &str) -> tempfile::TempDir {
    use tauri::Manager;
    let dir = tempfile::tempdir().expect("временная папка");
    let core = app.state::<Core>();
    core.bind_window(
        label,
        OpenVault::new(
            VaultSummary {
                id: VaultId("test".into()),
                name: "Тест".into(),
                path: dir.path().display().to_string(),
            },
            Vault::open(dir.path()).expect("заглушка хранилища"),
        )
        .expect("список заметок"),
    );
    dir
}

fn core_error(name: &str, error: Value) -> CoreError {
    serde_json::from_value(error.clone())
        .unwrap_or_else(|_| panic!("{name}: не ошибка ядра, а отказ моста: {error}"))
}

#[test]
fn command_outside_app_manifest_is_denied_even_with_handler() {
    let app = app();
    let main = window(&app, MAIN);

    let error = invoke(&main, "contract_probe", json!({})).expect_err("вызов должен быть отклонён");

    assert_eq!(error, denial("contract_probe", "Command not found"));
}

#[test]
fn listed_command_is_allowed_for_main_window() {
    let app = app();
    let main = window(&app, MAIN);

    let response = invoke(&main, "vault_current", json!({})).expect("vault_current разрешена");

    assert_eq!(response, json!({ "api_version": 1, "vault": null }));
}

#[test]
fn commands_are_denied_for_window_without_capability() {
    let app = app();
    let other = window(&app, "other");

    let error = invoke(&other, "vault_current", json!({})).expect_err("у окна нет прав");

    assert_eq!(
        error,
        json!(
            "vault_current not allowed on window \"other\", webview \"other\", URL: local\n\n\
             allowed on: [windows: \"vault-*\", URL: local]\n\n\
             referenced by: capability: main, permission: allow-vault-current"
        )
    );
}

/// Страница со стороннего адреса (если бы окно ушло со своего интерфейса)
/// не может вызвать команды ядра.
#[test]
fn commands_are_denied_from_remote_origin() {
    let app = app();
    let main = window(&app, MAIN);

    let error = invoke_from(&main, "https://evil.example", "vault_current", json!({}))
        .expect_err("сторонний адрес");

    assert_eq!(
        error,
        json!(
            "vault_current not allowed on window \"vault-0123456789abcdef\", webview \"vault-0123456789abcdef\", URL: https://evil.example/\n\n\
             allowed on: [windows: \"vault-*\", URL: local]\n\n\
             referenced by: capability: main, permission: allow-vault-current"
        )
    );
}

/// Команды Tauri и плагинов, которых у окна нет: отправка событий, пути ОС,
/// окна, версия приложения, файлы, открытие ссылок, диалоги.
#[test]
fn tauri_and_plugin_commands_are_denied() {
    let app = app();
    let main = window(&app, MAIN);
    let cases = [
        (
            "plugin:event|emit",
            "event.emit",
            "Permissions associated with this command: core:event:allow-emit, core:event:default",
        ),
        (
            "plugin:event|emit_to",
            "event.emit_to",
            "Permissions associated with this command: core:event:allow-emit-to, core:event:default",
        ),
        (
            "plugin:path|resolve_directory",
            "path.resolve_directory",
            "Permissions associated with this command: core:path:allow-resolve-directory, core:path:default",
        ),
        (
            "plugin:window|close",
            "window.close",
            "Permissions associated with this command: core:window:allow-close",
        ),
        (
            "plugin:webview|create_webview_window",
            "webview.create_webview_window",
            "Permissions associated with this command: core:webview:allow-create-webview-window",
        ),
        (
            "plugin:app|version",
            "app.version",
            "Permissions associated with this command: core:app:allow-version, core:app:default",
        ),
        ("plugin:fs|read_file", "fs.read_file", "Plugin not found"),
        (
            "plugin:opener|open_url",
            "opener.open_url",
            "Plugin not found",
        ),
        (
            "plugin:dialog|open",
            "dialog.open",
            "Permissions associated with this command: dialog:allow-open, dialog:default",
        ),
    ];
    for (command, shown_as, reason) in cases {
        let error = invoke(&main, command, json!({})).expect_err(command);
        assert_eq!(error, denial(shown_as, reason), "{command}");
    }
}

/// Подписка на события разрешена: отказ — из-за аргументов, не из-за прав.
#[test]
fn event_listen_passes_acl() {
    let app = app();
    let main = window(&app, MAIN);

    let error = invoke(&main, "plugin:event|listen", json!({})).expect_err("нет аргументов");

    assert_eq!(
        error,
        json!(
            "invalid args `event` for command `listen`: command listen missing required key event"
        )
    );
}

/// Каждая команда из списка разрешена окну и доходит до модулей ядра:
/// ответ — результат или любая ошибка ядра, но не отказ прав и не ошибка
/// разбора параметров (те приходят строкой, а не объектом ошибки ядра).
#[test]
fn every_listed_command_reaches_core_modules() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    for name in COMMAND_NAMES {
        if let Err(error) = invoke(&main, name, example_args(name)) {
            core_error(name, error);
        }
    }
}

/// Пределы проверяются в слое команд, до модулей: даже без открытого
/// хранилища ответ — `invalid_argument` с именем поля.
#[test]
fn limits_are_checked_before_modules() {
    let app = app();
    let main = window(&app, MAIN);
    let long = "я".repeat(256);
    let deep: Vec<&str> = std::iter::repeat_n("a", 65).collect();
    let cases = [
        ("note_read", json!({ "path": [long] }), "path"),
        ("note_create", json!({ "dir": deep, "name": "a.md" }), "dir"),
        ("folder_create", json!({ "dir": [], "name": long }), "name"),
        (
            "entry_move",
            json!({ "path": ["a.md"], "new_dir": deep }),
            "new_dir",
        ),
        ("entry_trash", json!({ "path": deep }), "path"),
        (
            "name_check",
            json!({ "dir": [], "name": "a.md", "renaming": deep }),
            "renaming",
        ),
        (
            "links_resolve",
            json!({ "from": ["a.md"], "targets": vec!["x"; 10_001] }),
            "targets",
        ),
        (
            "links_suggest",
            json!({ "from": ["a.md"], "query": "ж".repeat(257) }),
            "query",
        ),
        (
            "switcher_search",
            json!({ "query": "ж".repeat(257) }),
            "query",
        ),
        (
            "note_write",
            json!({
                "path": ["a.md"],
                "text": "a".repeat(100 * 1024 * 1024 + 1),
                "expected_version": { "hash": "0", "size": 0, "modified_ms": 0 }
            }),
            "text",
        ),
        (
            "workspace_write",
            json!({ "workspace": { "big": "a".repeat(1024 * 1024) } }),
            "workspace",
        ),
        (
            "open_external",
            json!({ "url": "https://a.example/\nhttps://b.example" }),
            "url",
        ),
        (
            "open_external",
            json!({ "url": format!("https://a.example/{}", "a".repeat(8 * 1024)) }),
            "url",
        ),
    ];
    for (command, request, field) in cases {
        let error = invoke(&main, command, json!({ "request": request })).expect_err(command);
        let error = core_error(command, error);
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{command}");
        assert_eq!(error.field.as_deref(), Some(field), "{command}");
    }
}

#[test]
fn settings_out_of_range_are_rejected() {
    let app = app();
    let main = window(&app, MAIN);
    let mut settings = contract("settings_write")["request"]["settings"].clone();
    settings["font_size"] = json!(200);

    let error = invoke(
        &main,
        "settings_write",
        json!({ "request": { "settings": settings } }),
    )
    .expect_err("размер шрифта вне диапазона");

    assert_eq!(
        core_error("settings_write", error).field.as_deref(),
        Some("settings.font_size")
    );
}

/// Плохое имя хранилища — `invalid_name` с причиной, до диалога выбора папки.
#[test]
fn vault_create_rejects_bad_name_with_reason() {
    let app = app();
    let main = window(&app, MAIN);

    let error = invoke(
        &main,
        "vault_create",
        json!({ "request": { "name": "CON" } }),
    )
    .expect_err("зарезервированное имя");

    assert_eq!(
        core_error("vault_create", error),
        CoreError::invalid_name(NameProblem::ReservedName)
    );
}

/// Заметка не меняет расширение при переименовании, вложение не становится
/// заметкой — до обращения к хранилищу.
#[test]
fn rename_cannot_change_note_extension() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    for (from, to) in [("План.md", "План.txt"), ("схема.png", "схема.md")] {
        let error = invoke(
            &main,
            "entry_rename",
            json!({ "request": { "path": [from], "new_name": to } }),
        )
        .expect_err(to);
        assert_eq!(
            core_error("entry_rename", error),
            CoreError::invalid_name(NameProblem::NoteExtension),
            "{from} → {to}"
        );
    }
    // Смена регистра расширения — не смена вида: доходит до модуля
    // (заметки в пустом хранилище нет — `not_found` от `vault`).
    let error = invoke(
        &main,
        "entry_rename",
        json!({ "request": { "path": ["План.md"], "new_name": "План.MD" } }),
    )
    .expect_err("заметки нет");
    assert_eq!(core_error("entry_rename", error).code, ErrorCode::NotFound);
}

/// Правила имён в командах создания и переименования — в слое команд, до
/// хранилища (повторное ревью 0.4.1, п. 2): с открытой заглушкой хранилища
/// ответ — `invalid_name` с причиной, а не `not_implemented` модуля `vault`.
#[test]
fn create_and_rename_check_name_rules_before_modules() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    for (name, problem) in [
        ("CON.md", NameProblem::ReservedName),
        ("a:b.md", NameProblem::ForbiddenChars),
    ] {
        let cases = [
            ("note_create", json!({ "dir": [], "name": name })),
            ("folder_create", json!({ "dir": ["Проекты"], "name": name })),
            (
                "entry_rename",
                json!({ "path": ["План.md"], "new_name": name }),
            ),
        ];
        for (command, request) in cases {
            let error = invoke(&main, command, json!({ "request": request })).expect_err(command);
            assert_eq!(
                core_error(command, error),
                CoreError::invalid_name(problem),
                "{command} {name}"
            );
        }
    }
}

/// Проверка имени: правила имён и смена вида — без хранилища; совпадение с
/// соседями — в модуле `vault`.
#[test]
fn name_check_applies_name_rules_and_renaming() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);
    let check = |request: Value| invoke(&main, "name_check", json!({ "request": request }));

    assert_eq!(
        check(json!({ "dir": [], "name": "Итоги..md" })).expect("ответ"),
        json!({ "status": "rejected", "problem": "trailing_dot_or_space", "message_key": "names.trailing_dot_or_space" })
    );
    assert_eq!(
        check(json!({ "dir": [], "name": "План.txt", "renaming": ["План.md"] })).expect("ответ"),
        json!({ "status": "rejected", "problem": "note_extension", "message_key": "names.note_extension" })
    );
    // Совпадение с соседями — в модуле `vault`: в пустом хранилище имя свободно.
    assert_eq!(
        check(json!({ "dir": [], "name": "план.md", "renaming": ["План.md"] })).expect("ответ"),
        json!({ "status": "ok", "warning": null })
    );
}

/// Плохая цель ссылки — `path: null`, а не ошибка всего ответа.
#[test]
fn links_resolve_marks_bad_targets_unresolved() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    let response = invoke(
        &main,
        "links_resolve",
        json!({ "request": { "from": ["a.md"], "targets": ["", "a\u{1}b"] } }),
    )
    .expect("ответ без ошибки");

    assert_eq!(
        response,
        json!({ "results": [{ "target": "", "path": null }, { "target": "a\u{1}b", "path": null }] })
    );
}

/// `vault_current` не отдаёт путь к папке хранилища.
#[test]
fn vault_current_has_no_path() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    let response = invoke(&main, "vault_current", json!({})).expect("ответ");

    assert_eq!(
        response,
        json!({ "api_version": 1, "vault": { "id": "test", "name": "Тест" } })
    );
}

/// Запрос с лишним полем отклоняется до вызова модуля.
#[test]
fn request_with_unknown_field_is_rejected() {
    let app = app();
    let main = window(&app, MAIN);
    let _vault_dir = bind_stub_vault(&app, MAIN);

    let error = invoke(
        &main,
        "note_read",
        json!({ "request": { "path": ["a.md"], "unexpected": true } }),
    )
    .expect_err("лишнее поле");

    assert!(
        error
            .as_str()
            .is_some_and(|text| text.contains("unknown field")),
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
    let main = window(&app, MAIN);
    for name in COMMAND_NAMES {
        if let Err(error) = invoke(&main, name, example_args(name)) {
            assert!(
                !error
                    .as_str()
                    .is_some_and(|text| text.contains("not found")),
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
        .chain([
            "core:event:allow-listen".into(),
            "core:event:allow-unlisten".into(),
        ])
        .collect();
    expected.sort();
    assert_eq!(granted, expected);
}
