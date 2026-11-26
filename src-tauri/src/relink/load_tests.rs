//! Настоящие события файловой системы → команды, файлы и индекс (0.2-8.2).
//! 1000 заметок в папке и 25 источников по 40 ссылок. Штатные задержки
//! наблюдателя и затишье повторов не сокращаются; сроки ниже — только
//! предохранитель от зависания, а не проверка скорости продукта.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::test::MockRuntime;
use tauri::{App, Manager, WebviewWindow};

use crate::commands::core_tests::{
    app, call, flush_ok, indexed_vault, preview_rename, record_events, rename_update,
};
use crate::commands::{Core, NoteList, OpenVault, VaultPath, event};
use crate::index::{VaultIndex, rebuild};

const NOTES: usize = 1000;
const SOURCES: usize = 25;
const WAIT: Duration = Duration::from_secs(60);
// Дольше штатных 2 с затишья повторов и 250+150 мс склейки наблюдателя.
const QUIET: Duration = Duration::from_secs(3);
type Events = Arc<Mutex<Vec<(String, Value)>>>;

struct Watched {
    app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    open: Arc<OpenVault>,
    seen: Events,
    writes: Arc<Mutex<Vec<VaultPath>>>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn vp(rel: &str) -> VaultPath {
    VaultPath::from_components(rel.split('/'))
}

fn references(source: usize, prefix: &str) -> String {
    let mut text = String::from("# References\r\n#load/refs\r\n");
    for n in source * 40..(source + 1) * 40 {
        text.push_str(&format!(
            "before [[{prefix}N{n:04}#Heading|alias]] after\r\n"
        ));
    }
    text
}

impl Watched {
    fn new(label: &str, prefix: &str) -> Self {
        let files: Vec<(String, String)> = (0..NOTES)
            .map(|n| {
                (
                    format!("Original/N{n:04}.md"),
                    format!("# Heading\n#load/old\nnote {n}\n"),
                )
            })
            .chain((0..SOURCES).map(|n| (format!("Ref{n:02}.md"), references(n, prefix))))
            .collect();
        let files: Vec<_> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let app = app();
        // Core::open_vault_with запускает настоящий VaultWatcher::start_with,
        // общий OwnWrites и тот же VaultEvents, что окно приложения.
        let (window, dir, root) = indexed_vault(&app, label, &files);
        let open = app.state::<Core>().require_vault(label).expect("хранилище");
        flush_ok(&window);
        let seen = record_events(
            &window,
            &[
                event::VAULT_CHANGED,
                event::INDEX_CHANGED,
                event::NOTE_CHANGED,
                event::NOTE_RENAMED,
                event::LINKS_PENDING,
                event::LINKS_UPDATED,
                event::LINKS_FLUSH_REQUESTED,
                event::LINKS_PROGRESS,
            ],
        );
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        open.vault.own_writes().observe(Box::new(move |path, _, _| {
            recorded.lock().expect("свои записи").push(path.clone());
        }));
        Self {
            app,
            window,
            open,
            seen,
            writes,
            root,
            _dir: dir,
        }
    }

    fn events(&self, name: &str) -> Vec<Value> {
        self.seen
            .lock()
            .expect("события")
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .collect()
    }

    fn tree_trace(&self) -> Vec<Value> {
        self.events(event::VAULT_CHANGED).iter().map(|p| {
            let changes = p["changes"].as_array().expect("изменения");
            json!({
                "rescan": p["rescan"], "changes": changes.len(),
                "renamed": changes.iter().filter(|c| c["kind"] == "renamed").take(5).collect::<Vec<_>>(),
                "created": changes.iter().filter(|c| c["kind"] == "created").count(),
                "removed": changes.iter().filter(|c| c["kind"] == "removed").count(),
            })
        }).collect()
    }
    fn until(&self, why: &str, check: impl Fn() -> bool) {
        let start = Instant::now();
        while !check() {
            assert!(
                start.elapsed() < WAIT,
                "{why}; pending={:?}, updated={:?}, tree={:?}, renamed_notes={}",
                self.events(event::LINKS_PENDING),
                self.events(event::LINKS_UPDATED),
                self.tree_trace(),
                self.events(event::NOTE_RENAMED).len()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_event(&self, name: &str) -> Value {
        self.until(name, || !self.events(name).is_empty());
        self.events(name)[0].clone()
    }

    fn quiet(&self) {
        let mut count = self.seen.lock().expect("события").len();
        let mut since = Instant::now();
        let start = Instant::now();
        loop {
            let now = self.seen.lock().expect("события").len();
            if now != count || self.open.relink.queue_busy() {
                count = now;
                since = Instant::now();
            }
            if since.elapsed() >= QUIET {
                break;
            }
            assert!(start.elapsed() < WAIT, "события не затихли");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn answer(&self, pending: &Value) {
        call(
            &self.window,
            "relink_answer",
            json!({"token": pending["token"], "apply": true}),
        )
        .expect("подтверждение плана");
    }

    fn text(&self, path: &str) -> String {
        std::fs::read_to_string(self.root.join(path)).expect("текст заметки")
    }

    fn assert_refs(&self, prefix: &str) {
        for n in 0..SOURCES {
            assert_eq!(self.text(&format!("Ref{n:02}.md")), references(n, prefix));
        }
    }

    fn assert_one_update(&self, reason: &str, pending_count: usize) {
        self.quiet();
        let updated = self.events(event::LINKS_UPDATED);
        assert_eq!(updated.len(), 1, "без второй волны: {updated:?}");
        assert_eq!(updated[0]["reason"], reason);
        assert_eq!(updated[0]["updated"], SOURCES);
        assert_eq!(updated[0]["failed"], json!([]));
        assert_eq!(updated[0]["retry_token"], Value::Null);
        assert_eq!(self.events(event::LINKS_PENDING).len(), pending_count);
        assert_eq!(self.events(event::LINKS_FLUSH_REQUESTED).len(), 1);
        let writes = self.writes.lock().expect("свои записи").clone();
        assert_eq!(writes.len(), SOURCES, "нет скрытых повторных записей");
        for n in 0..SOURCES {
            assert_eq!(
                writes
                    .iter()
                    .filter(|p| **p == vp(&format!("Ref{n:02}.md")))
                    .count(),
                1
            );
        }
        let changed = self.events(event::NOTE_CHANGED);
        assert_eq!(changed.len(), SOURCES, "каждая запись ровно один раз");
        for n in 0..SOURCES {
            assert_eq!(
                changed
                    .iter()
                    .filter(|p| p["path"] == json!([format!("Ref{n:02}.md")]))
                    .count(),
                1
            );
        }
    }

    fn assert_rebuilt(&self, count: usize) {
        let rebuilt = RwLock::new(VaultIndex::new());
        rebuild(&self.open.vault, &rebuilt, &|_| {}).expect("пересборка");
        let rebuilt = rebuilt.read().expect("пересобранный индекс");
        let disk = NoteList::new(self.open.vault.notes().expect("заметки на диске"));
        assert_eq!(disk.note_refs().len(), count);
        let entries = |index: &VaultIndex| {
            index
                .entries()
                .map(|(p, e)| (p.components().to_vec(), e.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        self.until("индекс должен догнать диск", || {
            self.open
                .index
                .with_index(|i| entries(i) == entries(&rebuilt))
        });
        let live = self.open.read_notes();
        assert_eq!(live.note_refs(), disk.note_refs());
        self.open.index.with_index(|index| {
            assert_eq!(entries(index), entries(&rebuilt));
            let tags = |i: &VaultIndex| {
                i.tags()
                    .into_iter()
                    .map(|t| (t.key.clone(), t))
                    .collect::<BTreeMap<_, _>>()
            };
            assert_eq!(tags(index), tags(&rebuilt));
            for tag in rebuilt.tags() {
                assert_eq!(index.tag_notes(&tag.key), rebuilt.tag_notes(&tag.key));
            }
            for note in disk.note_refs() {
                assert_eq!(
                    index.backlinks(&note.path, live.links()),
                    rebuilt.backlinks(&note.path, disk.links()),
                    "обратные ссылки {:?}",
                    note.path
                );
            }
        });
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.app.state::<Core>().unbind_window(self.window.label());
    }
}

#[test]
fn copy_folder_1000_with_duplicates_one_plan_no_second_wave() {
    let w = Watched::new("load-copy", "");
    std::fs::create_dir(w.root.join("Copy")).expect("папка копии");
    // Именно поток копирования в наблюдаемую папку: не rename готовой папки.
    for n in 0..NOTES {
        std::fs::copy(
            w.root.join(format!("Original/N{n:04}.md")),
            w.root.join(format!("Copy/N{n:04}.md")),
        )
        .expect("копирование");
    }
    let pending = w.wait_event(event::LINKS_PENDING);
    assert_eq!(pending["reason"], "duplicate");
    assert_eq!(pending["notes"], SOURCES);
    assert_eq!(pending["links"], NOTES);
    assert_eq!(pending["renames_total"], 0);
    w.answer(&pending);
    w.wait_event(event::LINKS_UPDATED);
    w.assert_refs("Original/");
    w.assert_one_update("duplicate", 1);
    assert!(
        !w.events(event::VAULT_CHANGED).is_empty(),
        "наблюдатель доставил изменения"
    );
    for n in 0..NOTES {
        assert_eq!(
            w.text(&format!("Copy/N{n:04}.md")),
            w.text(&format!("Original/N{n:04}.md"))
        );
    }
    w.assert_rebuilt(NOTES * 2 + SOURCES);
}
#[test]
fn move_folder_1000_external_confirmed_and_relinked() {
    let w = Watched::new("load-move", "Original/");
    std::fs::rename(w.root.join("Original"), w.root.join("Moved")).expect("перенос в Проводнике");
    let pending = w.wait_event(event::LINKS_PENDING);
    assert_eq!(pending["reason"], "external_rename");
    assert_eq!(pending["notes"], SOURCES);
    assert_eq!(pending["links"], NOTES);
    assert_eq!(pending["renames_total"], 1);
    assert_eq!(
        pending["renames"],
        json!([{"from": ["Original"], "to": ["Moved"]}])
    );
    // Только настоящий watcher может подтвердить идентичность файлов.
    w.answer(&pending);
    w.wait_event(event::LINKS_UPDATED);
    w.assert_refs("");
    w.assert_one_update("external_rename", 1);
    let renamed = w.events(event::NOTE_RENAMED);
    assert_eq!(renamed.len(), NOTES);
    for n in 0..NOTES {
        assert_eq!(
            renamed
                .iter()
                .filter(|p| p["from"] == json!(["Original", format!("N{n:04}.md")])
                    && p["to"] == json!(["Moved", format!("N{n:04}.md")]))
                .count(),
            1
        );
        assert_eq!(
            w.text(&format!("Moved/N{n:04}.md")),
            format!("# Heading\n#load/old\nnote {n}\n")
        );
        assert!(!w.root.join(format!("Original/N{n:04}.md")).exists());
    }
    w.assert_rebuilt(NOTES + SOURCES);
    let live = w.open.read_notes();
    assert_eq!(
        w.open
            .index
            .with_index(|i| i.backlinks(&vp("Moved/N0000.md"), live.links()).len()),
        1
    );
}
#[test]
fn git_checkout_1000_replaced_no_rewrites() {
    let w = Watched::new("load-checkout", "Original/");
    let stage = w._dir.path().join("checkout-staging");
    std::fs::create_dir(&stage).expect("подготовка checkout");
    let replacement = |n: usize| {
        format!(
            "# Replacement {n}\n#load/new\n[[N{:04}|checkout]]\n",
            (n + 1) % NOTES
        )
    };
    // Новые файлы создаются до удаления старых: ОС не может повторно
    // выдать им идентичность только что удалённых файлов.
    for n in 0..NOTES {
        std::fs::write(stage.join(format!("N{n:04}.md")), replacement(n)).expect("новая версия");
    }
    // Как checkout: замена каждого файла другим, с тем же именем.
    for n in 0..NOTES {
        let path = w.root.join(format!("Original/N{n:04}.md"));
        std::fs::remove_file(&path).expect("старая версия удалена");
        std::fs::rename(stage.join(format!("N{n:04}.md")), path).expect("новая версия установлена");
    }
    w.until(
        "наблюдатель увидел новые тексты",
        || {
            w.open.index.with_index(|i| {
                i.tag_notes("load/new").len() == NOTES && i.tag_notes("load/old").is_empty()
            })
        },
    );
    w.quiet();
    assert!(
        !w.events(event::VAULT_CHANGED).is_empty(),
        "есть настоящая доставка watcher"
    );
    for name in [
        event::LINKS_PENDING,
        event::LINKS_UPDATED,
        event::LINKS_FLUSH_REQUESTED,
        event::LINKS_PROGRESS,
    ] {
        assert!(
            w.events(name).is_empty(),
            "checkout не запускает обновления: {name}"
        );
    }
    w.assert_refs("Original/");
    for n in 0..NOTES {
        assert_eq!(
            w.text(&format!("Original/N{n:04}.md")),
            replacement(n),
            "файл checkout не переписан"
        );
    }
    assert!(
        w.writes.lock().expect("свои записи").is_empty(),
        "checkout не вызвал ни одной записи ядра"
    );
    assert!(
        w.events(event::NOTE_RENAMED).is_empty(),
        "новый файл — не подтверждённое переименование"
    );
    w.assert_rebuilt(NOTES + SOURCES);
}
#[test]
fn own_renames_not_processed_twice_under_load() {
    let w = Watched::new("load-own", "Original/");
    let plan = preview_rename(&w.window, &["Original"], "Own");
    assert_eq!((plan.notes, plan.links), (25, 1000));
    let reply =
        rename_update(&w.window, &["Original"], "Own", &plan.token).expect("своё переименование");
    assert_eq!(reply["relink"]["updated"], SOURCES);
    assert_eq!(reply["relink"]["failed"], json!([]));
    w.until(
        "наблюдатель доставил своё переименование",
        || {
            w.events(event::VAULT_CHANGED).iter().any(|p| {
                p["changes"]
                    .as_array()
                    .expect("изменения")
                    .iter()
                    .any(|c| c["from"] == json!(["Original"]) && c["to"] == json!(["Own"]))
            })
        },
    );
    w.assert_refs("");
    w.assert_one_update("rename", 0);
    for n in 0..NOTES {
        assert!(!w.root.join(format!("Original/N{n:04}.md")).exists());
        assert_eq!(
            w.text(&format!("Own/N{n:04}.md")),
            format!("# Heading\n#load/old\nnote {n}\n")
        );
        assert!(
            w.open
                .read_notes()
                .note(&vp(&format!("Own/N{n:04}.md")))
                .is_some()
        );
    }
    w.assert_rebuilt(NOTES + SOURCES);
}
