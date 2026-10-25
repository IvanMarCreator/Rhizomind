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
    }
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
        },
        &VaultChooseOutcome::Opened { vault: vault() },
        &[
            CoreError::invalid_argument("name"),
            CoreError::invalid_name(NameProblem::ReservedName),
            err(ErrorCode::AlreadyExists),
        ],
    );
}

#[test]
fn vault_open_folder() {
    command(
        "vault_open_folder",
        &VaultChooseOutcome::Cancelled,
        &[err(ErrorCode::VaultUnavailable)],
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
        &[err(ErrorCode::VaultNotFound)],
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
        },
        &EntryPathResponse {
            path: path(&["Проекты", "Встреча 2.md"]),
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
        },
        &EntryPathResponse {
            path: path(&["Архив", "Встреча 2.md"]),
        },
        &[err(ErrorCode::AlreadyExists), err(ErrorCode::NotFound)],
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

/// В папке снимков ровно по файлу на каждую команду и событие: ни одна
/// команда не осталась без снимка, ни один снимок не устарел.
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
            | ErrorCode::IoFailed => {}
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

    let enums = json!({
        "ErrorCode": tags(&error_codes, ""),
        "NameProblem": tags(&name_problems, ""),
        "NameWarning": tags(&all_variants!(NameWarning: Unlinkable), ""),
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
    });
    settings().bind(|| insta::assert_json_snapshot!("enums", enums));
}

/// Пределы параметров — те же числа читает `src/api/limits.test.ts`.
#[test]
fn limits() {
    settings().bind(|| insta::assert_json_snapshot!("limits", super::limits::LIMITS));
}
