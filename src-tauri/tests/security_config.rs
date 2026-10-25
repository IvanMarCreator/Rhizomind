//! Меры безопасности, заданные конфигурацией (ADR 0007, 0009): тест читает
//! `tauri.conf.json` и файл разрешений и не даёт ослабить их незаметно.

use serde_json::Value;

fn read_json(relative: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn config() -> Value {
    read_json("tauri.conf.json")
}

const EXPECTED_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
connect-src ipc: http://ipc.localhost; img-src 'self' asset: http://asset.localhost blob: data:; \
object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[test]
fn csp_has_no_external_hosts_and_matches_adr_0009() {
    assert_eq!(config()["app"]["security"]["csp"], EXPECTED_CSP);
}

#[test]
fn dev_csp_differs_only_by_dev_server_on_localhost() {
    let dev = config()["app"]["security"]["devCsp"]
        .as_str()
        .expect("devCsp")
        .to_owned();
    for source in dev
        .split([' ', ';'])
        .filter(|s| s.contains("://") || s.ends_with(':'))
    {
        let allowed = [
            "ipc:",
            "http://ipc.localhost",
            "asset:",
            "http://asset.localhost",
            "blob:",
            "data:",
        ];
        assert!(
            allowed.contains(&source) || source == "ws://localhost:5173",
            "в devCsp внешний источник: {source}"
        );
    }
}

#[test]
fn prototype_is_frozen_and_no_global_tauri_object() {
    let app = &config()["app"];
    assert_eq!(app["security"]["freezePrototype"], true);
    assert_eq!(app["withGlobalTauri"], false);
}

#[test]
fn no_windows_in_config_main_window_is_created_in_rust() {
    assert_eq!(config()["app"]["windows"], serde_json::json!([]));
}

#[test]
fn installer_does_not_download_webview2_and_nsis_is_per_user() {
    let windows = &config()["bundle"]["windows"];
    assert_eq!(windows["webviewInstallMode"]["type"], "skip");
    assert_eq!(windows["nsis"]["installMode"], "currentUser");
}

#[test]
fn only_main_capability_is_enabled() {
    assert_eq!(
        config()["app"]["security"]["capabilities"],
        serde_json::json!(["main"])
    );
}

#[test]
fn main_window_has_no_file_network_shell_or_dialog_permissions() {
    let capability = read_json("capabilities/main.json");
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
    let permissions = capability["permissions"].as_array().expect("permissions");
    let forbidden = [
        "fs:",
        "opener:",
        "shell:",
        "dialog:",
        "http:",
        "updater:",
        "websocket:",
        "upload:",
    ];
    for permission in permissions {
        let id = permission
            .as_str()
            .or_else(|| permission["identifier"].as_str())
            .unwrap_or_default();
        assert!(
            !forbidden.iter().any(|prefix| id.starts_with(prefix)),
            "окну выдано запрещённое право {id}"
        );
    }
    // Из прав ядра Tauri (`core:*`) — только подписка на события. Точный
    // состав (свои команды по списку) проверяет src/commands/acl_tests.rs.
    for permission in permissions {
        let id = permission.as_str().unwrap_or_default();
        assert!(
            !id.contains(':')
                || ["core:event:allow-listen", "core:event:allow-unlisten"].contains(&id),
            "окну выдано лишнее право Tauri {id}"
        );
    }
}
