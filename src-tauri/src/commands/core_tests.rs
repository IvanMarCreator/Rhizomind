//! Команды на **настоящем ядре** (задача 1.7): настоящее хранилище во
//! временной папке, наблюдатель за папкой, события — через тот же мост, что
//! из WebView (`Webview::on_message`, проверка прав Tauri до вызова).
//!
//! Что проверяется:
//! - примеры запросов из снимков `contracts/v1/` проходят на настоящих
//!   модулях, ответы разбираются строгими типами контракта;
//! - неверные типы, лишние поля и путь за контуром отклоняются до модулей;
//! - окно не может обратиться к хранилищу другого окна, события уходят
//!   только окну своего хранилища;
//! - окно подтверждения безвозвратного удаления и общий замок диалогов ядра.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, CapabilityBuilder, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewWindow, WebviewWindowBuilder};

use super::*;
use crate::dialogs::test_answers::{self, Answers};
use crate::events::WindowEmitter;
use crate::vault::TrashFailure;

const LOCAL_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

/// Сколько ждать события наблюдателя (дерево — «в течение секунды», запас
/// на CI).
const WAIT: Duration = Duration::from_secs(10);

fn app() -> App<MockRuntime> {
    // Оболочка 0.3 — для новых окон хранилищ (`crate::create_window`).
    let data_dir = std::env::temp_dir().join(format!("rhz-core-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir).expect("папка");
    let data_dir = crate::webview_traces::WebviewDataDir::prepare_in(&data_dir, std::process::id())
        .expect("папка данных");
    mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .manage(crate::Shell::new(data_dir, crate::security::browser_args()))
        .manage(Core::default())
        .invoke_handler(handler())
        .build(tauri::generate_context!(test = true))
        .expect("тестовое приложение")
}

/// Окно с правами на все команды ядра и подписку на события — как у
/// главного окна, но со своей меткой (тесты идут параллельно, а ответы
/// диалогов задаются по метке окна).
fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    if !label.starts_with(crate::windows::LABEL_PREFIX) {
        let capability = COMMAND_NAMES.iter().fold(
            CapabilityBuilder::new(format!("test-{label}")).window(label),
            |capability, name| capability.permission(format!("allow-{}", name.replace('_', "-"))),
        );
        app.add_capability(
            capability
                .permission("core:event:allow-listen")
                .permission("core:event:allow-unlisten"),
        )
        .expect("права окна");
    }
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("окно")
}

/// Окно «Управление хранилищами» (метка `manager-*`, запись в ядре).
fn manager_window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    assert_eq!(
        app.state::<Core>().reserve_manager(label),
        crate::commands::ManagerSlot::New(label.into())
    );
    window(app, label)
}

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
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

fn call(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    request: Value,
) -> Result<Value, Value> {
    invoke(window, command, json!({ "request": request }))
}

fn core_error(command: &str, error: Value) -> CoreError {
    serde_json::from_value(error.clone())
        .unwrap_or_else(|_| panic!("{command}: не ошибка ядра, а отказ моста: {error}"))
}

/// Ответ строгим типом контракта (`deny_unknown_fields`).
fn typed<T: DeserializeOwned>(command: &str, value: Value) -> T {
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("{command}: ответ не по контракту ({e}): {value}"))
}

/// Тело снимка `contracts/v1/<имя>.snap`.
fn contract(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../contracts/v1")
        .join(format!("{name}.snap"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let body = text.splitn(3, "---").nth(2).expect("заголовок insta");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn example(name: &str) -> Value {
    contract(name)["request"].clone()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let path = rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
    std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
    std::fs::write(path, bytes).expect("файл");
}

/// Подменная корзина: переносит в папку рядом с хранилищем или сообщает,
/// что корзины нет.
fn trash_into(bin: PathBuf, has_trash: bool) -> crate::vault::TrashBackend {
    Box::new(move |path: &Path| {
        if !has_trash {
            return Err(TrashFailure::NoTrash);
        }
        std::fs::create_dir_all(&bin).expect("корзина");
        std::fs::rename(path, bin.join(path.file_name().expect("имя"))).expect("в корзину");
        Ok(())
    })
}

/// Открывает хранилище `root` в окне `label` той же функцией, что команды
/// группы «Хранилища» (`Core::open_vault`), с подменной корзиной.
fn open_vault(
    app: &App<MockRuntime>,
    label: &str,
    id: &str,
    root: &Path,
    trash: crate::vault::TrashBackend,
) -> Arc<OpenVault> {
    let core = app.state::<Core>();
    core.open_vault_with(
        label,
        VaultSummary {
            id: VaultId(id.into()),
            name: id.into(),
            path: root.display().to_string(),
        },
        root,
        Arc::new(WindowEmitter::new(app.handle().clone(), label)),
        Some(trash),
    )
    .expect("хранилище открыто")
}

/// События окна, полученные слушателем на стороне Rust.
fn record_events(
    window: &WebviewWindow<MockRuntime>,
    names: &[&str],
) -> Arc<Mutex<Vec<(String, Value)>>> {
    let seen: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    for name in names {
        let log = Arc::clone(&seen);
        let event_name = (*name).to_owned();
        window.listen(*name, move |event| {
            let payload = serde_json::from_str(event.payload()).unwrap_or(Value::Null);
            log.lock()
                .expect("журнал")
                .push((event_name.clone(), payload));
        });
    }
    seen
}

fn wait_for(
    seen: &Mutex<Vec<(String, Value)>>,
    check: impl Fn(&[(String, Value)]) -> bool,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < WAIT {
        if check(&seen.lock().expect("журнал")) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

// ---- Примеры контракта на настоящем ядре ----------------------------------

/// Хранилище для примеров из снимков.
fn contract_vault(root: &Path) {
    put(
        root,
        "Проекты/Встреча.md",
        "# Встреча\nСм. [[Альфа/План]]\n".as_bytes(),
    );
    put(root, "Проекты/схема.png", b"png");
    put(root, "Проекты/Альфа/План.md", "# План\n".as_bytes());
    put(root, "Проекты/Альфа/Встреча.md", b"");
    put(root, "Встреча.md", b"");
    put(root, "#Встреча 2.md", b"");
    put(root, "Дневник.md", b"");
}

#[test]
fn contract_examples_pass_on_real_core() {
    let app = app();
    let w = window(&app, "contract");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("Работа");
    contract_vault(&root);
    open_vault(
        &app,
        "contract",
        "7f3c9a1e5b2d4c60",
        &root,
        trash_into(dir.path().join("bin"), true),
    );

    // Хранилище окна.
    assert_eq!(
        invoke(&w, "vault_current", json!({})).expect("vault_current"),
        json!({ "api_version": 1, "vault": { "id": "7f3c9a1e5b2d4c60", "name": "7f3c9a1e5b2d4c60" } })
    );

    // Дерево: все элементы из примера есть в ответе.
    let tree: TreeListResponse = typed(
        "tree_list",
        invoke(&w, "tree_list", json!({})).expect("tree_list"),
    );
    let expected: TreeListResponse = typed("tree_list", contract("tree_list")["response"].clone());
    for entry in &expected.entries {
        assert!(
            tree.entries.contains(entry),
            "нет {entry:?} в {:?}",
            tree.entries
        );
    }

    // Операции с деревом — точно как в примерах, по порядку.
    for name in [
        "folder_create",
        "note_create",
        "entry_rename",
        "entry_move",
        "entry_trash",
    ] {
        let response = call(&w, name, example(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(response, contract(name)["response"], "{name}");
    }
    assert!(
        dir.path().join("bin").join("Встреча 2.md").is_file(),
        "в корзине"
    );

    // Проверка имени: переименование в другом регистре не совпадает с собой.
    let check: NameCheck = typed(
        "name_check",
        call(&w, "name_check", example("name_check")).expect("name_check"),
    );
    assert!(matches!(check, NameCheck::Ok { .. }), "{check:?}");

    // Чтение и запись с конфликтом изменений: версия в примере не совпадает
    // с диском — копия версии с диска рядом.
    let read: NoteReadResponse = typed(
        "note_read",
        call(&w, "note_read", example("note_read")).expect("note_read"),
    );
    assert_eq!(read.text, "# Встреча\nСм. [[Альфа/План]]\n");
    assert_eq!(read.read_only, None);
    let written: NoteWriteResponse = typed(
        "note_write",
        call(&w, "note_write", example("note_write")).expect("note_write"),
    );
    let NoteWriteResponse::ConflictSaved { copy_path, .. } = written else {
        panic!("ожидался конфликт: {written:?}");
    };
    assert_eq!(copy_path.components()[0], "Проекты");
    assert!(copy_path.components()[1].starts_with("Встреча (внешняя версия "));
    assert_eq!(
        std::fs::read_to_string(root.join("Проекты").join("Встреча.md")).expect("заметка"),
        "# Встреча\nИтоги\n"
    );

    // Ссылки и поиск.
    assert_eq!(
        call(&w, "links_resolve", example("links_resolve")).expect("links_resolve"),
        contract("links_resolve")["response"]
    );
    let suggest: LinksSuggestResponse = typed(
        "links_suggest",
        call(&w, "links_suggest", example("links_suggest")).expect("links_suggest"),
    );
    let expected: LinksSuggestResponse = typed(
        "links_suggest",
        contract("links_suggest")["response"].clone(),
    );
    for item in &expected.items {
        let found = suggest
            .items
            .iter()
            .find(|got| got.path == item.path)
            .unwrap_or_else(|| panic!("нет подсказки {:?}: {:?}", item.path, suggest.items));
        assert_eq!(found.linkable, item.linkable, "{:?}", item.path);
        assert_eq!(found.name, item.name);
    }
    let found: SwitcherSearchResponse = typed(
        "switcher_search",
        call(&w, "switcher_search", example("switcher_search")).expect("поиск"),
    );
    let expected: SwitcherSearchResponse = typed(
        "switcher_search",
        contract("switcher_search")["response"].clone(),
    );
    assert_eq!(found.items.first(), expected.items.first());

    // Настройки и раскладка: записанное читается обратно.
    call(&w, "settings_write", example("settings_write")).expect("settings_write");
    assert_eq!(
        invoke(&w, "settings_read", json!({})).expect("settings_read"),
        example("settings_write")
    );
    assert_eq!(
        invoke(&w, "workspace_read", json!({})).expect("workspace_read"),
        json!({ "workspace": null })
    );
    call(&w, "workspace_write", example("workspace_write")).expect("workspace_write");
    assert_eq!(
        invoke(&w, "workspace_read", json!({})).expect("workspace_read"),
        example("workspace_write")
    );

    // Внешняя ссылка: в тестах окно подтверждения отвечает «отмена».
    // Показанная заметка (задача 2.12): заголовок ставит ядро, ответ пустой.
    assert_eq!(
        call(&w, "window_shown_note", example("window_shown_note")).expect("window_shown_note"),
        Value::Null
    );
    assert_eq!(
        call(&w, "open_external", example("open_external")).expect("open_external"),
        json!({ "outcome": "cancelled" })
    );

    // Хранилища. Окну хранилища команды окна управления ядро не выполняет
    // (второй рубеж прав, задача 2.8б), даже если права выданы.
    for name in crate::commands::MANAGER_ONLY_COMMANDS {
        let args = match example(name) {
            Value::Null => json!({}),
            request => json!({ "request": request }),
        };
        let error = core_error(name, invoke(&w, name, args).expect_err(name));
        assert_eq!(error.field.as_deref(), Some("window"), "{name}");
    }
    // Окно управления: выбор папки отменён — `cancelled`; `vault_create`
    // без выбранной в окне ядра папки — отказ; список хранилищ
    // (`vaults.json`, задача 1.3) — в памяти: хранилища из примеров в нём нет.
    let m = manager_window(&app, "manager-c0ffee0000000001");
    assert_eq!(
        invoke(&m, "vault_open_folder", json!({})).expect("vault_open_folder"),
        json!({ "outcome": "cancelled" })
    );
    assert_eq!(
        invoke(&m, "folder_pick", json!({})).expect("folder_pick"),
        json!({ "outcome": "cancelled" })
    );
    let error = core_error(
        "vault_create",
        call(&m, "vault_create", example("vault_create")).expect_err("без выбора папки"),
    );
    assert_eq!(error.field.as_deref(), Some("parent"));
    let list: VaultListResponse = typed(
        "vault_list",
        invoke(&w, "vault_list", json!({})).expect("vault_list"),
    );
    assert!(list.vaults.is_empty());
    let w = m;
    for (name, args) in [
        ("vault_open", json!({ "request": example("vault_open") })),
        (
            "vault_remove_from_list",
            json!({ "request": example("vault_remove_from_list") }),
        ),
    ] {
        let error = invoke(&w, name, args).expect_err(name);
        assert_eq!(
            core_error(name, error).code,
            ErrorCode::VaultNotFound,
            "{name}"
        );
    }
}

// ---- Отказы до модулей ----------------------------------------------------

#[test]
fn wrong_types_extra_fields_and_paths_outside_the_vault_are_rejected() {
    let app = app();
    let w = window(&app, "rejects");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Своя.md", "своя".as_bytes());
    put(dir.path(), "secret.md", b"secret");
    open_vault(
        &app,
        "rejects",
        "rejects",
        &root,
        trash_into(dir.path().join("bin"), true),
    );

    // Неверные типы и лишние поля — отказ разбора (строка Tauri, не ошибка
    // ядра), команда не вызывается.
    let bad_requests = [
        ("note_read", json!({ "path": "Своя.md" }), "invalid type"),
        ("note_read", json!({ "path": [1] }), "invalid type"),
        (
            "note_read",
            json!({ "path": ["Своя.md"], "extra": 1 }),
            "unknown field `extra`",
        ),
        (
            "note_write",
            json!({ "path": ["Своя.md"], "text": 5, "expected_version": null }),
            "invalid type",
        ),
        (
            "note_write",
            json!({ "path": ["Своя.md"], "text": "", "expected_version": { "hash": "0", "size": -1, "modified_ms": 0 } }),
            "invalid value",
        ),
        (
            "note_create",
            json!({ "dir": [], "name": ["a.md"] }),
            "invalid type",
        ),
        (
            "entry_rename",
            json!({ "path": ["Своя.md"], "new_name": "b.md", "force": true }),
            "unknown field `force`",
        ),
        (
            "links_resolve",
            json!({ "from": ["Своя.md"], "targets": "x" }),
            "invalid type",
        ),
        ("switcher_search", json!({ "query": null }), "invalid type"),
        (
            "settings_write",
            json!({ "settings": { "theme": "dark" } }),
            "missing field",
        ),
        (
            "name_check",
            json!({ "dir": [], "name": "a.md", "renaming": "a.md" }),
            "invalid type",
        ),
        (
            "open_external",
            json!({ "url": "https://example.org", "confirmed": true }),
            "unknown field `confirmed`",
        ),
    ];
    for (command, request, expected) in bad_requests {
        let error = call(&w, command, request.clone()).expect_err(command);
        let text = error.as_str().unwrap_or_else(|| {
            panic!("{command} {request}: ожидался отказ разбора, получено {error}")
        });
        assert!(
            text.contains(expected) && text.contains("invalid args `request`"),
            "{command} {request}: ожидалось «{expected}», получено {text}"
        );
    }
    let error = invoke(&w, "note_read", json!({})).expect_err("нет request");
    assert!(
        error
            .as_str()
            .is_some_and(|text| text.contains("missing required key request")),
        "{error}"
    );

    // Путь за контуром — `invalid_path` из слоя команд, снаружи ничего не
    // тронуто.
    let escapes = [
        ("note_read", json!({ "path": ["..", "secret.md"] })),
        (
            "note_write",
            json!({ "path": ["..", "secret.md"], "text": "x", "expected_version": { "hash": "0", "size": 0, "modified_ms": 0 } }),
        ),
        ("note_create", json!({ "dir": [".."], "name": "x.md" })),
        (
            "folder_create",
            json!({ "dir": ["a", "..", ".."], "name": "x" }),
        ),
        (
            "entry_rename",
            json!({ "path": ["C:"], "new_name": "x.md" }),
        ),
        (
            "entry_move",
            json!({ "path": ["Своя.md"], "new_dir": [".."] }),
        ),
        ("entry_trash", json!({ "path": ["..", "secret.md"] })),
        ("name_check", json!({ "dir": ["/etc"], "name": "x.md" })),
        (
            "links_resolve",
            json!({ "from": ["..", "secret.md"], "targets": [] }),
        ),
        (
            "links_suggest",
            json!({ "from": ["\\\\?\\C:"], "query": "" }),
        ),
        (
            "note_read",
            json!({ "path": [".rhizomind", "settings.json"] }),
        ),
    ];
    for (command, request) in escapes {
        let error = call(&w, command, request.clone()).expect_err(command);
        let code = core_error(command, error).code;
        assert!(
            matches!(code, ErrorCode::InvalidPath | ErrorCode::OutsideVault),
            "{command} {request}: {code:?}"
        );
    }
    assert_eq!(
        std::fs::read(dir.path().join("secret.md")).expect("секрет"),
        b"secret"
    );

    // Имя служебной папки — одна причина в проверке при вводе и в создании.
    assert_eq!(
        call(&w, "name_check", json!({ "dir": [], "name": ".Rhizomind" })).expect("ответ"),
        json!({ "status": "rejected", "problem": "reserved_name", "message_key": "names.reserved_name" })
    );
    assert_eq!(
        core_error(
            "folder_create",
            call(
                &w,
                "folder_create",
                json!({ "dir": [], "name": ".Rhizomind" })
            )
            .expect_err("служебное")
        ),
        CoreError::invalid_name(NameProblem::ReservedName)
    );
}

// ---- Окна и хранилища -----------------------------------------------------

#[test]
fn window_cannot_reach_the_vault_of_another_window() {
    let app = app();
    let a = window(&app, "vault-a");
    let b = window(&app, "vault-b");
    let none = window(&app, "no-vault");
    let dir = tempfile::tempdir().expect("папка");
    let (root_a, root_b) = (dir.path().join("А"), dir.path().join("Б"));
    put(&root_a, "Только в А.md", b"a");
    put(&root_b, "Только в Б.md", b"b");
    open_vault(
        &app,
        "vault-a",
        "a",
        &root_a,
        trash_into(dir.path().join("bin"), true),
    );
    open_vault(
        &app,
        "vault-b",
        "b",
        &root_b,
        trash_into(dir.path().join("bin"), true),
    );

    let tree_b: TreeListResponse = typed(
        "tree_list",
        invoke(&b, "tree_list", json!({})).expect("дерево"),
    );
    assert_eq!(
        tree_b.entries,
        vec![TreeEntry {
            path: VaultPath::from_components(["Только в Б.md"]),
            kind: EntryKind::Note
        }]
    );
    let error =
        call(&b, "note_read", json!({ "path": ["Только в А.md"] })).expect_err("чужая заметка");
    assert_eq!(core_error("note_read", error).code, ErrorCode::NotFound);
    let error = call(
        &b,
        "note_write",
        json!({ "path": ["Только в А.md"], "text": "взлом", "expected_version": { "hash": "0", "size": 0, "modified_ms": 0 } }),
    )
    .expect_err("чужая заметка");
    assert_eq!(core_error("note_write", error).code, ErrorCode::NotFound);
    let error =
        call(&b, "entry_trash", json!({ "path": ["Только в А.md"] })).expect_err("чужая заметка");
    assert_eq!(core_error("entry_trash", error).code, ErrorCode::NotFound);
    assert_eq!(
        std::fs::read(root_a.join("Только в А.md")).expect("цела"),
        b"a"
    );
    let found: SwitcherSearchResponse = typed(
        "switcher_search",
        call(&b, "switcher_search", json!({ "query": "только" })).expect("поиск"),
    );
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].name, "Только в Б");

    // Окно без хранилища не видит ничьё.
    let error = invoke(&none, "tree_list", json!({})).expect_err("нет хранилища");
    assert_eq!(core_error("tree_list", error).code, ErrorCode::NoVaultOpen);
    assert_eq!(
        invoke(&a, "vault_current", json!({})).expect("ответ")["vault"]["id"],
        json!("a")
    );

    // Окно закрыто — хранилище отвязано.
    app.state::<Core>().unbind_window("vault-a");
    let error = invoke(&a, "tree_list", json!({})).expect_err("отвязано");
    assert_eq!(core_error("tree_list", error).code, ErrorCode::NoVaultOpen);
}

/// `window_shown_note` (задача 2.12, решение 82): только заметка хранилища
/// своего окна; окно без хранилища (окно управления) — отказ.
#[test]
fn shown_note_is_checked_against_the_vault_of_the_window() {
    let app = app();
    let b = window(&app, "vault-shown-b");
    let dir = tempfile::tempdir().expect("папка");
    let (root_a, root_b) = (dir.path().join("А"), dir.path().join("Б"));
    put(&root_a, "Только в А.md", b"a");
    put(&root_b, "Папка/Только в Б.md", b"b");
    open_vault(
        &app,
        "vault-shown-a",
        "a",
        &root_a,
        trash_into(dir.path().join("bin"), true),
    );
    open_vault(
        &app,
        "vault-shown-b",
        "b",
        &root_b,
        trash_into(dir.path().join("bin"), true),
    );
    let shown = |w: &WebviewWindow<MockRuntime>, request: Value| {
        call(w, "window_shown_note", request)
            .map_err(|error| core_error("window_shown_note", error).code)
    };

    assert_eq!(
        shown(&b, json!({ "path": ["Папка", "Только в Б.md"] })),
        Ok(Value::Null)
    );
    assert_eq!(shown(&b, json!({ "path": null })), Ok(Value::Null));
    // Заметка другого хранилища, папка, путь за контуром.
    assert_eq!(
        shown(&b, json!({ "path": ["Только в А.md"] })),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        shown(&b, json!({ "path": ["Папка"] })),
        Err(ErrorCode::NotANote)
    );
    assert_eq!(shown(&b, json!({ "path": [] })), Err(ErrorCode::NotANote));
    assert_eq!(
        shown(&b, json!({ "path": ["..", "А", "Только в А.md"] })),
        Err(ErrorCode::InvalidPath)
    );
    // Лишнее поле (например, готовый заголовок) отклоняется мостом.
    assert!(
        call(
            &b,
            "window_shown_note",
            json!({ "path": null, "title": "Чужой текст" })
        )
        .is_err()
    );
    // Окно управления и окно без хранилища.
    let m = manager_window(&app, "manager-c0ffee0000000002");
    assert_eq!(
        shown(&m, json!({ "path": ["Только в А.md"] })),
        Err(ErrorCode::NoVaultOpen)
    );
    let none = window(&app, "vault-shown-none");
    assert_eq!(
        shown(&none, json!({ "path": null })),
        Err(ErrorCode::NoVaultOpen)
    );
}

#[test]
fn events_reach_only_the_window_of_their_vault() {
    let app = app();
    let a = window(&app, "events-a");
    let b = window(&app, "events-b");
    let dir = tempfile::tempdir().expect("папка");
    let (root_a, root_b) = (dir.path().join("А"), dir.path().join("Б"));
    put(&root_a, "Заметка.md", b"1");
    put(&root_b, "Другая.md", b"1");
    open_vault(
        &app,
        "events-a",
        "a",
        &root_a,
        trash_into(dir.path().join("bin"), true),
    );
    open_vault(
        &app,
        "events-b",
        "b",
        &root_b,
        trash_into(dir.path().join("bin"), true),
    );
    let names = [
        "vault:changed",
        "note:changed",
        "note:deleted",
        "note:renamed",
    ];
    let seen_a = record_events(&a, &names);
    let seen_b = record_events(&b, &names);

    // Внешняя правка в хранилище А.
    std::fs::write(root_a.join("Заметка.md"), "снаружи").expect("правка");

    assert!(
        wait_for(&seen_a, |events| events
            .iter()
            .any(|(name, payload)| name == "note:changed"
                && payload["path"] == json!(["Заметка.md"]))),
        "окно А не получило note:changed: {:?}",
        seen_a.lock().expect("журнал")
    );
    assert!(
        seen_a
            .lock()
            .expect("журнал")
            .iter()
            .any(|(name, _)| name == "vault:changed")
    );
    // Окно Б за это время ничего не получило.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        seen_b.lock().expect("журнал").is_empty(),
        "чужое окно получило события: {:?}",
        seen_b.lock().expect("журнал")
    );

    // Переименование снаружи — note:renamed (история переходов интерфейса
    // ждёт его), только окну А.
    std::fs::rename(root_a.join("Заметка.md"), root_a.join("Переименована.md")).expect("перенос");
    assert!(
        wait_for(&seen_a, |events| events
            .iter()
            .any(|(name, payload)| name == "note:renamed"
                && payload["from"] == json!(["Заметка.md"])
                && payload["to"] == json!(["Переименована.md"]))),
        "окно А не получило note:renamed: {:?}",
        seen_a.lock().expect("журнал")
    );
    assert!(seen_b.lock().expect("журнал").is_empty());

    // Удаление снаружи — note:deleted, тоже только окну А.
    std::fs::remove_file(root_a.join("Переименована.md")).expect("удаление");
    assert!(wait_for(&seen_a, |events| events
        .iter()
        .any(|(name, payload)| name == "note:deleted"
            && payload["path"] == json!(["Переименована.md"]))));
    assert!(seen_b.lock().expect("журнал").is_empty());
}

#[test]
fn own_write_is_not_reported_as_external_change() {
    let app = app();
    let w = window(&app, "own-write");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Заметка.md", b"1");
    open_vault(
        &app,
        "own-write",
        "own",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    let seen = record_events(&w, &["note:changed", "note:conflict-saved"]);

    let read: NoteReadResponse = typed(
        "note_read",
        call(&w, "note_read", json!({ "path": ["Заметка.md"] })).expect("чтение"),
    );
    let written: NoteWriteResponse = typed(
        "note_write",
        call(
            &w,
            "note_write",
            // На Windows путь можно прислать в другом регистре, чем на диске:
            // журнал своих записей сравнивает по ключу имени.
            json!({ "path": [if cfg!(windows) { "заметка.md" } else { "Заметка.md" }], "text": "своё", "expected_version": read.version }),
        )
        .expect("запись"),
    );
    assert!(
        matches!(written, NoteWriteResponse::Saved { .. }),
        "{written:?}"
    );

    // Своя запись не стала внешним изменением.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        seen.lock().expect("журнал").is_empty(),
        "{:?}",
        seen.lock().expect("журнал")
    );

    // Конфликт: окну — note:conflict-saved.
    std::fs::write(root.join("Заметка.md"), "снаружи").expect("правка");
    let written: NoteWriteResponse = typed(
        "note_write",
        call(
            &w,
            "note_write",
            json!({ "path": ["Заметка.md"], "text": "моё", "expected_version": read.version }),
        )
        .expect("запись"),
    );
    let NoteWriteResponse::ConflictSaved { copy_path, .. } = written else {
        panic!("ожидался конфликт: {written:?}");
    };
    assert!(wait_for(&seen, |events| events
        .iter()
        .any(|(name, payload)| name == "note:conflict-saved"
            && payload["copy_path"]
                == serde_json::to_value(&copy_path).expect("путь"))));
}

// ---- Диалоги ядра ---------------------------------------------------------

#[test]
fn permanent_delete_needs_confirmation_in_core_dialog() {
    let app = app();
    let w = window(&app, "no-trash");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("флешка");
    put(&root, "Отказ.md", b"1");
    put(&root, "Согласие.md", b"1");
    open_vault(
        &app,
        "no-trash",
        "flash",
        &root,
        trash_into(dir.path().join("bin"), false),
    );

    test_answers::set("no-trash", Answers::default());
    assert_eq!(
        call(&w, "entry_trash", json!({ "path": ["Отказ.md"] })).expect("ответ"),
        json!({ "outcome": "cancelled" })
    );
    assert!(root.join("Отказ.md").is_file(), "отказ — файл на месте");

    test_answers::set(
        "no-trash",
        Answers {
            confirm_delete: true,
            ..Answers::default()
        },
    );
    assert_eq!(
        call(&w, "entry_trash", json!({ "path": ["Согласие.md"] })).expect("ответ"),
        json!({ "outcome": "deleted_permanently" })
    );
    assert!(!root.join("Согласие.md").exists());
    let found: SwitcherSearchResponse = typed(
        "switcher_search",
        call(&w, "switcher_search", json!({ "query": "согласие" })).expect("поиск"),
    );
    assert!(found.items.is_empty(), "удалённая заметка ушла из поиска");
}

/// Пока в окне открыт диалог ядра, выбор папки в том же окне не
/// показывается (общий замок диалогов ядра, `dialogs::DialogSlot`) — ответ
/// `cancelled`. С задачи 2.8б выбор папки есть только у окна управления;
/// общий замок для всех видов диалогов проверяет и `dialogs::tests`.
#[test]
fn core_dialogs_share_one_lock_per_window() {
    let app = app();
    let label = "manager-b0b0000000000001";
    let w = manager_window(&app, label);
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("Флешка");
    put(&root, "Заметка.md", b"1");
    test_answers::set(
        label,
        Answers {
            folder: Some(root.clone()),
            ..Answers::default()
        },
    );

    let open_dialog = crate::dialogs::DialogSlot::take(label).expect("окно свободно");
    for name in ["vault_open_folder", "folder_pick"] {
        assert_eq!(
            invoke(&w, name, json!({})).expect("ответ"),
            json!({ "outcome": "cancelled" }),
            "{name}"
        );
    }
    drop(open_dialog);

    // Окно свободно — выбор папки доходит до списка хранилищ: папка
    // становится хранилищем и открывается в новом окне.
    let opened: VaultChooseOutcome = typed(
        "vault_open_folder",
        invoke(&w, "vault_open_folder", json!({})).expect("ответ"),
    );
    let VaultChooseOutcome::Opened { vault } = opened else {
        panic!("ожидалось открытие: {opened:?}");
    };
    let core = app.state::<Core>();
    let opened_in = core.window_of_vault(&vault.id).expect("открыто");
    assert_ne!(opened_in, label);
    assert!(app.get_webview_window(&opened_in).is_some());
}

/// Решение владельца 62: «Обзор…» (`folder_pick`) → «Создать»
/// (`vault_create` с той же папкой). Папка внутри хранилища — `nested_vault`;
/// папка, которую не выбирали в окне ядра, — `invalid_argument` (`parent`).
#[test]
fn vault_is_created_in_folder_picked_by_core_dialog() {
    let app = app();
    let label = "manager-b0b0000000000002";
    let w = manager_window(&app, label);
    let dir = tempfile::tempdir().expect("папка");
    let parent = dir.path().join("Заметки");
    std::fs::create_dir(&parent).expect("папка");
    let other = dir.path().join("Другая");
    std::fs::create_dir(&other).expect("папка");
    let canonical = dunce::canonicalize(&parent).expect("путь");
    let path = canonical.to_str().expect("путь строкой").to_owned();

    test_answers::set(
        label,
        Answers {
            folder: Some(parent.clone()),
            ..Answers::default()
        },
    );
    assert_eq!(
        invoke(&w, "folder_pick", json!({})).expect("folder_pick"),
        json!({ "outcome": "picked", "path": path })
    );

    // Другую папку подставить нельзя.
    let other_path = dunce::canonicalize(&other).expect("путь");
    let error = core_error(
        "vault_create",
        call(
            &w,
            "vault_create",
            json!({ "name": "Проекты", "parent": other_path.to_str().expect("путь") }),
        )
        .expect_err("не выбранная папка"),
    );
    assert_eq!(error.field.as_deref(), Some("parent"));
    assert!(!other.join("Проекты").exists());

    let created: VaultChooseOutcome = typed(
        "vault_create",
        call(
            &w,
            "vault_create",
            json!({ "name": "Проекты", "parent": path }),
        )
        .expect("vault_create"),
    );
    let VaultChooseOutcome::Opened { vault } = created else {
        panic!("ожидалось открытие: {created:?}");
    };
    assert!(canonical.join("Проекты").join(".rhizomind").is_dir());
    assert_eq!(vault.name, "Проекты");

    // Внутри созданного хранилища — нельзя. Окно управления после открытия
    // хранилища закрылось; в тесте — снова то же окно.
    app.state::<Core>().reserve_manager(label);
    test_answers::set(
        label,
        Answers {
            folder: Some(canonical.join("Проекты")),
            ..Answers::default()
        },
    );
    let error = core_error(
        "folder_pick",
        invoke(&w, "folder_pick", json!({})).expect_err("внутри хранилища"),
    );
    assert_eq!(error.code, ErrorCode::NestedVault);
}

/// Задача 2.8в: служебную папку хранилища и папку внутри неё нельзя открыть
/// как хранилище — `nested_vault`, список хранилищ не меняется.
#[test]
fn service_folder_is_not_opened_as_vault() {
    let app = app();
    let label = "manager-b0b0000000000003";
    let w = manager_window(&app, label);
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("Хранилище");
    put(&root, "Заметка.md", b"1");
    crate::vault::Vault::open(&root).expect("хранилище");
    for folder in [root.join(".rhizomind"), root.join(".rhizomind").join("tmp")] {
        test_answers::set(
            label,
            Answers {
                folder: Some(folder.clone()),
                ..Answers::default()
            },
        );
        let error = core_error(
            "vault_open_folder",
            invoke(&w, "vault_open_folder", json!({})).expect_err("служебная папка"),
        );
        assert_eq!(error.code, ErrorCode::NestedVault, "{}", folder.display());
    }
    let list: VaultListResponse = typed(
        "vault_list",
        invoke(&w, "vault_list", json!({})).expect("vault_list"),
    );
    assert!(list.vaults.is_empty());
}

// ---- Скорость ---------------------------------------------------------------

/// Индекс быстрого перехода живёт в состоянии окна: строится при открытии
/// хранилища и обновляется по изменениям, а не на каждый запрос. Ответ
/// `switcher_search` на 10 000 заметках — не дольше 100 мс (задача 2.7).
/// Только в выпускной сборке, как другие тесты скорости:
/// `cargo test -p rhizomind --release --lib -- --ignored` (шаг CI).
#[test]
#[ignore = "тест производительности: запускается отдельным шагом CI в выпускной сборке"]
fn switcher_search_on_10k_notes_answers_within_100ms() {
    let app = app();
    let w = window(&app, "speed");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("большое");
    for folder in 0..100 {
        let path = root.join(format!("Папка {folder}"));
        std::fs::create_dir_all(&path).expect("папка");
        for note in 0..100 {
            std::fs::write(path.join(format!("Заметка {folder}-{note}.md")), b"").expect("заметка");
        }
    }
    open_vault(
        &app,
        "speed",
        "speed",
        &root,
        trash_into(dir.path().join("bin"), true),
    );

    let mut worst = Duration::ZERO;
    for query in ["зам 42", "папка 7 99", "з", "нет такой", "заметка 99-99"]
    {
        let start = std::time::Instant::now();
        let found: SwitcherSearchResponse = typed(
            "switcher_search",
            call(&w, "switcher_search", json!({ "query": query })).expect("поиск"),
        );
        let took = start.elapsed();
        worst = worst.max(took);
        assert!(took < Duration::from_millis(100), "«{query}»: {took:?}");
        assert!(found.items.len() <= 50);
    }
    eprintln!("switcher_search на 10 000 заметках: худший ответ {worst:?}");
}

// ---- Открытие хранилища -------------------------------------------------------

fn summary(id: &str, root: &Path) -> VaultSummary {
    VaultSummary {
        id: VaultId(id.into()),
        name: id.into(),
        path: root.display().to_string(),
    }
}

/// Изменения снаружи между обходом папки и записью списка заметок не
/// теряются: события наблюдателя ждут замка списка и применяются к полному
/// списку, а не затираются им.
#[test]
fn changes_during_vault_opening_are_not_lost() {
    let app = app();
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Уйдёт.md", b"1");
    put(&root, "Останется.md", b"1");
    let core = app.state::<Core>();

    let open = core
        .open_vault_observed(
            "opening-race",
            summary("race", &root),
            &root,
            Arc::new(WindowEmitter::new(app.handle().clone(), "opening-race")),
            Some(trash_into(dir.path().join("bin"), true)),
            &|| {
                // Обход уже прошёл: этих изменений в нём нет.
                put(&root, "Новая.md", b"");
                std::fs::remove_file(root.join("Уйдёт.md")).expect("удаление");
                // Время наблюдателю увидеть изменения (раньше он успевал
                // применить их до записи списка, и список их затирал).
                std::thread::sleep(Duration::from_millis(1500));
            },
        )
        .expect("открыто");

    let start = std::time::Instant::now();
    loop {
        let index = open.read_index();
        let fresh = index.contains(&VaultPath::from_components(["Новая.md"]))
            && !index.contains(&VaultPath::from_components(["Уйдёт.md"]));
        if fresh {
            break;
        }
        drop(index);
        assert!(
            start.elapsed() < WAIT,
            "изменения во время открытия потеряны"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        open.read_index()
            .contains(&VaultPath::from_components(["Останется.md"]))
    );
}

/// Окно закрыли, пока хранилище открывалось: хранилище к окну не
/// привязывается, наблюдатель не остаётся.
#[test]
fn window_closed_during_opening_gets_no_vault() {
    let app = app();
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Заметка.md", b"1");
    let core = app.state::<Core>();

    let result = core.open_vault_observed(
        "closed-early",
        summary("closed", &root),
        &root,
        Arc::new(WindowEmitter::new(app.handle().clone(), "closed-early")),
        Some(trash_into(dir.path().join("bin"), true)),
        &|| core.unbind_window("closed-early"),
    );

    let open = match result {
        Err(error) => {
            assert_eq!(error.code, ErrorCode::NoVaultOpen);
            None
        }
        Ok(open) => Some(open),
    };
    assert!(open.is_none(), "открытие закрытого окна должно отказать");
    assert!(core.window_vault("closed-early").is_none());
    assert!(core.window_of_vault(&VaultId("closed".into())).is_none());

    // Закрытое окно закрыто навсегда (метки окон не повторяются); другое
    // окно открывает то же хранилище.
    let error = core
        .open_vault_with(
            "closed-early",
            summary("closed", &root),
            &root,
            Arc::new(WindowEmitter::new(app.handle().clone(), "closed-early")),
            Some(trash_into(dir.path().join("bin"), true)),
        )
        .expect_err("окно закрыто");
    assert_eq!(error.code, ErrorCode::NoVaultOpen);
    core.open_vault_with(
        "opened-later",
        summary("closed", &root),
        &root,
        Arc::new(WindowEmitter::new(app.handle().clone(), "opened-later")),
        Some(trash_into(dir.path().join("bin"), true)),
    )
    .expect("открыто");
    assert!(core.window_vault("opened-later").is_some());
}

/// Обзор 1.7: окно закрыли (`unbind_window`) раньше, чем открытие записало
/// свой номер, — хранилище не привязывается к закрытому окну, наблюдатель не
/// остаётся.
#[test]
fn window_closed_before_opening_started_gets_no_vault() {
    let app = app();
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Заметка.md", b"1");
    let core = app.state::<Core>();

    core.unbind_window("closed-first");
    let error = core
        .open_vault_with(
            "closed-first",
            summary("first", &root),
            &root,
            Arc::new(WindowEmitter::new(app.handle().clone(), "closed-first")),
            Some(trash_into(dir.path().join("bin"), true)),
        )
        .expect_err("окно уже закрыто");

    assert_eq!(error.code, ErrorCode::NoVaultOpen);
    assert!(core.window_vault("closed-first").is_none());
    assert!(core.window_of_vault(&VaultId("first".into())).is_none());
}

/// Папка новых заметок в настройках — путь по компонентам и не служебная.
#[test]
fn settings_new_note_folder_must_be_a_vault_path() {
    let app = app();
    let w = window(&app, "settings-path");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Заметка.md", b"1");
    open_vault(
        &app,
        "settings-path",
        "settings",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    let mut settings = example("settings_write")["settings"].clone();

    for (path, code) in [
        (json!([".."]), ErrorCode::InvalidPath),
        (json!(["a", ".Rhizomind"]), ErrorCode::InvalidPath),
        (json!([".rhizomind"]), ErrorCode::InvalidPath),
        (json!(["C:"]), ErrorCode::InvalidPath),
    ] {
        settings["new_note_location"] = json!({ "kind": "folder", "path": path });
        let error =
            call(&w, "settings_write", json!({ "settings": settings })).expect_err("плохая папка");
        assert_eq!(core_error("settings_write", error).code, code, "{path}");
    }
    settings["new_note_location"] = json!({ "kind": "folder", "path": ["Входящие"] });
    call(&w, "settings_write", json!({ "settings": settings })).expect("обычная папка");
}
