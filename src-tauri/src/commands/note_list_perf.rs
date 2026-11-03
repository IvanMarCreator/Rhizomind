//! Замеры ядра на уровне команд окна (задача 0.2-7.1, С2-8.3, С2-5.2).
//! Общие помощники и методика — `index::perf_tests` (медиана [`REPEATS`]
//! повторов, пределы — выпускная сборка на Windows, 10 000 заметок).
//!
//! - [`perf_backlinks_tags_on_real_core`] — `backlinks_list` «узла»
//!   холодный (первый вызов после открытия окна) и повторный, `tags_list`,
//!   `tag_notes` через тот же мост, что из WebView (`core_tests`).
//! - [`perf_note_list_replace_notes`] — риск С2-5.2: `NoteList` после пачки
//!   изменений дерева перестраивает ссылки и быстрый переход целиком
//!   (`Links::replace_notes`, `Switcher::replace_notes`). Условие задачи
//!   0.2-7.3: пачка 1 000 изменений на 50 000 заметок дольше 200 мс.
//! - [`perf_relink_rename_move_via_commands`] — переименование «узла» и
//!   перенос папки на 1 000 заметок с обновлением ссылок, от ответа до
//!   `links:updated` (второй прогон 0.2-7.1, перед КТ2).
//!
//! Запуск: `cargo test -p rhizomind --release --lib -- --ignored --nocapture --test-threads=1 perf_`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, CapabilityBuilder, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewWindow, WebviewWindowBuilder};

use super::*;
use crate::events::WindowEmitter;
use crate::index::perf_tests::{CHECKED_NOTES, REPEATS, Report, median, ms, scales, timed};
use crate::index::store::{IndexStore, WriteOutcome};
use crate::index::{VaultIndex, rebuild};
use crate::links::Links;
use crate::switcher::Switcher;
use crate::vault::NoteRef;

const LOCAL_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

/// Тестовое приложение, как `core_tests::app`.
fn app() -> App<MockRuntime> {
    let data_dir = std::env::temp_dir().join(format!("rhz-perf-test-{}", std::process::id()));
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

/// Окно с правами на все команды ядра, как `core_tests::window`.
fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    let capability = COMMAND_NAMES.iter().fold(
        CapabilityBuilder::new(format!("test-{label}")).window(label),
        |capability, name| capability.permission(format!("allow-{}", name.replace('_', "-"))),
    );
    app.add_capability(capability).expect("права окна");
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("окно")
}

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Value {
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
    .unwrap_or_else(|e| panic!("{command}: {e}"))
}

/// Готовый ответ команды индекса (`IndexReply::Ready`).
fn ready<T: serde::de::DeserializeOwned>(command: &str, value: Value) -> T {
    match serde_json::from_value::<IndexReply<T>>(value.clone()) {
        Ok(IndexReply::Ready(reply)) => reply,
        Ok(IndexReply::IndexBuilding(p)) => panic!("{command}: индекс строится: {p:?}"),
        Err(e) => panic!("{command}: ответ не по контракту ({e}): {value}"),
    }
}

/// Ждёт `index_status` = `ready`, как окно перед запросом панели.
fn wait_index_ready(w: &WebviewWindow<MockRuntime>, limit: Duration) {
    let start = Instant::now();
    loop {
        let status: IndexStatus =
            serde_json::from_value(invoke(w, "index_status", json!({}))).expect("index_status");
        if status.state == IndexState::Ready {
            return;
        }
        assert!(start.elapsed() < limit, "индекс не готов: {status:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `index.json` хранилища генератора — как после прошлого запуска
/// программы: окно откроется с готовым индексом (сверка, без сборки).
fn write_index_json(root: &Path) {
    let vault = crate::vault::Vault::open(root).expect("хранилище");
    let index = std::sync::RwLock::new(VaultIndex::new());
    rebuild(&vault, &index, &|_| {}).expect("собран");
    assert_eq!(
        IndexStore::new().write_now(&vault, &index),
        WriteOutcome::Written
    );
}

/// Панель обратных ссылок после открытия заметки ≤ 200 мс (50 000 — 300),
/// список тегов ≤ 100 мс (50 000 — 200) — С2-8.3.
///
/// **Холодный вызов** (ревью 0.2-1.4: в одном прогоне на Windows 1,56 с):
/// каждый повтор — новое хранилище генератора (своё зерно, файлы ещё ни
/// разу не открывались программой, на Windows их проверяет антивирус),
/// `index.json` как после прошлого запуска, новое приложение и окно;
/// после `index_status` = `ready` первый же `backlinks_list` «узла» (100
/// ссылающихся заметок: их строки читаются из файлов) — холодный. Предел —
/// по медиане холодных вызовов пяти повторов, как у остальных замеров.
/// Затем 5 повторных вызовов (худший — значение повтора), `tags_list` и
/// `tag_notes` самого частого тега (в С2-8.3 своей цели нет — предел
/// списка тегов).
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_backlinks_tags_on_real_core() {
    for notes in scales() {
        let checked = notes == CHECKED_NOTES;
        let (backlinks_limit, tags_limit) = if notes > CHECKED_NOTES {
            (300.0, 200.0)
        } else {
            (200.0, 100.0)
        };
        let mut report = Report::new(&format!("perf-core запросы окна, {notes} заметок"));
        let (mut cold, mut warm, mut tags, mut tag_notes) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut hub_total = 0;
        let mut tags_total = 0;
        for repeat in 0..REPEATS {
            let dir = tempfile::tempdir().expect("папка");
            let root = dir.path().join("vault");
            let manifest =
                gen_vault::generate(&root, &gen_vault::Config::new(notes, 7 + repeat as u64))
                    .expect("генератор");
            write_index_json(&root);
            let hub = manifest.hub.as_ref().expect("узел").path.clone();
            let hub: Vec<&str> = hub.split('/').collect();

            let app = app();
            let label = format!("perf-{notes}-{repeat}");
            let w = window(&app, &label);
            let core = app.state::<Core>();
            core.open_vault_with(
                &label,
                VaultSummary {
                    id: VaultId(label.clone()),
                    name: label.clone(),
                    path: root.display().to_string(),
                },
                &root,
                Arc::new(WindowEmitter::new(app.handle().clone(), label.as_str())),
                None,
            )
            .expect("хранилище открыто");
            wait_index_ready(&w, Duration::from_secs(300));

            let backlinks = |w: &WebviewWindow<MockRuntime>| -> Backlinks {
                ready(
                    "backlinks_list",
                    invoke(w, "backlinks_list", json!({ "request": { "path": hub } })),
                )
            };
            let (took, reply) = timed(|| backlinks(&w));
            cold.push(took);
            hub_total = reply.notes_total;
            let mut worst_warm: f64 = 0.0;
            let mut worst_tags: f64 = 0.0;
            let mut worst_tag_notes: f64 = 0.0;
            let mut top = String::new();
            for _ in 0..REPEATS {
                let (took, _) = timed(|| backlinks(&w));
                worst_warm = worst_warm.max(took);
                let (took, reply) =
                    timed(|| ready::<Tags>("tags_list", invoke(&w, "tags_list", json!({}))));
                worst_tags = worst_tags.max(took);
                tags_total = reply.tags.len();
                if top.is_empty() {
                    top = reply
                        .tags
                        .iter()
                        .max_by_key(|t| t.count)
                        .map(|t| t.key.clone())
                        .unwrap_or_default();
                }
                let (took, _) = timed(|| {
                    ready::<TagNotes>(
                        "tag_notes",
                        invoke(&w, "tag_notes", json!({ "request": { "key": top } })),
                    )
                });
                worst_tag_notes = worst_tag_notes.max(took);
            }
            warm.push(worst_warm);
            tags.push(worst_tags);
            tag_notes.push(worst_tag_notes);
            core.unbind_window(&label);
        }
        if notes >= 1_000 {
            assert_eq!(hub_total, 100, "ссылок на «узел»");
        }
        report.note(format!(
            "  «узел»: {hub_total} ссылающихся заметок; тегов: {tags_total}"
        ));
        report.measure(
            "backlinks_list холодный (первый после открытия окна)",
            &cold,
            "мс",
            Some(backlinks_limit),
            checked,
        );
        report.measure(
            "backlinks_list повторный (худший из 5 в повторе)",
            &warm,
            "мс",
            Some(backlinks_limit),
            checked,
        );
        report.measure(
            "tags_list (худший из 5 в повторе)",
            &tags,
            "мс",
            Some(tags_limit),
            checked,
        );
        report.measure(
            "tag_notes самого частого тега (худший из 5 в повторе)",
            &tag_notes,
            "мс",
            Some(tags_limit),
            checked,
        );
        report.finish();
    }
}

/// Заметка `i` синтетического списка: 100 заметок в папке.
fn note_path(i: usize, suffix: &str) -> VaultPath {
    VaultPath::from_components([
        format!("Папка {}", i / 100),
        format!("Заметка {i}{suffix}.md"),
    ])
}

/// Риск С2-5.2 и условие задачи 0.2-7.3: `NoteList::apply` с пачкой из 1,
/// 300 и 1 000 переименований (как пачка наблюдателя после массового
/// переименования снаружи или `git checkout`) — ссылки и быстрый переход
/// перестраиваются целиком один раз на пачку. Список заметок — в памяти
/// (диск не нужен), поэтому 50 000 меряется в каждом прогоне. Отдельно —
/// из чего складывается перестройка: `Links::replace_notes` и
/// `Switcher::replace_notes` на всём списке.
///
/// Предела в С2-8.3 нет (справочно); порог 0.2-7.3 — 200 мс на пачку
/// 1 000 изменений при 50 000 заметок: вывод «нужна ли 7.3» — в строке
/// сводки.
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_note_list_replace_notes() {
    let sizes = if std::env::var_os("RHIZOMIND_PERF_NOTES").is_some() {
        scales()
    } else {
        vec![10_000, 50_000]
    };
    let mut report = Report::new("perf-core NoteList::replace_notes (С2-5.2, 0.2-7.3)");
    let mut needs_7_3 = None;
    for &notes in &sizes {
        let refs: Vec<NoteRef> = (0..notes)
            .map(|i| NoteRef {
                path: note_path(i, ""),
                created_ms: i as i64,
            })
            .collect();
        let (new_ms, mut list) = timed(|| NoteList::new(refs.clone()));
        report.note(format!(
            "  {notes} заметок: NoteList::new {new_ms:.1} мс (справочно)"
        ));
        let links: Vec<f64> = (0..REPEATS)
            .map(|_| timed(|| Links::new(refs.clone())).0)
            .collect();
        let paths: Vec<VaultPath> = refs.iter().map(|n| n.path.clone()).collect();
        let switcher: Vec<f64> = (0..REPEATS)
            .map(|_| timed(|| Switcher::new(paths.clone())).0)
            .collect();
        report.measure(
            &format!("{notes}: Links::replace_notes на всём списке"),
            &links,
            "мс",
            None,
            false,
        );
        report.measure(
            &format!("{notes}: Switcher::replace_notes на всём списке"),
            &switcher,
            "мс",
            None,
            false,
        );
        for batch in [1usize, 300, 1_000] {
            let batch = batch.min(notes);
            let step = notes / batch;
            let mut took = Vec::new();
            for repeat in 0..REPEATS {
                // Туда и обратно: каждый повтор — настоящая пачка изменений.
                let (from, to) = if repeat % 2 == 0 {
                    ("", " р")
                } else {
                    (" р", "")
                };
                let changes: Vec<TreeChange> = (0..batch)
                    .map(|k| TreeChange::Renamed {
                        from: note_path(k * step, from),
                        to: note_path(k * step, to),
                    })
                    .collect();
                let (ms, changed) = timed(|| list.apply(&changes, &|_| Some(0)));
                assert!(changed, "пачка не применилась");
                took.push(ms);
            }
            // После нечётного числа повторов имена — с « р»; вернуть.
            if REPEATS % 2 == 1 {
                let back: Vec<TreeChange> = (0..batch)
                    .map(|k| TreeChange::Renamed {
                        from: note_path(k * step, " р"),
                        to: note_path(k * step, ""),
                    })
                    .collect();
                list.apply(&back, &|_| Some(0));
            }
            let limit = (notes == 50_000 && batch == 1_000).then_some(200.0);
            if limit.is_some() {
                needs_7_3 = Some(median(&took) > 200.0);
            }
            report.measure(
                &format!("{notes}: пачка {batch} изменений (apply + перестройка)"),
                &took,
                "мс",
                limit,
                false,
            );
        }
    }
    report.note(match needs_7_3 {
        Some(true) => "  вывод: 0.2-7.3 НУЖНА (пачка 1 000 на 50 000 дольше 200 мс)",
        Some(false) => {
            "  вывод: 0.2-7.3 не нужна по replace_notes (пачка 1 000 на 50 000 ≤ 200 мс)"
        }
        None => "  вывод по 0.2-7.3: 50 000 в этом прогоне не мерились",
    });
    report.finish();
}

/// Копия папки `from` в `to` со всеми вложенными (хранилище генератора
/// для нового повтора: выполнение плана переписывает заметки).
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("папка");
    for entry in std::fs::read_dir(from).expect("папка") {
        let entry = entry.expect("элемент");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("тип").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("копия");
        }
    }
}

/// Готовый ответ `relink_preview` (как окно перед вопросом «Обновить
/// ссылки?»): знак и число заметок и ссылок плана. Сразу после
/// `index_status` = `ready` предпросмотр может коротко отвечать
/// `index_building`, пока индекс сверяется (ревью 0.2-4.6) — ждём.
fn relink_preview(w: &WebviewWindow<MockRuntime>, request: Value) -> RelinkPreview {
    let start = Instant::now();
    loop {
        let reply = invoke(w, "relink_preview", json!({ "request": request }));
        match serde_json::from_value::<IndexReply<RelinkPreview>>(reply.clone()) {
            Ok(IndexReply::Ready(preview)) => return preview,
            Ok(IndexReply::IndexBuilding(_)) => {
                assert!(
                    start.elapsed() < Duration::from_secs(300),
                    "relink_preview: индекс не готов"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("relink_preview: ответ не по контракту ({e}): {reply}"),
        }
    }
}

/// Переименование «узла» (100 ссылающихся заметок) и перенос папки на
/// 1 000 заметок с обновлением ссылок — С2-8.3, «от ответа до уведомления»:
/// время от вызова `entry_rename`/`entry_move` с `links: update` и знаком
/// предпросмотра до события `links:updated`. Путь целиком, как из окна:
/// запрос записи ввода (`links:flush-requested`, окно сразу отвечает
/// `links_flush_done`), план, переименование на диске, переписывание
/// заметок с журналом (`relink::run`), событие. Предел — по медиане 5
/// повторов. Каждый повтор — новая копия одного хранилища генератора
/// (зерно 7, `index.json` как после прошлого запуска), новое приложение и
/// окно: после первого переноса ссылки в папку уже короткие, и повтор «туда
/// и обратно» переписывать было бы нечего.
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_relink_rename_move_via_commands() {
    for notes in scales() {
        let checked = notes == CHECKED_NOTES;
        let folder_limit = if notes > CHECKED_NOTES {
            20_000.0
        } else {
            15_000.0
        };
        let mut report = Report::new(&format!(
            "perf-core переименование с обновлением ссылок через команды, {notes} заметок"
        ));
        let dir = tempfile::tempdir().expect("папка");
        let pristine = dir.path().join("pristine");
        let (gen_ms, manifest) = timed(|| {
            gen_vault::generate(&pristine, &gen_vault::Config::new(notes, 7)).expect("генератор")
        });
        std::fs::create_dir_all(pristine.join("Перенесено")).expect("папка");
        write_index_json(&pristine);
        report.note(format!("  генератор: {gen_ms:.0} мс"));
        let hub = manifest.hub.as_ref().expect("узел").path.clone();
        let hub: Vec<String> = hub.split('/').map(str::to_owned).collect();
        let hub_new = hub.last().expect("имя").replace(".md", " переименован.md");
        let folder = manifest.folder_1000.as_ref().expect("папка 1 000");

        let (mut hub_took, mut folder_took) = (Vec::new(), Vec::new());
        let mut hub_counts = Vec::new();
        let mut folder_counts = Vec::new();
        for repeat in 0..REPEATS {
            let root = dir.path().join(format!("vault-{repeat}"));
            copy_dir(&pristine, &root);
            let app = app();
            let label = format!("perf-relink-{notes}-{repeat}");
            let w = window(&app, &label);
            let core = app.state::<Core>();
            core.open_vault_with(
                &label,
                VaultSummary {
                    id: VaultId(label.clone()),
                    name: label.clone(),
                    path: root.display().to_string(),
                },
                &root,
                Arc::new(WindowEmitter::new(app.handle().clone(), label.as_str())),
                None,
            )
            .expect("хранилище открыто");
            wait_index_ready(&w, Duration::from_secs(300));

            // Окно сразу отвечает «ввод записан» (из другого потока:
            // слушатель вызывается в потоке команды).
            let flusher = w.clone();
            w.listen(event::LINKS_FLUSH_REQUESTED, move |e| {
                let payload: Value = serde_json::from_str(e.payload()).expect("данные");
                let window = flusher.clone();
                std::thread::spawn(move || {
                    invoke(
                        &window,
                        "links_flush_done",
                        json!({ "request": { "token": payload["token"], "saved": true } }),
                    );
                });
            });
            let (updated_tx, updated_rx) = std::sync::mpsc::channel::<(Instant, Value)>();
            w.listen(event::LINKS_UPDATED, move |e| {
                let payload = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
                let _ = updated_tx.send((Instant::now(), payload));
            });
            // Ответ пользователя (вызов команды) → `links:updated`: время в
            // мс; переписаны все заметки плана, сбоев нет.
            let answer = |what: &str, command: &str, request: Value, expected: u32| -> f64 {
                let start = Instant::now();
                let reply = invoke(&w, command, json!({ "request": request }));
                let (at, payload) = updated_rx
                    .recv_timeout(Duration::from_secs(300))
                    .unwrap_or_else(|_| panic!("{what}: нет links:updated, ответ {reply}"));
                assert_eq!(payload["updated"], json!(expected), "{what}: {payload}");
                assert_eq!(payload["failed"], json!([]), "{what}: {payload}");
                ms(at - start)
            };

            let plan = relink_preview(
                &w,
                json!({ "op": "rename", "path": hub, "new_name": hub_new }),
            );
            hub_counts.push((plan.notes, plan.links));
            hub_took.push(answer(
                "узел",
                "entry_rename",
                json!({ "path": hub, "new_name": hub_new, "links": "update", "token": plan.token }),
                plan.notes,
            ));

            let request = json!({ "op": "move", "path": [folder.path], "new_dir": ["Перенесено"] });
            let plan = relink_preview(&w, request);
            folder_counts.push((plan.notes, plan.links));
            folder_took.push(answer(
                "папка",
                "entry_move",
                json!({
                    "path": [folder.path],
                    "new_dir": ["Перенесено"],
                    "links": "update",
                    "token": plan.token,
                }),
                plan.notes,
            ));
            core.unbind_window(&label);
        }
        assert!(
            hub_counts.windows(2).all(|p| p[0] == p[1]),
            "{hub_counts:?}"
        );
        assert!(
            folder_counts.windows(2).all(|p| p[0] == p[1]),
            "{folder_counts:?}"
        );
        if notes >= 1_000 {
            assert_eq!(hub_counts[0].0, 100, "заметок, ссылающихся на «узел»");
        }
        report.measure(
            &format!(
                "переименование «узла» ({} заметок, {} ссылок): ответ → links:updated",
                hub_counts[0].0, hub_counts[0].1
            ),
            &hub_took,
            "мс",
            Some(2_000.0),
            checked,
        );
        report.note(format!(
            "  папка «{}»: {} заметок; входящих ссылок по генератору {} (с именем папки {})",
            folder.path, folder.notes, folder.incoming_links, folder.incoming_with_folder_name
        ));
        report.measure(
            &format!(
                "перенос папки на 1 000 заметок ({} заметок, {} ссылок в плане): ответ → links:updated",
                folder_counts[0].0, folder_counts[0].1
            ),
            &folder_took,
            "мс",
            Some(folder_limit),
            checked,
        );
        report.finish();
    }
}
