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

/// CSP как словарь «директива → источники».
fn directives(csp: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut parts = d.split_whitespace().map(str::to_owned);
            let name = parts.next().expect("имя директивы");
            (name, parts.collect())
        })
        .collect()
}

#[test]
fn dev_csp_differs_exactly_by_dev_server_and_inline_styles() {
    // devCsp (только `npm run dev`) = CSP выпуска + сервер разработки
    // `ws://localhost:5173` в connect-src + встроенные стили Vite в style-src.
    let security = &config()["app"]["security"];
    let release = directives(security["csp"].as_str().expect("csp"));
    let mut expected = release.clone();
    expected
        .get_mut("connect-src")
        .expect("connect-src")
        .push("ws://localhost:5173".into());
    expected
        .get_mut("style-src")
        .expect("style-src")
        .push("'unsafe-inline'".into());
    let dev = directives(security["devCsp"].as_str().expect("devCsp"));
    assert_eq!(dev, expected);
}

#[test]
fn csp_leaves_frames_to_default_src_for_isolation_iframe() {
    // Tauri добавляет адрес изолированного iframe (isolation pattern) в
    // default-src; своя frame-src/child-src перекрыла бы его и сломала мост.
    let csp = directives(config()["app"]["security"]["csp"].as_str().expect("csp"));
    assert!(!csp.contains_key("frame-src") && !csp.contains_key("child-src"));
    assert_eq!(csp["default-src"], ["'self'"]);
}

#[test]
fn isolation_pattern_is_enabled() {
    assert_eq!(
        config()["app"]["security"]["pattern"],
        serde_json::json!({ "use": "isolation", "options": { "dir": "../src-isolation" } })
    );
}

/// Раздел TOML-файла от заголовка до следующего заголовка.
fn toml_section(text: &str, header: &str) -> String {
    let start = text
        .find(&format!("\n{header}\n"))
        .unwrap_or_else(|| panic!("нет раздела {header}"));
    let body = &text[start + header.len() + 2..];
    body[..body.find("\n[").unwrap_or(body.len())].to_owned()
}

fn read_text(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Файл из корня рабочего пространства Cargo (`Cargo.toml`, `Cargo.lock`):
/// профили и `[patch]` действуют только там.
fn read_workspace_text(relative: &str) -> String {
    read_text(&format!("../{relative}"))
}

#[test]
fn member_manifest_has_no_profiles_or_patches_that_cargo_would_ignore() {
    // В члене рабочего пространства Cargo молча игнорирует [profile.*] и
    // [patch.*]: правка wry и профиль выпуска перестали бы действовать без ошибки.
    let member = read_text("Cargo.toml");
    for header in ["[profile.", "[patch."] {
        assert!(
            !member
                .lines()
                .any(|line| line.trim_start().starts_with(header)),
            "{header} в src-tauri/Cargo.toml не действует — только в корневом Cargo.toml"
        );
    }
    let root = read_workspace_text("Cargo.toml");
    assert!(
        root.lines().any(|line| line.trim() == "[workspace]"),
        "корневой Cargo.toml — рабочее пространство"
    );
}

#[test]
fn release_profile_keeps_debug_assertions_off_and_does_not_abort() {
    // debug-assertions = true в выпуске включили бы cfg(debug_assertions) и с
    // ним отладочные ветки Tauri; panic = "abort" запускал бы WER с дампом.
    let release = toml_section(&read_workspace_text("Cargo.toml"), "[profile.release]");
    let compact = release.replace(' ', "");
    assert!(!compact.contains("debug-assertions=true"), "{release}");
    assert!(!compact.contains("panic=\"abort\""), "{release}");
}

#[test]
fn deny_bans_devtools_on_every_crate_and_asset_protocol() {
    let deny = read_text("deny.toml").replace(' ', "");
    for (krate, feature) in [
        ("tauri", "devtools"),
        ("tauri-runtime-wry", "devtools"),
        ("tauri-runtime", "devtools"),
        ("wry", "devtools"),
        ("tauri", "protocol-asset"),
    ] {
        let block = deny
            .split("[[bans.features]]")
            .find(|b| b.contains(&format!("\ncrate=\"{krate}\"\n")))
            .unwrap_or_else(|| panic!("нет [[bans.features]] для {krate}"));
        assert!(
            block.contains(&format!("\"{feature}\"")),
            "{krate}: функция {feature} не запрещена"
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
fn only_main_and_manager_capabilities_are_enabled() {
    assert_eq!(
        config()["app"]["security"]["capabilities"],
        serde_json::json!(["main", "manager"])
    );
}

#[test]
fn vault_windows_have_no_file_network_shell_or_dialog_permissions() {
    // Окно хранилища — метка vault-<16 шестнадцатеричных цифр> (задача 1.3).
    no_forbidden_permissions("capabilities/main.json", "vault-*", true);
}

#[test]
fn manager_window_has_no_file_network_shell_dialog_or_event_permissions() {
    // Окно «Управление хранилищами» — метка manager-<16 шестнадцатеричных
    // цифр> (задача 2.8б); подписки на события у него нет.
    no_forbidden_permissions("capabilities/manager.json", "manager-*", false);
}

fn no_forbidden_permissions(file: &str, windows: &str, events: bool) {
    let capability = read_json(file);
    assert_eq!(capability["windows"], serde_json::json!([windows]));
    let allowed_tauri: &[&str] = if events {
        &["core:event:allow-listen", "core:event:allow-unlisten"]
    } else {
        &[]
    };
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
            !id.contains(':') || allowed_tauri.contains(&id),
            "окну выдано лишнее право Tauri {id}"
        );
    }
}

/// Блок `[[package]]` из Cargo.lock по имени крейта.
fn lock_package(name: &str) -> String {
    let lock = read_workspace_text("Cargo.lock");
    let blocks: Vec<&str> = lock
        .split("[[package]]")
        .filter(|block| block.contains(&format!("\nname = \"{name}\"\n")))
        .collect();
    assert_eq!(blocks.len(), 1, "в Cargo.lock ожидалась одна версия {name}");
    blocks[0].to_owned()
}

#[test]
fn wry_never_enables_single_sign_on_with_os_account() {
    // AllowSingleSignOnUsingOSPrimaryAccount: по умолчанию false
    // (webview2-com CoreWebView2EnvironmentOptions::default), wry его не
    // меняет. Своя копия wry не должна его включать (вход через учётную
    // запись Windows — токены WAM).
    let wry = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/wry/src/webview2/mod.rs"),
    )
    .expect("vendor/wry");
    assert!(!wry.contains("set_allow_single_sign_on_using_os_primary_account(true)"));
}

#[test]
fn wry_comes_from_own_patched_copy_of_the_same_version() {
    // Своя копия wry с IsCustomCrashReportingEnabled = true (ADR 0007). Если
    // версия wry в зависимостях Tauri сменится, cargo молча перестанет
    // применять [patch] — этот тест это ловит.
    let wry = lock_package("wry");
    assert!(wry.contains("\nversion = \"0.57.1\"\n"), "{wry}");
    assert!(
        !wry.contains("source ="),
        "wry берётся из crates.io, а не из vendor/wry: {wry}"
    );

    let manifest = read_workspace_text("Cargo.toml");
    assert!(
        manifest.contains("[patch.crates-io]\nwry = { path = \"vendor/wry\" }"),
        "нет [patch.crates-io] для wry в корневом Cargo.toml"
    );

    let vendor = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/wry/src/webview2/mod.rs"),
    )
    .expect("vendor/wry");
    assert_eq!(
        vendor
            .matches("options.set_is_custom_crash_reporting_enabled(true);")
            .count(),
        1,
        "в vendor/wry нет правки IsCustomCrashReportingEnabled"
    );
}
