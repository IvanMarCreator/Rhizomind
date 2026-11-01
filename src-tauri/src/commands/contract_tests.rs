//! Снимки контракта API v1: пример запроса, ответа и ошибок каждой команды и
//! данных каждого события — в `contracts/v1/*.snap`.
//!
//! Те же файлы читает Vitest (`src/api/contract.test.ts`) и сравнивает с
//! примерами, записанными в типах TypeScript. Поэтому расхождение типов
//! Rust и TypeScript валит один из тестов: изменили тип в Rust — меняется
//! снимок (insta), и тест TypeScript перестаёт совпадать; изменили тип в
//! TypeScript — пример не проходит проверку типов или не совпадает со снимком.
//!
//! Каждый пример ещё и проходит круг «JSON → тип Rust → JSON» без потерь, а
//! запрос с лишним полем отклоняется (`deny_unknown_fields`).

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;

const SNAPSHOT_DIR: &str = "../../../contracts/v1";

fn settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(SNAPSHOT_DIR);
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings
}

fn round_trip<T>(value: &T) -> Value
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_value(value).expect("сериализация");
    let back: T = serde_json::from_value(json.clone()).expect("обратное чтение");
    assert_eq!(&back, value, "круг JSON → Rust → JSON теряет данные");
    json
}

/// Запрос с лишним полем отклоняется.
fn assert_rejects_unknown_field<T: DeserializeOwned>(request: &Value) {
    let mut extra = request.clone();
    if let Value::Object(map) = &mut extra {
        map.insert("unexpected".into(), json!(1));
        assert!(
            serde_json::from_value::<T>(extra).is_err(),
            "лишнее поле в запросе должно отклоняться"
        );
    }
}

/// Команда без параметров.
fn command<Res>(name: &str, response: &Res, errors: &[CoreError])
where
    Res: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    snapshot_command(name, Value::Null, round_trip(response), errors);
}

/// Команда с параметрами: интерфейс передаёт `{ request: <пример> }`.
fn command_with<Req, Res>(name: &str, request: &Req, response: &Res, errors: &[CoreError])
where
    Req: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    Res: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let request = round_trip(request);
    assert_rejects_unknown_field::<Req>(&request);
    snapshot_command(name, request, round_trip(response), errors);
}

/// Второй пример той же команды — отдельный снимок `<команда>.<вариант>.snap`
/// (например, `entry_rename.update`: основной снимок остаётся примером 0.1).
fn command_variant<Req, Res>(
    name: &str,
    variant: &str,
    request: &Req,
    response: &Res,
    errors: &[CoreError],
) where
    Req: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    Res: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert!(COMMAND_NAMES.contains(&name), "{name} нет в списке команд");
    assert!(
        COMMAND_VARIANTS.contains(&(name, variant)),
        "{name}.{variant} нет в COMMAND_VARIANTS"
    );
    let request = round_trip(request);
    assert_rejects_unknown_field::<Req>(&request);
    let errors: Vec<Value> = errors.iter().map(round_trip).collect();
    let contract = json!({
        "command": name,
        "variant": variant,
        "request": request,
        "response": round_trip(response),
        "errors": errors,
    });
    settings().bind(|| insta::assert_json_snapshot!(format!("{name}.{variant}"), contract));
}

/// Вторые примеры команд (`<команда>.<вариант>.snap`).
const COMMAND_VARIANTS: [(&str, &str); 3] = [
    ("entry_rename", "update"),
    ("entry_move", "update"),
    ("relink_preview", "index_building"),
];

fn snapshot_command(name: &str, request: Value, response: Value, errors: &[CoreError]) {
    assert!(COMMAND_NAMES.contains(&name), "{name} нет в списке команд");
    let errors: Vec<Value> = errors.iter().map(round_trip).collect();
    let contract = json!({
        "command": name,
        "request": request,
        "response": response,
        "errors": errors,
    });
    settings().bind(|| insta::assert_json_snapshot!(name, contract));
}

fn event<P>(name: &str, payload: &P)
where
    P: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert!(EVENT_NAMES.contains(&name), "{name} нет в списке событий");
    let payload = round_trip(payload);
    assert_rejects_unknown_field::<P>(&payload);
    let contract = json!({ "event": name, "payload": payload });
    settings().bind(|| insta::assert_json_snapshot!(snapshot_name_of_event(name), contract));
}

/// `note:conflict-saved` → `event.note.conflict-saved`: двоеточие в имени
/// файла на Windows недопустимо.
fn snapshot_name_of_event(name: &str) -> String {
    format!("event.{}", name.replace(':', "."))
}

// ---- Примеры -------------------------------------------------------------

fn path(components: &[&str]) -> VaultPath {
    VaultPath::from_components(components.iter().copied())
}

fn version(hash_digit: char, size: u64) -> FileVersion {
    FileVersion {
        hash: std::iter::repeat_n(hash_digit, 64).collect(),
        size,
        modified_ms: 1_761_382_800_000,
    }
}

fn vault() -> VaultSummary {
    VaultSummary {
        id: VaultId("7f3c9a1e5b2d4c60".into()),
        name: "Работа".into(),
        path: "C:\\Users\\ivan\\Documents\\Работа".into(),
    }
}

fn err(code: ErrorCode) -> CoreError {
    CoreError::new(code)
}

fn settings_example() -> Settings {
    Settings {
        theme: Theme::System,
        editor_mode: EditorMode::LivePreview,
        text_width: TextWidth::Column,
        font_size: 16,
        new_note_location: NewNoteLocation::Folder {
            path: path(&["Входящие"]),
        },
        confirm_delete: true,
        max_edit_size_bytes: 10 * 1024 * 1024,
        hotkeys: [
            (
                "quick_switcher.open".to_string(),
                vec!["Ctrl+KeyO".to_string(), "Ctrl+KeyP".to_string()],
            ),
            ("tree.delete".to_string(), vec![]),
        ]
        .into_iter()
        .collect(),
        rename_links: RenameLinks::Ask,
    }
}

fn token(digit: char) -> RelinkToken {
    RelinkToken(std::iter::repeat_n(digit, 32).collect())
}

#[test]
fn vault_list() {
    let summary = vault();
    command(
        "vault_list",
        &VaultListResponse {
            vaults: vec![VaultListItem {
                id: summary.id.clone(),
                name: summary.name,
                path: summary.path,
                is_open: true,
                available: true,
            }],
            last_opened: Some(summary.id),
        },
        &[err(ErrorCode::IoFailed)],
    );
}

#[test]
fn vault_current() {
    command(
        "vault_current",
        &VaultCurrentResponse {
            api_version: 1,
            vault: Some(CurrentVault {
                id: vault().id,
                name: vault().name,
            }),
        },
        &[],
    );
}

#[test]
fn vault_create() {
    command_with(
        "vault_create",
        &VaultCreateRequest {
            name: "Работа".into(),
            parent: "C:\\Users\\ivan\\Documents".into(),
        },
        &VaultChooseOutcome::Opened { vault: vault() },
        &[
            CoreError::invalid_argument("name"),
            CoreError::invalid_argument("parent"),
            CoreError::invalid_argument("window"),
            CoreError::invalid_name(NameProblem::ReservedName),
            err(ErrorCode::AlreadyExists),
            err(ErrorCode::NestedVault),
        ],
    );
}

/// Выбор родительской папки нового хранилища (задача 2.8б, расширение
/// контракта v1).
#[test]
fn folder_pick() {
    command(
        "folder_pick",
        &FolderPickOutcome::Picked {
            path: "C:\\Users\\ivan\\Documents".into(),
        },
        &[
            err(ErrorCode::NestedVault),
            err(ErrorCode::VaultUnavailable),
            CoreError::invalid_argument("window"),
        ],
    );
}

#[test]
fn vault_open_folder() {
    command(
        "vault_open_folder",
        &VaultChooseOutcome::Cancelled,
        &[
            err(ErrorCode::VaultUnavailable),
            err(ErrorCode::NestedVault),
            CoreError::invalid_argument("window"),
        ],
    );
}

#[test]
fn vault_open() {
    command_with(
        "vault_open",
        &VaultIdRequest { id: vault().id },
        &VaultOpenOutcome::Focused,
        &[
            err(ErrorCode::VaultNotFound),
            err(ErrorCode::VaultUnavailable),
        ],
    );
}

#[test]
fn vault_remove_from_list() {
    command_with(
        "vault_remove_from_list",
        &VaultIdRequest { id: vault().id },
        &(),
        &[
            err(ErrorCode::VaultNotFound),
            CoreError::invalid_argument("window"),
        ],
    );
}

/// Окно «Управление хранилищами» (задача 2.8а, расширение контракта v1).
#[test]
fn vault_manager_open() {
    command("vault_manager_open", &(), &[err(ErrorCode::IoFailed)]);
}

/// Закрытие окна хранилища после записи ввода (задача 0.7, расширение
/// контракта v1): ответы интерфейса на `window:close-requested`.
#[test]
fn window_close_ready() {
    command("window_close_ready", &(), &[]);
}

#[test]
fn window_close_cancel() {
    command("window_close_cancel", &(), &[]);
}

#[test]
fn window_shown_note() {
    command_with(
        "window_shown_note",
        &WindowShownNoteRequest {
            path: Some(path(&["Проекты", "Встреча.md"])),
        },
        &(),
        &[
            err(ErrorCode::NotFound),
            err(ErrorCode::NotANote),
            err(ErrorCode::InvalidPath),
            err(ErrorCode::NoVaultOpen),
        ],
    );
    // «Заметка не открыта» — `path: null`.
    let none = round_trip(&WindowShownNoteRequest { path: None });
    assert_eq!(none, json!({ "path": null }));
}

#[test]
fn clipboard_read_text() {
    command(
        "clipboard_read_text",
        &ClipboardReadOutcome::Text {
            text: "Текст из буфера\nвторая строка".into(),
        },
        &[err(ErrorCode::NoVaultOpen), err(ErrorCode::IoFailed)],
    );
}

#[test]
fn tree_list() {
    command(
        "tree_list",
        &TreeListResponse {
            entries: vec![
                TreeEntry {
                    path: path(&["Проекты"]),
                    kind: EntryKind::Folder,
                },
                TreeEntry {
                    path: path(&["Проекты", "Встреча.md"]),
                    kind: EntryKind::Note,
                },
                TreeEntry {
                    path: path(&["Проекты", "схема.png"]),
                    kind: EntryKind::Attachment,
                },
            ],
        },
        &[err(ErrorCode::NoVaultOpen), err(ErrorCode::IoFailed)],
    );
}

#[test]
fn note_create() {
    command_with(
        "note_create",
        &NameInDirRequest {
            dir: path(&["Проекты"]),
            name: "Без названия.md".into(),
        },
        &EntryPathResponse {
            path: path(&["Проекты", "Без названия.md"]),
        },
        &[
            CoreError::invalid_name(NameProblem::Duplicate),
            err(ErrorCode::InvalidPath),
            err(ErrorCode::OutsideVault),
        ],
    );
}

#[test]
fn folder_create() {
    command_with(
        "folder_create",
        &NameInDirRequest {
            dir: VaultPath::root(),
            name: "Архив".into(),
        },
        &EntryPathResponse {
            path: path(&["Архив"]),
        },
        &[CoreError::invalid_name(NameProblem::TrailingDotOrSpace)],
    );
}

#[test]
fn entry_rename() {
    command_with(
        "entry_rename",
        &EntryRenameRequest {
            path: path(&["Проекты", "Без названия.md"]),
            new_name: "Встреча 2.md".into(),
            links: LinksMode::Keep,
            token: None,
        },
        &EntryMovedResponse {
            path: path(&["Проекты", "Встреча 2.md"]),
            relink: None,
        },
        &[
            err(ErrorCode::NotFound),
            CoreError::invalid_name(NameProblem::ForbiddenChars),
            CoreError::invalid_name(NameProblem::NoteExtension),
        ],
    );
}

#[test]
fn entry_move() {
    command_with(
        "entry_move",
        &EntryMoveRequest {
            path: path(&["Проекты", "Встреча 2.md"]),
            new_dir: path(&["Архив"]),
            links: LinksMode::Keep,
            token: None,
        },
        &EntryMovedResponse {
            path: path(&["Архив", "Встреча 2.md"]),
            relink: None,
        },
        &[err(ErrorCode::AlreadyExists), err(ErrorCode::NotFound)],
    );
}

/// Переименование с обновлением ссылок (0.2): знак из `relink_preview`,
/// в ответе — итог переписывания.
#[test]
fn entry_rename_update() {
    command_variant(
        "entry_rename",
        "update",
        &EntryRenameRequest {
            path: path(&["Проекты", "Встреча.md"]),
            new_name: "Планёрка.md".into(),
            links: LinksMode::Update,
            token: Some(token('a')),
        },
        &EntryMovedResponse {
            path: path(&["Проекты", "Планёрка.md"]),
            relink: Some(RelinkOutcome {
                updated: 4,
                failed: vec![RelinkFailure {
                    path: path(&["Архив", "Старое.md"]),
                    reason: RelinkFailureReason::Busy,
                }],
                retry_token: Some(token('b')),
            }),
        },
        &[
            CoreError::invalid_argument("token"),
            err(ErrorCode::NotFound),
            CoreError::invalid_name(NameProblem::Duplicate),
        ],
    );
}

/// Перенос с обновлением ссылок (0.2); все заметки переписаны — повторять
/// нечего.
#[test]
fn entry_move_update() {
    command_variant(
        "entry_move",
        "update",
        &EntryMoveRequest {
            path: path(&["Альфа", "Встреча.md"]),
            new_dir: path(&["Бета"]),
            links: LinksMode::Update,
            token: Some(token('c')),
        },
        &EntryMovedResponse {
            path: path(&["Бета", "Встреча.md"]),
            relink: Some(RelinkOutcome {
                updated: 2,
                failed: vec![],
                retry_token: None,
            }),
        },
        &[
            CoreError::invalid_argument("token"),
            err(ErrorCode::AlreadyExists),
        ],
    );
}

/// Запрос 0.1 без `links` и `token` читается как `keep` и пишется обратно
/// без новых полей: прежний снимок `entry_rename` не меняется (Р16).
#[test]
fn entry_rename_without_links_is_request_of_0_1() {
    let old = json!({ "path": ["Проекты", "Без названия.md"], "new_name": "Встреча 2.md" });
    let request: EntryRenameRequest = serde_json::from_value(old.clone()).expect("запрос 0.1");
    assert_eq!(request.links, LinksMode::Keep);
    assert_eq!(request.token, None);
    assert_eq!(serde_json::to_value(&request).expect("запрос"), old);
    let old_move = json!({ "path": ["a.md"], "new_dir": ["Б"] });
    let request: EntryMoveRequest = serde_json::from_value(old_move.clone()).expect("запрос 0.1");
    assert_eq!(request.links, LinksMode::Keep);
    assert_eq!(serde_json::to_value(&request).expect("запрос"), old_move);
    let response = EntryMovedResponse {
        path: path(&["a.md"]),
        relink: None,
    };
    assert_eq!(
        serde_json::to_value(&response).expect("ответ"),
        json!({ "path": ["a.md"] })
    );
}

#[test]
fn entry_trash() {
    command_with(
        "entry_trash",
        &EntryPathRequest {
            path: path(&["Архив", "Встреча 2.md"]),
        },
        &TrashOutcome::Trashed,
        &[err(ErrorCode::NotFound), err(ErrorCode::IoFailed)],
    );
}

#[test]
fn name_check() {
    command_with(
        "name_check",
        &NameCheckRequest {
            dir: path(&["Проекты"]),
            name: "Встреча.md".into(),
            renaming: Some(path(&["Проекты", "встреча.md"])),
        },
        &NameCheck::Ok {
            warning: Some(NameWarning::Unlinkable),
        },
        &[
            err(ErrorCode::InvalidPath),
            CoreError::invalid_argument("dir"),
        ],
    );
}

#[test]
fn note_read() {
    command_with(
        "note_read",
        &NoteReadRequest {
            path: path(&["Проекты", "Встреча.md"]),
        },
        &NoteReadResponse {
            text: "# Встреча\nСм. [[Альфа/План]]\n".into(),
            version: version('a', 43),
            read_only: Some(ReadOnlyReason::TooLarge {
                limit_bytes: 10 * 1024 * 1024,
            }),
        },
        &[
            err(ErrorCode::NotFound),
            err(ErrorCode::NotANote),
            err(ErrorCode::InvalidPath),
            err(ErrorCode::OutsideVault),
        ],
    );
}

#[test]
fn note_write() {
    command_with(
        "note_write",
        &NoteWriteRequest {
            path: path(&["Проекты", "Встреча.md"]),
            text: "# Встреча\nИтоги\n".into(),
            expected_version: version('a', 43),
        },
        &NoteWriteResponse::ConflictSaved {
            version: version('b', 21),
            copy_path: path(&["Проекты", "Встреча (внешняя версия 2026-10-25 12-00).md"]),
        },
        &[
            err(ErrorCode::ReadOnly),
            err(ErrorCode::NotFound),
            CoreError::invalid_argument("text"),
        ],
    );
}

#[test]
fn links_resolve() {
    command_with(
        "links_resolve",
        &LinksResolveRequest {
            from: path(&["Проекты", "Встреча.md"]),
            targets: vec!["Альфа/План".into(), "Нет такой".into()],
        },
        &LinksResolveResponse {
            results: vec![
                ResolvedLink {
                    target: "Альфа/План".into(),
                    path: Some(path(&["Проекты", "Альфа", "План.md"])),
                },
                ResolvedLink {
                    target: "Нет такой".into(),
                    path: None,
                },
            ],
        },
        &[err(ErrorCode::InvalidArgument)],
    );
}

#[test]
fn links_suggest() {
    command_with(
        "links_suggest",
        &LinksSuggestRequest {
            from: path(&["Дневник.md"]),
            query: "встр".into(),
        },
        &LinksSuggestResponse {
            items: vec![
                LinkSuggestion {
                    insert: Some("Альфа/Встреча".into()),
                    name: "Встреча".into(),
                    path: path(&["Проекты", "Альфа", "Встреча.md"]),
                    linkable: true,
                },
                LinkSuggestion {
                    insert: Some("/Встреча".into()),
                    name: "Встреча".into(),
                    path: path(&["Встреча.md"]),
                    linkable: true,
                },
                LinkSuggestion {
                    insert: None,
                    name: "#Встреча 2".into(),
                    path: path(&["#Встреча 2.md"]),
                    linkable: false,
                },
            ],
        },
        &[err(ErrorCode::NoVaultOpen)],
    );
}

#[test]
fn switcher_search() {
    command_with(
        "switcher_search",
        &SwitcherSearchRequest {
            query: "встр альф".into(),
        },
        &SwitcherSearchResponse {
            items: vec![SwitcherItem {
                name: "Встреча".into(),
                path: path(&["Проекты", "Альфа", "Встреча.md"]),
            }],
        },
        &[err(ErrorCode::NoVaultOpen)],
    );
}

#[test]
fn settings_read() {
    command(
        "settings_read",
        &SettingsReadResponse {
            settings: settings_example(),
        },
        &[err(ErrorCode::IoFailed)],
    );
}

#[test]
fn settings_write() {
    command_with(
        "settings_write",
        &SettingsWriteRequest {
            settings: Settings {
                theme: Theme::Dark,
                editor_mode: EditorMode::Source,
                text_width: TextWidth::Full,
                new_note_location: NewNoteLocation::CurrentFolder,
                rename_links: RenameLinks::Update,
                ..settings_example()
            },
        },
        &(),
        &[CoreError::invalid_argument("settings.font_size")],
    );
}

#[test]
fn workspace_read() {
    command(
        "workspace_read",
        &WorkspaceReadResponse { workspace: None },
        &[err(ErrorCode::IoFailed)],
    );
}

#[test]
fn workspace_write() {
    let Value::Object(layout) = json!({
        "format_version": 1,
        "panes": [{ "note": ["Проекты", "Встреча.md"] }]
    }) else {
        unreachable!()
    };
    command_with(
        "workspace_write",
        &WorkspaceWriteRequest {
            workspace: Workspace(layout),
        },
        &(),
        &[CoreError::invalid_argument("workspace")],
    );
}

#[test]
fn open_external() {
    command_with(
        "open_external",
        &OpenExternalRequest {
            url: "https://example.org/статья?id=1".into(),
        },
        &OpenExternalOutcome::Opened,
        &[err(ErrorCode::SchemeNotAllowed)],
    );
}

// ---- 0.2 «Связи»: индекс и обновление ссылок -------------------------------

#[test]
fn command_index_status() {
    command(
        "index_status",
        &IndexStatus {
            state: IndexState::Building,
            done: 1_200,
            total: 10_000,
        },
        &[err(ErrorCode::NoVaultOpen)],
    );
}

#[test]
fn command_backlinks_list() {
    command_with(
        "backlinks_list",
        &BacklinksListRequest {
            path: path(&["Проекты", "Встреча.md"]),
        },
        &IndexReply::Ready(Backlinks {
            notes_total: 1,
            truncated: false,
            notes: vec![BacklinkNote {
                from: path(&["Дневник", "2026-11-01.md"]),
                name: "2026-11-01".into(),
                links_total: 2,
                links: vec![
                    BacklinkLine {
                        line: 3,
                        column_utf16: 9,
                        snippet: "Обсудили [[Встреча|встречу]] и <b>планы</b>".into(),
                        from_utf16: 9,
                        to_utf16: 28,
                    },
                    BacklinkLine {
                        line: 7,
                        column_utf16: 0,
                        snippet: "![[Встреча#Итоги]]".into(),
                        from_utf16: 0,
                        to_utf16: 18,
                    },
                ],
            }],
        }),
        &[
            err(ErrorCode::InvalidPath),
            CoreError::invalid_argument("path"),
            err(ErrorCode::NoVaultOpen),
        ],
    );
}

#[test]
fn command_tags_list() {
    command(
        "tags_list",
        &IndexReply::Ready(Tags {
            tags: vec![
                TagSummary {
                    tag: "Проект".into(),
                    key: "проект".into(),
                    count: 12,
                },
                TagSummary {
                    tag: "работа/отчёт".into(),
                    key: "работа/отчёт".into(),
                    count: 3,
                },
            ],
        }),
        &[err(ErrorCode::NoVaultOpen)],
    );
}

#[test]
fn command_tag_notes() {
    command_with(
        "tag_notes",
        &TagNotesRequest {
            key: "проект".into(),
        },
        &IndexReply::Ready(TagNotes {
            notes_total: 2,
            notes: vec![path(&["Проекты", "Альфа.md"]), path(&["Встреча.md"])],
        }),
        &[
            CoreError::invalid_argument("key"),
            err(ErrorCode::NoVaultOpen),
        ],
    );
}

#[test]
fn command_relink_preview() {
    command_with(
        "relink_preview",
        &RelinkPreviewRequest::Rename {
            path: path(&["Проекты", "Встреча.md"]),
            new_name: "Планёрка.md".into(),
        },
        &IndexReply::Ready(RelinkPreview {
            notes: 5,
            links: 7,
            token: token('a'),
        }),
        &[
            CoreError::invalid_name(NameProblem::NoteExtension),
            err(ErrorCode::NotFound),
            err(ErrorCode::NoVaultOpen),
        ],
    );
}

/// Перенос, пока индекс строится: окно ждёт готовности (решение 114).
#[test]
fn command_relink_preview_index_building() {
    command_variant(
        "relink_preview",
        "index_building",
        &RelinkPreviewRequest::Move {
            path: path(&["Альфа", "Встреча.md"]),
            new_dir: path(&["Бета"]),
        },
        &IndexReply::<RelinkPreview>::IndexBuilding(IndexProgress {
            done: 1_200,
            total: 10_000,
        }),
        &[CoreError::invalid_argument("new_dir")],
    );
}

#[test]
fn command_relink_answer() {
    command_with(
        "relink_answer",
        &RelinkAnswerRequest {
            token: token('d'),
            apply: true,
        },
        &(),
        &[
            CoreError::invalid_argument("token"),
            err(ErrorCode::NoVaultOpen),
        ],
    );
}

#[test]
fn command_links_flush_done() {
    command_with(
        "links_flush_done",
        &LinksFlushDoneRequest {
            token: token('e'),
            saved: true,
        },
        &(),
        &[
            CoreError::invalid_argument("token"),
            err(ErrorCode::NoVaultOpen),
        ],
    );
}

#[test]
fn event_index_status() {
    event(
        event::INDEX_STATUS,
        &IndexStatus {
            state: IndexState::Ready,
            done: 10_000,
            total: 10_000,
        },
    );
}

#[test]
fn event_index_changed() {
    event(event::INDEX_CHANGED, &IndexChangedPayload {});
}

#[test]
fn event_links_flush_requested() {
    event(
        event::LINKS_FLUSH_REQUESTED,
        &LinksFlushRequestedPayload { token: token('f') },
    );
}

#[test]
fn event_links_pending() {
    event(
        event::LINKS_PENDING,
        &LinksPendingPayload {
            token: token('1'),
            reason: PendingReason::ExternalRename,
            notes: 12,
            links: 15,
            renames: vec![PathRename {
                from: path(&["Встреча.md"]),
                to: path(&["Архив", "Встреча.md"]),
            }],
            renames_total: 3,
        },
    );
}

#[test]
fn event_links_progress() {
    event(
        event::LINKS_PROGRESS,
        &LinksProgressPayload {
            token: None,
            done: 250,
            total: 1_000,
        },
    );
}

/// `settings_write` без `rename_links` (интерфейс 0.1) — `ask`.
#[test]
fn settings_without_rename_links_is_ask() {
    let mut request = serde_json::to_value(SettingsWriteRequest {
        settings: Settings {
            rename_links: RenameLinks::Update,
            ..settings_example()
        },
    })
    .expect("запрос");
    request["settings"]
        .as_object_mut()
        .expect("настройки")
        .remove("rename_links");
    let request: SettingsWriteRequest = serde_json::from_value(request).expect("без rename_links");
    assert_eq!(request.settings.rename_links, RenameLinks::Ask);
}

#[test]
fn event_links_updated() {
    event(
        event::LINKS_UPDATED,
        &LinksUpdatedPayload::new(
            LinksUpdateReason::ExternalRename,
            RelinkOutcome {
                updated: 11,
                failed: vec![
                    RelinkFailure {
                        path: path(&["Большая.md"]),
                        reason: RelinkFailureReason::TooLarge,
                    },
                    RelinkFailure {
                        path: path(&["Открыта в Word.md"]),
                        reason: RelinkFailureReason::Busy,
                    },
                ],
                retry_token: Some(token('2')),
            },
        ),
    );
}

#[test]
fn event_vault_changed() {
    event(
        event::VAULT_CHANGED,
        &VaultChangedPayload {
            rescan: false,
            changes: vec![
                TreeChange::Created {
                    path: path(&["Новая.md"]),
                    entry_kind: EntryKind::Note,
                },
                TreeChange::Modified {
                    path: path(&["Проекты", "Встреча.md"]),
                },
                TreeChange::Removed {
                    path: path(&["Старое"]),
                },
                TreeChange::Renamed {
                    from: path(&["План.md"]),
                    to: path(&["Проекты", "План.md"]),
                },
            ],
        },
    );
}

#[test]
fn event_note_changed() {
    event(
        event::NOTE_CHANGED,
        &NoteChangedPayload {
            path: path(&["Проекты", "Встреча.md"]),
            version: version('c', 50),
        },
    );
}

#[test]
fn event_note_renamed() {
    event(
        event::NOTE_RENAMED,
        &NoteRenamedPayload {
            from: path(&["Проекты", "Встреча.md"]),
            to: path(&["Архив", "Встреча.md"]),
        },
    );
}

#[test]
fn event_note_deleted() {
    event(
        event::NOTE_DELETED,
        &NoteDeletedPayload {
            path: path(&["Архив", "Встреча.md"]),
        },
    );
}

#[test]
fn event_note_conflict_saved() {
    event(
        event::NOTE_CONFLICT_SAVED,
        &NoteConflictSavedPayload {
            path: path(&["Проекты", "Встреча.md"]),
            copy_path: path(&["Проекты", "Встреча (внешняя версия 2026-10-25 12-00).md"]),
        },
    );
}

#[test]
fn event_vault_availability() {
    event(
        event::VAULT_AVAILABILITY,
        &VaultAvailabilityPayload { available: false },
    );
}

/// Данные без поля `sessionEnd` (как до задачи 0.11а) — закрытие окна.
#[test]
fn event_window_close_requested_without_session_end_is_window_close() {
    let payload: WindowCloseRequestedPayload =
        serde_json::from_str("{}").expect("пустые данные события");
    assert_eq!(payload, WindowCloseRequestedPayload { session_end: false });
}

#[test]
fn event_window_close_requested() {
    event(
        event::WINDOW_CLOSE_REQUESTED,
        &WindowCloseRequestedPayload { session_end: true },
    );
}

/// В папке снимков ровно по файлу на каждую команду и событие (и на каждый
/// второй пример из [`COMMAND_VARIANTS`]): ни одна команда не осталась без
/// снимка, ни один снимок не устарел.
#[test]
fn every_command_and_event_has_exactly_one_snapshot() {
    const EXTRA: [&str; 2] = ["enums.snap", "limits.snap"];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/v1");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("элемент папки")
                .file_name()
                .into_string()
                .expect("имя")
        })
        .filter(|name| name.ends_with(".snap"))
        .collect();
    found.sort();
    let mut expected: Vec<String> = COMMAND_NAMES
        .iter()
        .map(|name| format!("{name}.snap"))
        .chain(
            EVENT_NAMES
                .iter()
                .map(|name| format!("{}.snap", snapshot_name_of_event(name))),
        )
        .chain(EXTRA.iter().map(|name| (*name).to_owned()))
        .chain(
            COMMAND_VARIANTS
                .iter()
                .map(|(name, variant)| format!("{name}.{variant}.snap")),
        )
        .collect();
    expected.sort();
    assert_eq!(found, expected);
}

/// `VaultPath` в JSON — массив компонентов, корень — пустой массив.
#[test]
fn vault_path_is_array_of_components() {
    assert_eq!(
        serde_json::to_value(path(&["Проекты", "Встреча.md"])).expect("путь"),
        json!(["Проекты", "Встреча.md"])
    );
    assert_eq!(
        serde_json::to_value(VaultPath::root()).expect("корень"),
        json!([])
    );
    assert!(serde_json::from_value::<VaultPath>(json!("Проекты/Встреча.md")).is_err());
}

/// `renaming` в `name_check` можно не передавать.
#[test]
fn name_check_renaming_is_optional() {
    let request: NameCheckRequest =
        serde_json::from_value(json!({ "dir": [], "name": "a.md" })).expect("без renaming");
    assert_eq!(request.renaming, None);
    let request: NameCheckRequest =
        serde_json::from_value(json!({ "dir": [], "name": "a.md", "renaming": null }))
            .expect("renaming: null");
    assert_eq!(request.renaming, None);
}

/// Все варианты перечисления; `match` без `_` не даст забыть новый вариант.
macro_rules! all_variants {
    ($ty:ident: $($variant:ident),* $(,)?) => {{
        let all = vec![$($ty::$variant),*];
        for value in &all {
            match value {
                $($ty::$variant)|* => {}
            }
        }
        all
    }};
}

fn tags<T: Serialize>(values: &[T], tag: &str) -> Vec<Value> {
    values
        .iter()
        .map(|value| {
            let json = serde_json::to_value(value).expect("вариант");
            match tag {
                "" => json,
                tag => json[tag].clone(),
            }
        })
        .collect()
}

/// Снимок всех вариантов всех перечислений контракта: Vitest проверяет, что
/// в TypeScript те же варианты, не больше и не меньше.
#[test]
fn enums() {
    let error_codes = ErrorCode::ALL;
    for code in &error_codes {
        match code {
            ErrorCode::NotImplemented
            | ErrorCode::InvalidArgument
            | ErrorCode::NoVaultOpen
            | ErrorCode::VaultNotFound
            | ErrorCode::VaultUnavailable
            | ErrorCode::InvalidPath
            | ErrorCode::OutsideVault
            | ErrorCode::NotFound
            | ErrorCode::AlreadyExists
            | ErrorCode::InvalidName
            | ErrorCode::NotANote
            | ErrorCode::ReadOnly
            | ErrorCode::SchemeNotAllowed
            | ErrorCode::IoFailed
            | ErrorCode::NestedVault => {}
        }
    }
    let name_problems = NameProblem::ALL;
    for problem in &name_problems {
        match problem {
            NameProblem::Empty
            | NameProblem::ForbiddenChars
            | NameProblem::ReservedName
            | NameProblem::TrailingDotOrSpace
            | NameProblem::TooLong
            | NameProblem::Duplicate
            | NameProblem::NoteExtension => {}
        }
    }
    let p = VaultPath::root;
    let v = || version('0', 0);
    let read_only = [
        ReadOnlyReason::TooLarge { limit_bytes: 0 },
        ReadOnlyReason::InvalidUtf8,
    ];
    for value in &read_only {
        match value {
            ReadOnlyReason::TooLarge { .. } | ReadOnlyReason::InvalidUtf8 => {}
        }
    }
    let note_write = [
        NoteWriteResponse::Saved { version: v() },
        NoteWriteResponse::ConflictSaved {
            version: v(),
            copy_path: p(),
        },
    ];
    for value in &note_write {
        match value {
            NoteWriteResponse::Saved { .. } | NoteWriteResponse::ConflictSaved { .. } => {}
        }
    }
    let name_check = [
        NameCheck::Ok { warning: None },
        NameCheck::rejected(NameProblem::Empty),
    ];
    for value in &name_check {
        match value {
            NameCheck::Ok { .. } | NameCheck::Rejected { .. } => {}
        }
    }
    let vault_choose = [
        VaultChooseOutcome::Opened { vault: vault() },
        VaultChooseOutcome::Cancelled,
    ];
    for value in &vault_choose {
        match value {
            VaultChooseOutcome::Opened { .. } | VaultChooseOutcome::Cancelled => {}
        }
    }
    let new_note_location = [
        NewNoteLocation::VaultRoot,
        NewNoteLocation::CurrentFolder,
        NewNoteLocation::Folder { path: p() },
    ];
    for value in &new_note_location {
        match value {
            NewNoteLocation::VaultRoot
            | NewNoteLocation::CurrentFolder
            | NewNoteLocation::Folder { .. } => {}
        }
    }
    let tree_change = [
        TreeChange::Created {
            path: p(),
            entry_kind: EntryKind::Note,
        },
        TreeChange::Modified { path: p() },
        TreeChange::Removed { path: p() },
        TreeChange::Renamed { from: p(), to: p() },
    ];
    for value in &tree_change {
        match value {
            TreeChange::Created { .. }
            | TreeChange::Modified { .. }
            | TreeChange::Removed { .. }
            | TreeChange::Renamed { .. } => {}
        }
    }

    let clipboard = [
        ClipboardReadOutcome::Text {
            text: String::new(),
        },
        ClipboardReadOutcome::Empty,
        ClipboardReadOutcome::TooLarge { limit_bytes: 0 },
        ClipboardReadOutcome::NotFocused,
    ];
    for value in &clipboard {
        match value {
            ClipboardReadOutcome::Text { .. }
            | ClipboardReadOutcome::Empty
            | ClipboardReadOutcome::TooLarge { .. }
            | ClipboardReadOutcome::NotFocused => {}
        }
    }

    let index_reply: [IndexReply<()>; 2] = [IndexReply::Ready(()), IndexReply::building_unknown()];
    for value in &index_reply {
        match value {
            IndexReply::Ready(_) | IndexReply::IndexBuilding(_) => {}
        }
    }
    let preview_op = [
        RelinkPreviewRequest::Rename {
            path: p(),
            new_name: String::new(),
        },
        RelinkPreviewRequest::Move {
            path: p(),
            new_dir: p(),
        },
    ];
    for value in &preview_op {
        match value {
            RelinkPreviewRequest::Rename { .. } | RelinkPreviewRequest::Move { .. } => {}
        }
    }

    let enums = json!({
        "ErrorCode": tags(&error_codes, ""),
        "NameProblem": tags(&name_problems, ""),
        "NameWarning": tags(&all_variants!(NameWarning: Unlinkable, Hidden), ""),
        "EntryKind": tags(&all_variants!(EntryKind: Folder, Note, Attachment), ""),
        "Theme": tags(&all_variants!(Theme: System, Light, Dark), ""),
        "EditorMode": tags(&all_variants!(EditorMode: LivePreview, Source), ""),
        "TextWidth": tags(&all_variants!(TextWidth: Column, Full), ""),
        "NewNoteLocation.kind": tags(&new_note_location, "kind"),
        "ReadOnlyReason.reason": tags(&read_only, "reason"),
        "NoteWriteResponse.outcome": tags(&note_write, "outcome"),
        "NameCheck.status": tags(&name_check, "status"),
        "TrashOutcome.outcome": tags(
            &all_variants!(TrashOutcome: Trashed, DeletedPermanently, Cancelled),
            "outcome"
        ),
        "VaultChooseOutcome.outcome": tags(&vault_choose, "outcome"),
        "VaultOpenOutcome.outcome": tags(&all_variants!(VaultOpenOutcome: Opened, Focused), "outcome"),
        "OpenExternalOutcome.outcome": tags(
            &all_variants!(OpenExternalOutcome: Opened, Cancelled),
            "outcome"
        ),
        "TreeChange.kind": tags(&tree_change, "kind"),
        "ClipboardReadOutcome.outcome": tags(&clipboard, "outcome"),
        "RenameLinks": tags(&all_variants!(RenameLinks: Ask, Update), ""),
        "LinksMode": tags(&all_variants!(LinksMode: Keep, Update), ""),
        "IndexState": tags(&all_variants!(IndexState: Building, Ready), ""),
        "IndexReply.status": index_reply
            .iter()
            .map(|reply| match reply {
                IndexReply::Ready(_) => json!("ready"),
                building => serde_json::to_value(building).expect("вариант")["status"].clone(),
            })
            .collect::<Vec<_>>(),
        "RelinkPreviewRequest.op": tags(&preview_op, "op"),
        "PendingReason": tags(&all_variants!(PendingReason: ExternalRename, Duplicate), ""),
        "LinksUpdateReason": tags(
            &all_variants!(LinksUpdateReason: Rename, ExternalRename, Duplicate, Resumed, Retry),
            ""
        ),
        "RelinkFailureReason": tags(
            &all_variants!(RelinkFailureReason: TooLarge, NotUtf8, Busy, Io),
            ""
        ),
    });
    settings().bind(|| insta::assert_json_snapshot!("enums", enums));
}

/// Пределы параметров — те же числа читает `src/api/contract.test.ts` (константы — `src/api/limits.ts`).
#[test]
fn limits() {
    settings().bind(|| insta::assert_json_snapshot!("limits", super::limits::LIMITS));
}

/// Повторить можно только занятые и недоступные заметки (решение 115).
#[test]
fn only_busy_and_io_are_retryable() {
    let retryable: Vec<_> = all_variants!(RelinkFailureReason: TooLarge, NotUtf8, Busy, Io)
        .into_iter()
        .filter(|reason| reason.is_retryable())
        .collect();
    assert_eq!(
        retryable,
        vec![RelinkFailureReason::Busy, RelinkFailureReason::Io]
    );
}
