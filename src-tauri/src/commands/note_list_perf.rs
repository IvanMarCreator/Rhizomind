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
//!
//! Запуск: `cargo test -p rhizomind --release --lib -- --ignored --nocapture --test-threads=1 perf_`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, CapabilityBuilder, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

use super::*;
use crate::events::WindowEmitter;
use crate::index::perf_tests::{CHECKED_NOTES, REPEATS, Report, median, scales, timed};
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
