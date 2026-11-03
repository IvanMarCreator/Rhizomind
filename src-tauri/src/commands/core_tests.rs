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
use std::sync::{Arc, Mutex, RwLock};
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
    let open = core
        .open_vault_with(
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
        .expect("хранилище открыто");
    // Индекс готов и его события отправлены: слушатели `record_events`,
    // зарегистрированные после, не теряются (`WindowIndex::wait_settled`).
    open.index.wait_settled();
    open
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
        let index = open.read_notes();
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
        open.read_notes()
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

// ---- 0.2-1.4: индекс в работе окна ------------------------------------------

/// Ждёт готовности индекса окна (`index_status`).
fn wait_index_ready(w: &WebviewWindow<MockRuntime>) {
    wait_index_ready_within(w, WAIT);
}

/// [`wait_index_ready`] с пределом `limit` (большое хранилище).
fn wait_index_ready_within(w: &WebviewWindow<MockRuntime>, limit: Duration) {
    let start = std::time::Instant::now();
    loop {
        let status: IndexStatus = typed(
            "index_status",
            invoke(w, "index_status", json!({})).expect("index_status"),
        );
        if status.state == IndexState::Ready {
            return;
        }
        assert!(start.elapsed() < limit, "индекс не готов: {status:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Хранилище с индексом на настоящем ядре: открыто той же функцией, что
/// `vault_open`; ждём готовности индекса.
fn indexed_vault(
    app: &App<MockRuntime>,
    label: &str,
    files: &[(&str, &str)],
) -> (WebviewWindow<MockRuntime>, tempfile::TempDir, PathBuf) {
    let w = window(app, label);
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    std::fs::create_dir_all(&root).expect("папка");
    for (rel, text) in files {
        put(&root, rel, text.as_bytes());
    }
    open_vault(
        app,
        label,
        label,
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    wait_index_ready(&w);
    (w, dir, root)
}

fn backlinks(w: &WebviewWindow<MockRuntime>, path: &[&str]) -> Backlinks {
    let reply: IndexReply<Backlinks> = typed(
        "backlinks_list",
        call(w, "backlinks_list", json!({ "path": path })).expect("backlinks_list"),
    );
    match reply {
        IndexReply::Ready(backlinks) => backlinks,
        IndexReply::IndexBuilding(progress) => panic!("индекс строится: {progress:?}"),
    }
}

/// Отрезок строки в единицах UTF-16 — как `String.prototype.slice` в окне.
fn utf16_slice(text: &str, from: u32, to: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[from as usize..to as usize]).expect("UTF-16")
}

#[test]
fn backlinks_list_real_core() {
    let app = app();
    let (w, _dir, _root) = indexed_vault(
        &app,
        "backlinks",
        &[
            ("Узел.md", "# Узел\nСсылка на себя [[Узел]]\n"),
            (
                "Вторая.md",
                "Начало\nТекст [[Узел]] и [[Узел#Итоги|итоги]]\n",
            ),
            ("Папка/Альфа.md", "![[узел]]\n"),
            ("Папка/Заметка 10.md", "[[Узел]]"),
            ("Заметка 2.md", "[[Узел]]"),
            ("Код.md", "`[[Узел]]`\n```\n[[Узел]]\n```\n"),
            ("Другое.md", "[[Другое]] [[Нет такой]]"),
        ],
    );

    let got = backlinks(&w, &["Узел.md"]);
    assert_eq!(got.notes_total, 4);
    assert!(!got.truncated);
    // Естественный порядок имён, как в дереве файлов: «2» раньше «10».
    let names: Vec<&str> = got.notes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Альфа", "Вторая", "Заметка 2", "Заметка 10"]);
    let second = &got.notes[1];
    assert_eq!(second.from, VaultPath::from_components(["Вторая.md"]));
    assert_eq!(second.links_total, 2);
    let lines: Vec<(u32, u32, &str, String)> = second
        .links
        .iter()
        .map(|l| {
            (
                l.line,
                l.column_utf16,
                l.snippet.as_str(),
                utf16_slice(&l.snippet, l.from_utf16, l.to_utf16),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            (
                2,
                6,
                "Текст [[Узел]] и [[Узел#Итоги|итоги]]",
                "[[Узел]]".to_owned()
            ),
            (
                2,
                17,
                "Текст [[Узел]] и [[Узел#Итоги|итоги]]",
                "[[Узел#Итоги|итоги]]".to_owned()
            ),
        ]
    );
    assert_eq!(
        utf16_slice(
            &got.notes[0].links[0].snippet,
            got.notes[0].links[0].from_utf16,
            got.notes[0].links[0].to_utf16
        ),
        "![[узел]]"
    );

    // Заметки нет — «Нет обратных ссылок», не ошибка.
    assert_eq!(backlinks(&w, &["Нет.md"]).notes_total, 0);
    // Путь в другом регистре — та же заметка.
    assert_eq!(backlinks(&w, &["узел.md"]).notes_total, 4);
}

#[test]
fn backlinks_snippet_cut_to_200() {
    let app = app();
    let long_before = "а".repeat(500);
    let long_after = "б".repeat(500);
    let middle = format!("{long_before} [[Узел]] {long_after}\n");
    let start = format!("[[Узел]] {long_after}\n");
    let end = format!("{long_before} [[Узел]]\n");
    let huge_link = format!("[[Узел|{}]]\n", "в".repeat(300));
    let (w, _dir, _root) = indexed_vault(
        &app,
        "snippet",
        &[
            ("Узел.md", ""),
            ("Середина.md", &middle),
            ("Начало.md", &start),
            ("Конец.md", &end),
            ("Длинная ссылка.md", &huge_link),
        ],
    );
    let got = backlinks(&w, &["Узел.md"]);
    assert_eq!(got.notes_total, 4);
    for note in &got.notes {
        let line = &note.links[0];
        let chars = line.snippet.chars().count();
        assert_eq!(chars, limits::MAX_SNIPPET_CHARS, "{}: {chars}", note.name);
        let link = utf16_slice(&line.snippet, line.from_utf16, line.to_utf16);
        if note.name == "Длинная ссылка" {
            assert!(link.starts_with("[[Узел|"), "{link}");
            assert_eq!(line.from_utf16, 0);
        } else {
            assert_eq!(link, "[[Узел]]", "{}", note.name);
        }
        match note.name.as_str() {
            // Ссылка посередине — поровну текста слева и справа.
            "Середина" => {
                let left = line.from_utf16 as usize;
                let right = line.snippet.encode_utf16().count() - line.to_utf16 as usize;
                assert!(left.abs_diff(right) <= 1, "{left} / {right}");
                assert_eq!(line.column_utf16, 501);
            }
            "Начало" => assert_eq!(line.from_utf16, 0),
            "Конец" => {
                assert_eq!(line.to_utf16 as usize, line.snippet.encode_utf16().count());
            }
            _ => {}
        }
    }
}

#[test]
fn backlinks_utf16_offsets_highlight_link() {
    let app = app();
    // Эмодзи — две единицы UTF-16, буква с ударением — две точки кода.
    let text = "😀 е\u{301} 𝔸 [[Узел]] после\n";
    let (w, _dir, _root) = indexed_vault(&app, "utf16", &[("Узел.md", ""), ("Эмодзи.md", text)]);
    let got = backlinks(&w, &["Узел.md"]);
    let line = &got.notes[0].links[0];
    assert_eq!(line.snippet, text.trim_end());
    // 😀 (2) + пробел + е + ударение + пробел + 𝔸 (2) + пробел = 9.
    assert_eq!(line.from_utf16, 9);
    assert_eq!(line.column_utf16, 9);
    assert_eq!(line.to_utf16, 17);
    assert_eq!(
        utf16_slice(&line.snippet, line.from_utf16, line.to_utf16),
        "[[Узел]]"
    );
}

/// Строка с HTML возвращается как есть — текстом: ядро не вырезает и не
/// экранирует разметку, окно показывает её как текст (С2-6).
#[test]
fn backlinks_html_returned_as_text() {
    let app = app();
    let text = "<img src=x onerror=alert(1)> [[Узел]] <script>alert(2)</script>\n";
    let (w, _dir, _root) = indexed_vault(&app, "html", &[("Узел.md", ""), ("Html.md", text)]);
    let got = backlinks(&w, &["Узел.md"]);
    let line = &got.notes[0].links[0];
    assert_eq!(line.snippet, text.trim_end());
    assert_eq!(
        utf16_slice(&line.snippet, line.from_utf16, line.to_utf16),
        "[[Узел]]"
    );
}

/// Подменённый `index.json` с положениями за концом текста, внутри символа
/// и не на ссылке: команда не падает, такие строки пропускаются.
#[test]
fn backlinks_forged_index_positions_do_not_panic() {
    let app = app();
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    // BOM и `\r\n`: текст `read_note` короче файла.
    put(&root, "Узел.md", b"");
    put(
        &root,
        "Подмена.md",
        "\u{feff}Привет [[Узел]]\r\nещё [[Узел]]\r\n".as_bytes(),
    );
    // Настоящий индекс — затем подмена положений с теми же размером и датой.
    {
        let vault = crate::vault::Vault::open(&root).expect("хранилище");
        let index = RwLock::new(crate::index::VaultIndex::new());
        crate::index::rebuild(&vault, &index, &|_| {}).expect("индекс");
        crate::index::store::IndexStore::new().write_now(&vault, &index);
    }
    let file = root.join(".rhizomind").join("index.json");
    let mut json: Value =
        serde_json::from_slice(&std::fs::read(&file).expect("индекс")).expect("JSON");
    let size = std::fs::metadata(root.join("Подмена.md"))
        .expect("файл")
        .len();
    let note = json["notes"]
        .as_array_mut()
        .expect("заметки")
        .iter_mut()
        .find(|n| n["p"] == json!(["Подмена.md"]))
        .expect("заметка в индексе");
    let link = note["l"][0].clone();
    let forged = |start: u64, end: u64| {
        let mut l = link.clone();
        l[4] = json!(start);
        l[5] = json!(end);
        l[6] = json!(start);
        l[7] = json!(end);
        l
    };
    note["l"] = json!([
        forged(1, 3),           // внутри «П»
        forged(size - 2, size), // за концом текста (BOM и `\r` убраны)
        forged(0, 6),           // не ссылка
        forged(13, 25),         // настоящая ссылка `[[Узел]]`
    ]);
    std::fs::write(&file, serde_json::to_vec(&json).expect("JSON")).expect("подмена");

    let w = window(&app, "forged");
    open_vault(
        &app,
        "forged",
        "forged",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    wait_index_ready(&w);
    let got = backlinks(&w, &["Узел.md"]);
    assert_eq!(got.notes_total, 1);
    let note = &got.notes[0];
    assert_eq!(note.links_total, 4);
    let shown: Vec<String> = note
        .links
        .iter()
        .map(|l| utf16_slice(&l.snippet, l.from_utf16, l.to_utf16))
        .collect();
    assert_eq!(shown, ["[[Узел]]"]);
    assert_eq!(note.links[0].line, 1);
}

#[test]
fn tags_list_real_core() {
    let app = app();
    let (w, _dir, _root) = indexed_vault(
        &app,
        "tags",
        &[
            ("А.md", "#Проект и #проект #идея"),
            ("Б.md", "---\ntags: [ПРОЕКТ]\n---\n`#не-тег` #Проект"),
            ("В.md", "#123 текст"),
        ],
    );
    let reply: IndexReply<Tags> = typed(
        "tags_list",
        invoke(&w, "tags_list", json!({})).expect("tags_list"),
    );
    let IndexReply::Ready(Tags { mut tags }) = reply else {
        panic!("индекс строится");
    };
    tags.sort_by(|a, b| a.key.cmp(&b.key));
    let got: Vec<(&str, &str, u32)> = tags
        .iter()
        .map(|t| (t.tag.as_str(), t.key.as_str(), t.count))
        .collect();
    assert_eq!(got, [("идея", "идея", 1), ("Проект", "проект", 2)]);

    let reply: IndexReply<TagNotes> = typed(
        "tag_notes",
        call(&w, "tag_notes", json!({ "key": "проект" })).expect("tag_notes"),
    );
    assert_eq!(
        reply,
        IndexReply::Ready(TagNotes {
            notes_total: 2,
            notes: vec![
                VaultPath::from_components(["А.md"]),
                VaultPath::from_components(["Б.md"])
            ],
        })
    );
    let reply: IndexReply<TagNotes> = typed(
        "tag_notes",
        call(&w, "tag_notes", json!({ "key": "нет-такого" })).expect("tag_notes"),
    );
    assert_eq!(
        reply,
        IndexReply::Ready(TagNotes {
            notes_total: 0,
            notes: vec![],
        })
    );
}

#[test]
fn tag_notes_limit_10000() {
    let app = app();
    let w = window(&app, "tag-limit");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    for folder in 0..11 {
        let path = root.join(format!("п{folder}"));
        std::fs::create_dir_all(&path).expect("папка");
        for note in 0..910 {
            std::fs::write(path.join(format!("{note}.md")), "#общий").expect("заметка");
        }
    }
    open_vault(
        &app,
        "tag-limit",
        "tag-limit",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    wait_index_ready(&w);
    let reply: IndexReply<TagNotes> = typed(
        "tag_notes",
        call(&w, "tag_notes", json!({ "key": "общий" })).expect("tag_notes"),
    );
    let IndexReply::Ready(notes) = reply else {
        panic!("индекс строится");
    };
    assert_eq!(notes.notes_total, 10_010);
    assert_eq!(notes.notes.len(), limits::MAX_TAG_NOTES);
}

/// Пока индекс строится, команды отвечают `index_building` с ходом; по
/// готовности — `index:status` с `ready`, и те же команды дают данные.
#[test]
fn index_status_building_then_ready() {
    let app = app();
    let w = window(&app, "building");
    let seen = record_events(&w, &[event::INDEX_STATUS, event::INDEX_CHANGED]);
    let dir = tempfile::tempdir().expect("папка");
    put(dir.path(), "Узел.md", b"");
    put(dir.path(), "А.md", "[[Узел]] #тег".as_bytes());
    let core = app.state::<Core>();
    core.bind_window(
        "building",
        OpenVault::new(
            summary("building", dir.path()),
            Vault::open(dir.path()).expect("хранилище"),
        )
        .expect("заметки"),
    );
    // Индекс ещё не запущен — строится.
    assert_eq!(
        invoke(&w, "index_status", json!({})).expect("index_status"),
        json!({ "state": "building", "done": 0, "total": 0 })
    );
    for (name, args) in [
        (
            "backlinks_list",
            json!({ "request": { "path": ["Узел.md"] } }),
        ),
        ("tags_list", json!({})),
        ("tag_notes", json!({ "request": { "key": "тег" } })),
    ] {
        assert_eq!(
            invoke(&w, name, args).expect(name)["status"],
            json!("index_building"),
            "{name}"
        );
    }

    let open = core.window_vault("building").expect("хранилище окна");
    start_index(
        &open,
        Arc::new(WindowEmitter::new(app.handle().clone(), "building")),
    );
    assert!(
        wait_for(&seen, |events| events.iter().any(|(name, payload)| {
            name == event::INDEX_STATUS && payload["state"] == json!("ready")
        })),
        "index:status ready"
    );
    for (name, payload) in seen.lock().expect("журнал").iter() {
        if name == event::INDEX_STATUS {
            let status: IndexStatus = typed(name, payload.clone());
            assert!(
                status.done <= status.total || status.total == 0,
                "{status:?}"
            );
        }
    }
    assert_eq!(
        invoke(&w, "index_status", json!({})).expect("index_status"),
        json!({ "state": "ready", "done": 2, "total": 2 })
    );
    assert_eq!(backlinks(&w, &["Узел.md"]).notes_total, 1);
}

/// `note_write` обновляет индекс сразу после своей записи: обратная ссылка
/// видна без наблюдателя, `index:changed` приходит.
#[test]
fn note_write_updates_backlinks() {
    let app = app();
    let (w, _dir, _root) = indexed_vault(&app, "own-write", &[("Узел.md", ""), ("А.md", "пусто")]);
    let seen = record_events(&w, &[event::INDEX_CHANGED]);
    let read: NoteReadResponse = typed(
        "note_read",
        call(&w, "note_read", json!({ "path": ["А.md"] })).expect("note_read"),
    );
    call(
        &w,
        "note_write",
        json!({ "path": ["А.md"], "text": "теперь [[Узел]]", "expected_version": read.version }),
    )
    .expect("note_write");
    assert_eq!(backlinks(&w, &["Узел.md"]).notes_total, 1);
    assert!(
        wait_for(&seen, |events| !events.is_empty()),
        "index:changed"
    );
}

/// Пределы ответа на настоящем ядре: 1 000 заметок, 100 строк на заметку,
/// 5 000 строк всего, признак `truncated`.
#[test]
fn backlinks_list_limits_real_core() {
    let app = app();
    let w = window(&app, "limits");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    put(&root, "Узел.md", b"");
    let many = "[[Узел]]\n".repeat(120);
    for i in 0..1_001 {
        // Первые 60 заметок по порядку имён — по 120 ссылок (7 200 строк).
        let text = if i < 60 {
            many.as_str()
        } else {
            "[[Узел]]"
        };
        put(&root, &format!("Ссылки/з{i:04}.md"), text.as_bytes());
    }
    open_vault(
        &app,
        "limits",
        "limits",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    wait_index_ready(&w);
    let got = backlinks(&w, &["Узел.md"]);
    assert_eq!(got.notes_total, 1_001);
    assert!(got.truncated);
    assert_eq!(got.notes.len(), limits::MAX_BACKLINK_NOTES);
    assert_eq!(got.notes[0].name, "з0000");
    assert_eq!(got.notes[0].links_total, 120);
    assert_eq!(got.notes[0].links.len(), limits::MAX_BACKLINK_LINES);
    let shown: usize = got.notes.iter().map(|n| n.links.len()).sum();
    assert_eq!(shown, limits::MAX_BACKLINK_LINES_TOTAL);
    // 50 заметок по 100 строк исчерпали общий предел: дальше — имена без строк.
    assert!(got.notes[50..].iter().all(|n| n.links.is_empty()));
    assert!(got.notes.iter().all(|n| n.links_total >= 1));

    // Ответ без превышения пределов — без признака.
    let small = backlinks(&w, &["Ссылки", "з0000.md"]);
    assert!(!small.truncated);
}

/// Заметку изменили снаружи — настоящий наблюдатель, `index:changed` не
/// позже 2 с (С2-3.2), обратная ссылка видна.
#[test]
fn external_change_gives_index_changed_within_2s() {
    let app = app();
    let (w, _dir, root) =
        indexed_vault(&app, "external", &[("Узел.md", ""), ("А.md", "без ссылок")]);
    let seen = record_events(&w, &[event::INDEX_CHANGED]);
    let start = std::time::Instant::now();
    std::fs::write(root.join("А.md"), "снаружи [[Узел]]").expect("запись снаружи");
    assert!(
        wait_for(&seen, |events| !events.is_empty()),
        "index:changed не пришло"
    );
    let took = start.elapsed();
    eprintln!("внешнее изменение → index:changed: {took:?}");
    assert!(took <= Duration::from_secs(2), "{took:?}");
    assert_eq!(backlinks(&w, &["Узел.md"]).notes_total, 1);
}

// ---- Замеры на хранилище генератора (справочно) ------------------------------

/// `cargo test -p rhizomind --lib commands::core_tests::index_queries_on_generated_10k -- --ignored --nocapture`.
/// Пределы (`backlinks_list` «узла» ≤ 200 мс, `tags_list` ≤ 100 мс) — в
/// выпускной сборке; в отладочной — только печать.
#[test]
#[ignore = "замер: --ignored"]
fn index_queries_on_generated_10k() {
    let app = app();
    let w = window(&app, "gen10k");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("v10k");
    let manifest =
        gen_vault::generate(&root, &gen_vault::Config::new(10_000, 7)).expect("генератор");
    open_vault(
        &app,
        "gen10k",
        "gen10k",
        &root,
        trash_into(dir.path().join("bin"), true),
    );
    // Сборка 10 000 заметок рядом с другими замерами на машине CI (Windows,
    // антивирус) — дольше обычного предела ожидания; время сборки здесь не
    // проверяется (это `index::tests::index_build_10k`).
    wait_index_ready_within(&w, Duration::from_secs(120));
    let hub = manifest.hub.as_ref().expect("узел");
    let hub_path: Vec<&str> = hub.path.split('/').collect();

    // Первый вызов — холодный: 100 только что созданных генератором файлов
    // читаются впервые (на Windows их при первом открытии проверяет
    // антивирус — в CI до 1,5 с). Он печатается отдельно; предел — на
    // повторных вызовах (файлы заметок пользователя не создаются за секунду
    // до запроса).
    let start = std::time::Instant::now();
    let mut hub_count = backlinks(&w, &hub_path).notes_total;
    let backlinks_cold = start.elapsed();
    let mut backlinks_worst = Duration::ZERO;
    let mut tags_worst = Duration::ZERO;
    let mut tags_count = 0;
    for _ in 0..5 {
        let start = std::time::Instant::now();
        hub_count = backlinks(&w, &hub_path).notes_total;
        backlinks_worst = backlinks_worst.max(start.elapsed());
        let start = std::time::Instant::now();
        let reply = invoke(&w, "tags_list", json!({})).expect("tags_list");
        tags_worst = tags_worst.max(start.elapsed());
        tags_count = reply["tags"].as_array().map_or(0, Vec::len);
    }
    let build = if cfg!(debug_assertions) {
        "отладочная сборка"
    } else {
        "выпускная сборка"
    };
    println!(
        "backlinks_list «узла»: {hub_count} заметок, худший {backlinks_worst:?} \
         (первый, холодный — {backlinks_cold:?}); \
         tags_list: {tags_count} тегов, худший {tags_worst:?} ({build})"
    );
    assert_eq!(hub_count, 100);
    assert_eq!(tags_count, manifest.tags.len());
    if !cfg!(debug_assertions) {
        assert!(
            backlinks_worst <= Duration::from_millis(200),
            "{backlinks_worst:?}"
        );
        assert!(tags_worst <= Duration::from_millis(100), "{tags_worst:?}");
    }
}

// ---- 0.2-4.6: переименование и перенос с обновлением ссылок -----------------

/// Окно отвечает на `links:flush-requested`: перед ответом выполняет
/// `before` (запись ввода окна), потом `links_flush_done` с `saved`.
fn answer_flush(
    w: &WebviewWindow<MockRuntime>,
    saved: bool,
    before: impl Fn(&WebviewWindow<MockRuntime>) + Send + Sync + 'static,
) {
    let window = w.clone();
    let before = Arc::new(before);
    w.listen(event::LINKS_FLUSH_REQUESTED, move |event| {
        let payload: Value = serde_json::from_str(event.payload()).expect("данные");
        let window = window.clone();
        let before = Arc::clone(&before);
        // Слушатель вызывается в потоке команды — отвечаем из другого.
        std::thread::spawn(move || {
            before(&window);
            call(
                &window,
                "links_flush_done",
                json!({ "token": payload["token"], "saved": saved }),
            )
            .expect("links_flush_done");
        });
    });
}

/// Окно сразу отвечает «ввод записан».
fn flush_ok(w: &WebviewWindow<MockRuntime>) {
    answer_flush(w, true, |_| {});
}

fn try_preview(w: &WebviewWindow<MockRuntime>, request: Value) -> IndexReply<RelinkPreview> {
    typed(
        "relink_preview",
        call(w, "relink_preview", request).expect("relink_preview"),
    )
}

fn preview(w: &WebviewWindow<MockRuntime>, request: Value) -> RelinkPreview {
    match try_preview(w, request) {
        IndexReply::Ready(preview) => preview,
        IndexReply::IndexBuilding(progress) => panic!("индекс строится: {progress:?}"),
    }
}

fn preview_rename(w: &WebviewWindow<MockRuntime>, path: &[&str], new_name: &str) -> RelinkPreview {
    preview(
        w,
        json!({ "op": "rename", "path": path, "new_name": new_name }),
    )
}

fn rename_update(
    w: &WebviewWindow<MockRuntime>,
    path: &[&str],
    new_name: &str,
    token: &RelinkToken,
) -> Result<Value, Value> {
    call(
        w,
        "entry_rename",
        json!({ "path": path, "new_name": new_name, "links": "update", "token": token }),
    )
}

fn moved(command: &str, value: Value) -> EntryMovedResponse {
    typed(command, value)
}

fn read(root: &Path, rel: &str) -> String {
    let path = rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn named<'a>(events: &'a [(String, Value)], name: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|(event, _)| event == name)
        .map(|(_, payload)| payload)
        .collect()
}

fn assert_token_rejected(command: &str, result: Result<Value, Value>) {
    let error = core_error(command, result.expect_err("знак не принят"));
    assert_eq!(error.code, ErrorCode::InvalidArgument, "{command}");
    assert_eq!(error.field.as_deref(), Some("token"), "{command}");
}

const RELINK_EVENTS: [&str; 4] = [
    event::LINKS_UPDATED,
    event::NOTE_CHANGED,
    event::LINKS_FLUSH_REQUESTED,
    event::LINKS_PROGRESS,
];

fn meeting_vault(
    app: &App<MockRuntime>,
    label: &str,
) -> (WebviewWindow<MockRuntime>, tempfile::TempDir, PathBuf) {
    indexed_vault(
        app,
        label,
        &[
            ("Встреча.md", "# Встреча\n"),
            ("Список.md", "до [[Встреча]] после\n"),
            ("Другая.md", "[[Встреча|встречи]] и [[Встреча#Итоги]]\r\n"),
            ("Чужая.md", "[[Другое]] и `[[Встреча]]`\n"),
        ],
    )
}

/// Переименование с `links: update`: ссылки на заметку переписаны (только
/// цель, псевдоним, заголовок и CRLF на месте), ответ с `relink`, окну —
/// `note:changed` по каждой переписанной заметке и одно `links:updated` с
/// именами «старое → новое».
#[test]
fn rename_with_update_rewrites_backlinks() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-update");
    let seen = record_events(&w, &RELINK_EVENTS);
    flush_ok(&w);

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_eq!((plan.notes, plan.links), (2, 3));
    let response = moved(
        "entry_rename",
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование"),
    );

    assert_eq!(response.path, VaultPath::from_components(["Планёрка.md"]));
    assert_eq!(
        response.relink,
        Some(RelinkOutcome {
            updated: 2,
            failed: vec![],
            retry_token: None
        })
    );
    assert_eq!(read(&root, "Список.md"), "до [[Планёрка]] после\n");
    assert_eq!(
        read(&root, "Другая.md"),
        "[[Планёрка|встречи]] и [[Планёрка#Итоги]]\r\n"
    );
    assert_eq!(read(&root, "Чужая.md"), "[[Другое]] и `[[Встреча]]`\n");
    assert!(!root.join("Встреча.md").exists());
    let events = seen.lock().expect("журнал").clone();
    let mut changed: Vec<String> = named(&events, event::NOTE_CHANGED)
        .iter()
        .map(|payload| payload["path"][0].as_str().expect("путь").to_owned())
        .collect();
    changed.sort();
    assert_eq!(changed, ["Другая.md", "Список.md"]);
    let updated = named(&events, event::LINKS_UPDATED);
    assert_eq!(updated.len(), 1, "{events:?}");
    let payload: LinksUpdatedPayload = typed(event::LINKS_UPDATED, updated[0].clone());
    assert_eq!(payload.reason, LinksUpdateReason::Rename);
    assert_eq!(payload.updated, 2);
    assert_eq!(
        payload.renames,
        [PathRename {
            from: VaultPath::from_components(["Встреча.md"]),
            to: VaultPath::from_components(["Планёрка.md"]),
        }]
    );
    // Индекс перечитал переписанные заметки.
    assert_eq!(backlinks(&w, &["Планёрка.md"]).notes_total, 2);
}

/// Уведомление — только по `links:updated`: на одно переименование папки
/// со многими переписанными заметками — ровно одно событие, а ход
/// (`links:progress`) — не чаще 4 раз в секунду.
#[test]
fn one_links_updated_per_rename() {
    let app = app();
    let mut files: Vec<(String, String)> = Vec::new();
    for i in 0..30 {
        files.push((format!("Альфа/З{i}.md"), String::new()));
        files.push((format!("Ссылки/С{i}.md"), format!("[[Альфа/З{i}]]\n")));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (w, _dir, root) = indexed_vault(&app, "relink-one", &refs);
    let seen = record_events(&w, &RELINK_EVENTS);
    flush_ok(&w);

    let plan = preview_rename(&w, &["Альфа"], "Бета");
    assert_eq!(plan.notes, 30);
    let started = std::time::Instant::now();
    let response = moved(
        "entry_rename",
        rename_update(&w, &["Альфа"], "Бета", &plan.token).expect("переименование"),
    );
    let elapsed = started.elapsed();
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(response.relink.expect("relink").updated, 30);
    // Имя уникальное — новая цель, как у автодополнения, без пути (Р3).
    assert_eq!(read(&root, "Ссылки/С7.md"), "[[З7]]\n");
    let events = seen.lock().expect("журнал").clone();
    assert_eq!(named(&events, event::LINKS_UPDATED).len(), 1);
    let progress = named(&events, event::LINKS_PROGRESS).len();
    let allowed = 1 + 4 * elapsed.as_secs() as usize + 4;
    assert!(
        progress <= allowed,
        "{progress} событий хода за {elapsed:?}"
    );
}

/// `links: keep` — как в 0.1: ни записи ввода, ни переписывания, ни
/// событий обновления ссылок.
#[test]
fn rename_keep_rewrites_nothing() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-keep");
    let seen = record_events(&w, &RELINK_EVENTS);

    assert_eq!(preview_rename(&w, &["Встреча.md"], "Планёрка.md").notes, 2);
    let response = call(
        &w,
        "entry_rename",
        json!({ "path": ["Встреча.md"], "new_name": "Планёрка.md" }),
    )
    .expect("переименование");

    assert_eq!(response, json!({ "path": ["Планёрка.md"] }));
    assert_eq!(read(&root, "Список.md"), "до [[Встреча]] после\n");
    std::thread::sleep(Duration::from_millis(200));
    assert!(seen.lock().expect("журнал").is_empty());
}

/// Переименование не удалось (имя занято) — ни один файл не переписан.
#[test]
fn rename_failed_rewrites_nothing() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-failed");
    put(&root, "Планёрка.md/вложение.txt", b"x");
    let seen = record_events(&w, &RELINK_EVENTS);
    flush_ok(&w);

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_eq!(plan.notes, 2, "заметок с именем нет — план есть");
    let error = rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token)
        .expect_err("имя занято папкой");

    assert_ne!(
        core_error("entry_rename", error).code,
        ErrorCode::InvalidArgument
    );
    assert!(root.join("Встреча.md").is_file());
    assert_eq!(read(&root, "Список.md"), "до [[Встреча]] после\n");
    assert_eq!(
        read(&root, "Другая.md"),
        "[[Встреча|встречи]] и [[Встреча#Итоги]]\r\n"
    );
    std::thread::sleep(Duration::from_millis(200));
    let events = seen.lock().expect("журнал").clone();
    assert!(named(&events, event::LINKS_UPDATED).is_empty());
    assert!(named(&events, event::NOTE_CHANGED).is_empty());
}

/// Ввод окна записан **до** переименования и переписывания (инвариант 4):
/// на запрос записи файл ещё не переименован; записанный ввод с новой
/// ссылкой тоже переписан — план считается после записи.
#[test]
fn flush_waited_before_rewrite() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-flush");
    let renamed_early = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let early = Arc::clone(&renamed_early);
    let vault_root = root.clone();
    answer_flush(&w, true, move |window| {
        early.store(
            !vault_root.join("Встреча.md").exists(),
            std::sync::atomic::Ordering::SeqCst,
        );
        // Окно записывает несохранённый ввод своей заметки.
        let read: NoteReadResponse = typed(
            "note_read",
            call(window, "note_read", json!({ "path": ["Список.md"] })).expect("note_read"),
        );
        call(
            window,
            "note_write",
            json!({
                "path": ["Список.md"],
                "text": "до [[Встреча]] после\nввод [[Встреча]]\n",
                "expected_version": read.version,
            }),
        )
        .expect("note_write");
    });

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование");

    assert!(
        !renamed_early.load(std::sync::atomic::Ordering::SeqCst),
        "переименование ждёт записи ввода"
    );
    assert_eq!(
        read(&root, "Список.md"),
        "до [[Планёрка]] после\nввод [[Планёрка]]\n"
    );
}

/// Сбой записи ввода — ошибка `io_failed`, на диске ничего не меняется.
fn assert_flush_failed(result: Result<Value, Value>, root: &Path) {
    let error = core_error("entry_rename", result.expect_err("ввод не записан"));
    assert_eq!(error.code, ErrorCode::IoFailed);
    assert!(root.join("Встреча.md").is_file(), "не переименована");
    assert!(!root.join("Планёрка.md").exists());
    assert_eq!(read(root, "Список.md"), "до [[Встреча]] после\n");
    assert_eq!(
        read(root, "Другая.md"),
        "[[Встреча|встречи]] и [[Встреча#Итоги]]\r\n"
    );
}

/// Окно не ответило на запрос записи ввода за предел — переименование не
/// выполняется (С2-5.4: запись ввода раньше переименования, решение 114),
/// ошибка `io_failed`, ни один файл не тронут, `links:updated` нет; поздний
/// ответ не принимается; знак плана остаётся действующим — попытка
/// повторяется с тем же знаком.
#[test]
fn flush_timeout_cancels_rewrite() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-timeout");
    let open = app
        .state::<Core>()
        .window_vault("relink-timeout")
        .expect("хранилище");
    open.relink.set_flush_timeout(Duration::from_millis(200));
    let seen = record_events(&w, &RELINK_EVENTS);

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_flush_failed(
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token),
        &root,
    );

    let events = seen.lock().expect("журнал").clone();
    assert!(named(&events, event::LINKS_UPDATED).is_empty());
    let flush = named(&events, event::LINKS_FLUSH_REQUESTED);
    assert_eq!(flush.len(), 1);
    assert_token_rejected(
        "links_flush_done",
        call(
            &w,
            "links_flush_done",
            json!({ "token": flush[0]["token"], "saved": true }),
        ),
    );

    // Тот же знак — вторая попытка, окно отвечает: ссылки обновлены.
    open.relink.set_flush_timeout(Duration::from_secs(10));
    flush_ok(&w);
    rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("вторая попытка");
    assert_eq!(read(&root, "Список.md"), "до [[Планёрка]] после\n");
}

/// Окно ответило «ввод не записан» — так же: ошибка, ничего не изменилось.
#[test]
fn flush_refused_cancels_rename() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-refused");
    let seen = record_events(&w, &RELINK_EVENTS);
    answer_flush(&w, false, |_| {});

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_flush_failed(
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token),
        &root,
    );
    std::thread::sleep(Duration::from_millis(200));
    let events = seen.lock().expect("журнал").clone();
    assert!(named(&events, event::LINKS_UPDATED).is_empty());
    assert!(named(&events, event::NOTE_CHANGED).is_empty());
}

/// «Повторить», а ввод не записан — ни одна заметка не переписана, но знак
/// не теряется молча: `links:updated` (`retry`) со сбоем `busy` у заметок
/// повтора и новым `retry_token`, по которому повтор проходит.
#[test]
fn retry_flush_failure_reported_with_new_token() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-retry-flush");
    let busy = root.join("Список.md");
    let set_readonly = |readonly: bool| {
        let mut permissions = std::fs::metadata(&busy).expect("мета").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(readonly);
        std::fs::set_permissions(&busy, permissions).expect("права");
    };
    set_readonly(true);
    let seen = record_events(&w, &[event::LINKS_UPDATED]);
    let saved = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let answer = Arc::clone(&saved);
    let window = w.clone();
    w.listen(event::LINKS_FLUSH_REQUESTED, move |event| {
        let payload: Value = serde_json::from_str(event.payload()).expect("данные");
        let window = window.clone();
        let saved = answer.load(std::sync::atomic::Ordering::SeqCst);
        std::thread::spawn(move || {
            call(
                &window,
                "links_flush_done",
                json!({ "token": payload["token"], "saved": saved }),
            )
            .expect("links_flush_done");
        });
    });
    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    let relink = moved(
        "entry_rename",
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование"),
    )
    .relink
    .expect("relink");
    set_readonly(false);
    let retry = relink.retry_token.expect("знак повтора");

    saved.store(false, std::sync::atomic::Ordering::SeqCst);
    call(
        &w,
        "relink_answer",
        json!({ "token": retry, "apply": true }),
    )
    .expect("повтор");

    assert_eq!(read(&root, "Список.md"), "до [[Встреча]] после\n");
    assert!(wait_for(&seen, |events| events.len() == 2));
    let events = seen.lock().expect("журнал").clone();
    let failed: LinksUpdatedPayload = typed(event::LINKS_UPDATED, events[1].1.clone());
    assert_eq!(failed.reason, LinksUpdateReason::Retry);
    assert_eq!(failed.updated, 0);
    assert_eq!(
        failed.failed,
        [RelinkFailure {
            path: VaultPath::from_components(["Список.md"]),
            reason: RelinkFailureReason::Busy,
        }]
    );
    let again = failed.retry_token.expect("новый знак повтора");
    assert_ne!(again, retry);
    assert_eq!(failed.renames, relink_renames());

    saved.store(true, std::sync::atomic::Ordering::SeqCst);
    call(
        &w,
        "relink_answer",
        json!({ "token": again, "apply": true }),
    )
    .expect("повтор");
    assert_eq!(read(&root, "Список.md"), "до [[Планёрка]] после\n");
}

fn relink_renames() -> Vec<PathRename> {
    vec![PathRename {
        from: VaultPath::from_components(["Встреча.md"]),
        to: VaultPath::from_components(["Планёрка.md"]),
    }]
}

/// Очередь планов — общая для хранилища (по пути корня), а не для окна:
/// окно закрыли и сразу открыли снова — доделка в новом окне ждёт
/// выполнение в старом. Переименование с `links: keep` тоже стоит в очереди.
#[test]
fn relink_queue_shared_per_vault() {
    let app = app();
    let dir = tempfile::tempdir().expect("папка");
    put(dir.path(), "Встреча.md", b"");
    let other = tempfile::tempdir().expect("папка");
    let open = |root: &Path, label: &str| {
        OpenVault::new(summary(label, root), Vault::open(root).expect("хранилище"))
            .expect("заметки")
    };
    let old = open(dir.path(), "queue-old");
    let new = open(dir.path(), "queue-new");
    let foreign = open(other.path(), "queue-foreign");

    let guard = old.relink.lock_queue();
    assert!(new.relink.queue_busy(), "то же хранилище — та же очередь");
    assert!(
        !foreign.relink.queue_busy(),
        "другое хранилище — своя очередь"
    );
    drop(guard);
    assert!(!new.relink.queue_busy());

    // `links: keep` ждёт очереди.
    let w = window(&app, "queue-keep");
    app.state::<Core>()
        .bind_window("queue-keep", open(dir.path(), "queue-keep"));
    let bound = app
        .state::<Core>()
        .window_vault("queue-keep")
        .expect("хранилище");
    let guard = old.relink.lock_queue();
    let renamer = {
        let w = w.clone();
        std::thread::spawn(move || {
            call(
                &w,
                "entry_rename",
                json!({ "path": ["Встреча.md"], "new_name": "Планёрка.md" }),
            )
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(dir.path().join("Встреча.md").is_file(), "ждёт очереди");
    drop(guard);
    renamer.join().expect("поток").expect("переименование");
    assert!(dir.path().join("Планёрка.md").is_file());
    drop(bound);
}

/// Знак одноразовый: второе предъявление — `invalid_argument` до любого
/// действия с файлами.
#[test]
fn token_single_use() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-once");
    flush_ok(&w);
    let plan = preview_rename(&w, &["Список.md"], "Перечень.md");
    rename_update(&w, &["Список.md"], "Перечень.md", &plan.token).expect("первое");
    put(&root, "Список.md", b"");

    assert_token_rejected(
        "entry_rename",
        rename_update(&w, &["Список.md"], "Перечень.md", &plan.token),
    );
    assert!(root.join("Список.md").is_file(), "файл не тронут");
}

/// Знак привязан к окну: окно другого хранилища его не принимает.
#[test]
fn token_other_window_rejected() {
    let app = app();
    let (a, _dir_a, _root_a) = meeting_vault(&app, "relink-win-a");
    let (b, _dir_b, root_b) = meeting_vault(&app, "relink-win-b");
    flush_ok(&b);
    let plan = preview_rename(&a, &["Встреча.md"], "Планёрка.md");

    assert_token_rejected(
        "entry_rename",
        rename_update(&b, &["Встреча.md"], "Планёрка.md", &plan.token),
    );
    assert!(root_b.join("Встреча.md").is_file());
    assert_eq!(read(&root_b, "Список.md"), "до [[Встреча]] после\n");
}

/// Знак живёт 600 с: просроченный не принимается.
#[test]
fn token_expired_rejected() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-expired");
    let open = app
        .state::<Core>()
        .window_vault("relink-expired")
        .expect("хранилище");
    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    open.relink.advance_tokens(Duration::from_secs(600));

    assert_token_rejected(
        "entry_rename",
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token),
    );
    assert!(root.join("Встреча.md").is_file());
}

/// Знак выдан для одного переименования — для другого (другое имя, другая
/// папка, знак повтора вместо предпросмотра) не годится: интерфейс
/// выполняет только план ядра.
#[test]
fn token_other_plan_rejected() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-other");
    std::fs::create_dir_all(root.join("Архив")).expect("папка");
    let cases: [(&str, Value, Value); 3] = [
        (
            "entry_rename",
            json!({ "op": "rename", "path": ["Встреча.md"], "new_name": "Планёрка.md" }),
            json!({ "path": ["Встреча.md"], "new_name": "Итоги.md", "links": "update" }),
        ),
        (
            "entry_move",
            json!({ "op": "rename", "path": ["Встреча.md"], "new_name": "Планёрка.md" }),
            json!({ "path": ["Встреча.md"], "new_dir": ["Архив"], "links": "update" }),
        ),
        (
            "entry_rename",
            json!({ "op": "move", "path": ["Встреча.md"], "new_dir": ["Архив"] }),
            json!({ "path": ["Список.md"], "new_name": "Встреча.md", "links": "update" }),
        ),
    ];
    for (command, op, mut request) in cases {
        let plan = preview(&w, op);
        request["token"] = json!(plan.token);
        assert_token_rejected(command, call(&w, command, request));
    }
    // Знак предпросмотра — не ответ на вопрос.
    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_token_rejected(
        "relink_answer",
        call(
            &w,
            "relink_answer",
            json!({ "token": plan.token, "apply": true }),
        ),
    );
    assert!(root.join("Встреча.md").is_file());
    assert_eq!(read(&root, "Список.md"), "до [[Встреча]] после\n");
}

/// Пока индекс строится — `index_building` с ходом, знака нет; когда готов
/// и сверен — предпросмотр с числами.
#[test]
fn preview_while_index_building() {
    let app = app();
    let w = window(&app, "relink-building");
    let dir = tempfile::tempdir().expect("папка");
    put(dir.path(), "Встреча.md", b"");
    put(dir.path(), "Список.md", "[[Встреча]]".as_bytes());
    let core = app.state::<Core>();
    core.bind_window(
        "relink-building",
        OpenVault::new(
            summary("relink-building", dir.path()),
            Vault::open(dir.path()).expect("хранилище"),
        )
        .expect("заметки"),
    );
    let request = json!({ "op": "rename", "path": ["Встреча.md"], "new_name": "Планёрка.md" });
    assert!(matches!(
        try_preview(&w, request.clone()),
        IndexReply::IndexBuilding(_)
    ));

    let open = core.window_vault("relink-building").expect("хранилище");
    start_index(
        &open,
        Arc::new(WindowEmitter::new(app.handle().clone(), "relink-building")),
    );
    open.index.wait_settled();
    let plan = preview(&w, request);
    assert_eq!((plan.notes, plan.links), (1, 1));
    assert!(plan.token.is_well_formed());
}

/// Индекс загружен из файла, но ещё не сверен с диском — планов нет:
/// `index_building`, хотя `index_status` уже `ready` (ревью 0.2-1.4).
#[test]
fn preview_waits_for_reconcile() {
    let app = app();
    let w = window(&app, "relink-reconcile");
    let dir = tempfile::tempdir().expect("папка");
    put(dir.path(), "Встреча.md", b"");
    let core = app.state::<Core>();
    core.bind_window(
        "relink-reconcile",
        OpenVault::new(
            summary("relink-reconcile", dir.path()),
            Vault::open(dir.path()).expect("хранилище"),
        )
        .expect("заметки"),
    );
    let open = core.window_vault("relink-reconcile").expect("хранилище");
    open.index
        .load_unreconciled(crate::index::VaultIndex::from_entries([]));

    assert_eq!(open.index.status().state, IndexState::Ready);
    assert!(matches!(
        try_preview(
            &w,
            json!({ "op": "rename", "path": ["Встреча.md"], "new_name": "Планёрка.md" })
        ),
        IndexReply::IndexBuilding(_)
    ));
}

/// Решение 114: пока индекс строится, переименование с обновлением ссылок
/// невозможно (знака нет, выдуманный не принимается, файл не тронут); по
/// готовности окну приходит `index:status`, предпросмотр даёт знак, и
/// переименование обновляет ссылки.
#[test]
fn rename_waits_for_index_in_update_mode() {
    let app = app();
    let w = window(&app, "relink-wait");
    let seen = record_events(&w, &[event::INDEX_STATUS]);
    let dir = tempfile::tempdir().expect("папка");
    put(dir.path(), "Встреча.md", b"");
    put(dir.path(), "Список.md", "[[Встреча]]".as_bytes());
    let core = app.state::<Core>();
    core.bind_window(
        "relink-wait",
        OpenVault::new(
            summary("relink-wait", dir.path()),
            Vault::open(dir.path()).expect("хранилище"),
        )
        .expect("заметки"),
    );
    let request = json!({ "op": "rename", "path": ["Встреча.md"], "new_name": "Планёрка.md" });
    assert!(matches!(
        try_preview(&w, request.clone()),
        IndexReply::IndexBuilding(_)
    ));
    assert_token_rejected(
        "entry_rename",
        rename_update(
            &w,
            &["Встреча.md"],
            "Планёрка.md",
            &RelinkToken("0123456789abcdef0123456789abcdef".into()),
        ),
    );
    assert!(dir.path().join("Встреча.md").is_file());

    let open = core.window_vault("relink-wait").expect("хранилище");
    start_index(
        &open,
        Arc::new(WindowEmitter::new(app.handle().clone(), "relink-wait")),
    );
    assert!(
        wait_for(&seen, |events| events
            .iter()
            .any(|(_, payload)| payload["state"] == json!("ready"))),
        "index:status ready"
    );
    open.index.wait_settled();
    flush_ok(&w);
    let plan = preview(&w, request);
    rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование");
    assert_eq!(read(dir.path(), "Список.md"), "[[Планёрка]]");
}

/// «Повторить» (решение 115): знак повтора выдан, если есть `busy`/`io`;
/// повтор переписывает только их, `too_large` не трогает; знак повтора
/// одноразовый.
#[test]
fn retry_token_reruns_busy_only() {
    let app = app();
    let mut big = "[[Встреча]]\n".to_owned();
    big.push_str(&"x".repeat(1024 * 1024 + 10));
    let (w, _dir, root) = indexed_vault(
        &app,
        "relink-retry",
        &[
            ("Встреча.md", ""),
            ("Список.md", "[[Встреча]]\n"),
            ("Занятая.md", "[[Встреча]]\n"),
            ("Большая.md", &big),
        ],
    );
    let open = app
        .state::<Core>()
        .window_vault("relink-retry")
        .expect("хранилище");
    open.vault
        .write_settings(&Settings {
            max_edit_size_bytes: 1024 * 1024,
            ..Settings::default()
        })
        .expect("настройки");
    let busy = root.join("Занятая.md");
    let set_readonly = |readonly: bool| {
        let mut permissions = std::fs::metadata(&busy).expect("мета").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(readonly);
        std::fs::set_permissions(&busy, permissions).expect("права");
    };
    set_readonly(true);
    let seen = record_events(&w, &[event::LINKS_UPDATED]);
    flush_ok(&w);

    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_eq!(plan.notes, 3);
    let relink = moved(
        "entry_rename",
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование"),
    )
    .relink
    .expect("relink");
    set_readonly(false);

    assert_eq!(relink.updated, 1);
    let mut reasons: Vec<_> = relink
        .failed
        .iter()
        .map(|f| (f.path.components()[0].clone(), f.reason))
        .collect();
    reasons.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        reasons,
        [
            ("Большая.md".to_owned(), RelinkFailureReason::TooLarge),
            ("Занятая.md".to_owned(), RelinkFailureReason::Busy),
        ]
    );
    let retry = relink.retry_token.expect("знак повтора");
    assert_eq!(read(&root, "Занятая.md"), "[[Встреча]]\n");

    call(
        &w,
        "relink_answer",
        json!({ "token": retry, "apply": true }),
    )
    .expect("повтор");

    assert_eq!(read(&root, "Занятая.md"), "[[Планёрка]]\n");
    assert!(read(&root, "Большая.md").starts_with("[[Встреча]]\n"));
    assert!(wait_for(&seen, |events| events.len() == 2));
    let events = seen.lock().expect("журнал").clone();
    let first: LinksUpdatedPayload = typed(event::LINKS_UPDATED, events[0].1.clone());
    assert_eq!(first.retry_token.as_ref(), Some(&retry));
    let again: LinksUpdatedPayload = typed(event::LINKS_UPDATED, events[1].1.clone());
    assert_eq!(again.reason, LinksUpdateReason::Retry);
    assert_eq!((again.updated, again.failed.len()), (1, 0));
    assert_eq!(again.retry_token, None);
    assert_eq!(again.renames, first.renames);
    assert_token_rejected(
        "relink_answer",
        call(
            &w,
            "relink_answer",
            json!({ "token": retry, "apply": true }),
        ),
    );
}

/// Перенос заметки с уникальным именем не меняет ни одного файла (Р2):
/// предпросмотр — 0 заметок, после переноса с `update` ни один файл не
/// переписан (дата изменения прежняя), событий обновления нет.
#[test]
fn move_unique_note_changes_no_file() {
    let app = app();
    let (w, _dir, root) = indexed_vault(
        &app,
        "relink-unique",
        &[
            ("Альфа/Уникальная.md", ""),
            ("Бета/Другое.md", ""),
            ("Список.md", "[[Уникальная]]\n"),
            ("Другая.md", "[[Уникальная|у]] и [[Уникальная#Итоги]]\n"),
        ],
    );
    let mtimes = || -> Vec<std::time::SystemTime> {
        ["Список.md", "Другая.md"]
            .iter()
            .map(|n| {
                std::fs::metadata(root.join(n))
                    .expect("мета")
                    .modified()
                    .expect("дата")
            })
            .collect()
    };
    let before = mtimes();
    let seen = record_events(&w, &RELINK_EVENTS);
    flush_ok(&w);

    let plan = preview(
        &w,
        json!({ "op": "move", "path": ["Альфа", "Уникальная.md"], "new_dir": ["Бета"] }),
    );
    assert_eq!((plan.notes, plan.links), (0, 0));
    let response = moved(
        "entry_move",
        call(
            &w,
            "entry_move",
            json!({ "path": ["Альфа", "Уникальная.md"], "new_dir": ["Бета"], "links": "update", "token": plan.token }),
        )
        .expect("перенос"),
    );

    assert_eq!(
        response.path,
        VaultPath::from_components(["Бета", "Уникальная.md"])
    );
    assert_eq!(response.relink, Some(RelinkOutcome::default()));
    assert_eq!(mtimes(), before, "ни один файл не переписан");
    assert_eq!(read(&root, "Список.md"), "[[Уникальная]]\n");
    std::thread::sleep(Duration::from_millis(200));
    let events = seen.lock().expect("журнал").clone();
    assert!(named(&events, event::LINKS_UPDATED).is_empty());
    assert!(named(&events, event::NOTE_CHANGED).is_empty());
}

/// Ждёт, пока у заметки `path` станет `count` обратных ссылок (внешняя
/// правка дошла до индекса через наблюдателя).
fn wait_index_settles_on(w: &WebviewWindow<MockRuntime>, path: &[&str], count: u32) {
    let start = std::time::Instant::now();
    while backlinks(w, path).notes_total != count {
        assert!(start.elapsed() < WAIT, "обратных ссылок не {count}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Состав заметок изменился после предпросмотра — план считается заново:
/// новая заметка со ссылкой тоже переписана.
#[test]
fn stale_preview_replanned() {
    let app = app();
    let (w, _dir, root) = meeting_vault(&app, "relink-stale");
    flush_ok(&w);
    let plan = preview_rename(&w, &["Встреча.md"], "Планёрка.md");
    assert_eq!(plan.notes, 2);

    put(&root, "Новая.md", "и [[Встреча]]\n".as_bytes());
    wait_index_settles_on(&w, &["Встреча.md"], 3);
    let relink = moved(
        "entry_rename",
        rename_update(&w, &["Встреча.md"], "Планёрка.md", &plan.token).expect("переименование"),
    )
    .relink
    .expect("relink");

    assert_eq!(relink.updated, 3);
    assert_eq!(read(&root, "Новая.md"), "и [[Планёрка]]\n");
}

/// Обновление ссылок, прерванное падением, доделывается при открытии
/// хранилища — после сверки индекса и до того, как предпросмотр даёт знак:
/// все ссылки верны, журнала нет, окну — `links:updated` (`resumed`).
#[test]
fn resume_on_open_after_reconcile() {
    let app = app();
    let w = window(&app, "relink-resume");
    let dir = tempfile::tempdir().expect("папка");
    let root = dir.path().join("хранилище");
    for (rel, text) in [
        ("Встреча.md", ""),
        ("А.md", "[[Встреча]]\n"),
        ("Б.md", "[[Встреча]]\n"),
        ("В.md", "[[Встреча]]\n"),
    ] {
        put(&root, rel, text.as_bytes());
    }
    {
        // Прошлый запуск: переименование выполнено, после первой заметки —
        // падение.
        let vault = Vault::open(&root).expect("хранилище");
        let index = RwLock::new(crate::index::VaultIndex::new());
        crate::index::rebuild(&vault, &index, &|_| {}).expect("индекс");
        let before = crate::links::Links::new(vault.notes().expect("заметки"));
        let renames = [PathRename {
            from: VaultPath::from_components(["Встреча.md"]),
            to: VaultPath::from_components(["Планёрка.md"]),
        }];
        let plan = crate::relink::plan(&index.read().expect("индекс"), &before, &renames, &[]);
        assert_eq!(plan.note_count(), 3);
        vault
            .rename(&renames[0].from, "Планёрка.md")
            .expect("переименование");
        let after = crate::links::Links::new(vault.notes().expect("заметки"));
        crate::relink::crash_after(1);
        crate::relink::run(&vault, &after.view(), &plan, &renames, &mut |_, _| {});
    }
    let journal = root.join(".rhizomind").join("link-update.json");
    assert!(journal.is_file(), "журнал прерванного плана");
    let seen = record_events(&w, &[event::LINKS_UPDATED, event::NOTE_CHANGED]);

    open_vault(
        &app,
        "relink-resume",
        "relink-resume",
        &root,
        trash_into(dir.path().join("bin"), true),
    );

    for note in ["А.md", "Б.md", "В.md"] {
        assert_eq!(read(&root, note), "[[Планёрка]]\n", "{note}");
    }
    assert!(!journal.exists(), "журнал удалён");
    assert!(wait_for(&seen, |events| events
        .iter()
        .any(|(name, _)| name == event::LINKS_UPDATED)));
    let events = seen.lock().expect("журнал").clone();
    let updated = named(&events, event::LINKS_UPDATED);
    assert_eq!(updated.len(), 1);
    let payload: LinksUpdatedPayload = typed(event::LINKS_UPDATED, updated[0].clone());
    assert_eq!(payload.reason, LinksUpdateReason::Resumed);
    // Заметка, записанная до падения без отметки «готова», тоже в итоге.
    assert_eq!(payload.updated, 3);
    assert_eq!(named(&events, event::NOTE_CHANGED).len(), 3);
    assert_eq!(backlinks(&w, &["Планёрка.md"]).notes_total, 3);
}

/// Замер (справочно, план: QA 0.2-4.6): переименование «узла» со 100
/// ссылающимися заметками — от ответа пользователя до `links:updated`,
/// цель ≤ 2 с. Только по явному запросу (`RHIZOMIND_RENAME_HUB=1`): в общем
/// прогоне `--ignored` (CI, шаг «Производительность») запись 100 заметок
/// мешала бы замерам скорости, идущим параллельно (`index_build_10k`).
#[test]
#[ignore = "замер: RHIZOMIND_RENAME_HUB=1 cargo test --lib rename_hub_100 -- --ignored --nocapture"]
fn rename_hub_100() {
    if std::env::var_os("RHIZOMIND_RENAME_HUB").is_none() {
        println!("rename_hub_100: пропущен (нужен RHIZOMIND_RENAME_HUB=1)");
        return;
    }
    let app = app();
    let mut files: Vec<(String, String)> = vec![("Узел.md".into(), "# Узел\n".into())];
    for i in 0..100 {
        files.push((
            format!("Папка {}/Заметка {i}.md", i % 10),
            format!(
                "# {i}\nСм. [[Узел]], [[Узел#Узел|узел]] и текст {}\n",
                "слово ".repeat(50)
            ),
        ));
    }
    for i in 0..900 {
        files.push((format!("Прочее/П{i}.md"), format!("[[П{}]]\n", i + 1)));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (w, _dir, root) = indexed_vault(&app, "relink-hub", &refs);
    let seen = record_events(&w, &[event::LINKS_UPDATED]);
    flush_ok(&w);
    let plan = preview_rename(&w, &["Узел.md"], "Центр.md");
    assert_eq!(plan.notes, 100);

    let started = std::time::Instant::now();
    rename_update(&w, &["Узел.md"], "Центр.md", &plan.token).expect("переименование");
    assert!(wait_for(&seen, |events| !events.is_empty()));
    let elapsed = started.elapsed();

    println!("rename_hub_100: от ответа до links:updated — {elapsed:?} (цель ≤ 2 с, справочно)");
    assert!(read(&root, "Папка 3/Заметка 13.md").contains("[[Центр]], [[Центр#Узел|узел]]"));
}
