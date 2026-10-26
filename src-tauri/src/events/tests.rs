//! Перевод сообщений наблюдателя в события окна — с подменным отправителем.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::commands::{EntryKind, VaultId, VaultSummary};
use crate::vault::Vault;

/// Отправитель, который запоминает события.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl Emit for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0
            .lock()
            .expect("журнал")
            .push((event.to_owned(), payload));
    }
}

impl Recorder {
    fn take(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.0.lock().expect("журнал"))
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    open: Arc<OpenVault>,
    recorder: Arc<Recorder>,
    events: VaultEvents,
}

fn fixture(files: &[&str]) -> Fixture {
    let dir = tempfile::tempdir().expect("папка");
    for rel in files {
        let path = rel
            .split('/')
            .fold(dir.path().to_path_buf(), |p, c| p.join(c));
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
        std::fs::write(&path, rel.as_bytes()).expect("файл");
    }
    let open = Arc::new(
        OpenVault::new(
            VaultSummary {
                id: VaultId("t".into()),
                name: "t".into(),
                path: String::new(),
            },
            Vault::open(dir.path()).expect("хранилище"),
        )
        .expect("заметки"),
    );
    let recorder = Arc::new(Recorder::default());
    let events = VaultEvents::new(Arc::clone(&open), recorder.clone());
    Fixture {
        dir,
        open,
        recorder,
        events,
    }
}

fn vp(rel: &str) -> VaultPath {
    VaultPath::from_components(rel.split('/'))
}

fn changed(changes: Vec<TreeChange>) -> WatcherEvent {
    WatcherEvent::Changed(VaultChangedPayload {
        rescan: false,
        changes,
    })
}

#[test]
fn unavailable_and_available_become_availability_events() {
    let f = fixture(&[]);
    f.events.handle(WatcherEvent::Unavailable);
    f.events.handle(WatcherEvent::DeliveryFailed);
    f.events.handle(WatcherEvent::Available);
    assert_eq!(
        f.recorder.take(),
        vec![
            (
                "vault:availability".to_owned(),
                json!({ "available": false })
            ),
            (
                "vault:availability".to_owned(),
                json!({ "available": true })
            ),
        ]
    );
}

#[test]
fn removed_folder_closes_every_note_under_it() {
    let f = fixture(&["Архив/1.md", "Архив/Глубже/2.md", "Другое.md"]);
    std::fs::remove_dir_all(f.dir.path().join("Архив")).expect("удаление");

    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Архив")
    }]));

    let events = f.recorder.take();
    assert_eq!(events[0].0, "vault:changed");
    assert_eq!(
        events[1..].to_vec(),
        vec![
            (
                "note:deleted".to_owned(),
                json!({ "path": ["Архив", "1.md"] })
            ),
            (
                "note:deleted".to_owned(),
                json!({ "path": ["Архив", "Глубже", "2.md"] })
            ),
        ]
    );
    assert!(!f.open.read_index().contains(&vp("Архив/1.md")));
}

#[test]
fn renamed_folder_renames_every_note_under_it_and_repeat_is_ignored() {
    let f = fixture(&["А/1.md", "А/Б/2.md"]);
    std::fs::rename(f.dir.path().join("А"), f.dir.path().join("В")).expect("перенос");
    let rename = || {
        changed(vec![TreeChange::Renamed {
            from: vp("А"),
            to: vp("В"),
        }])
    };

    f.events.handle(rename());
    let events = f.recorder.take();
    assert_eq!(
        events[1..].to_vec(),
        vec![
            (
                "note:renamed".to_owned(),
                json!({ "from": ["А", "1.md"], "to": ["В", "1.md"] })
            ),
            (
                "note:renamed".to_owned(),
                json!({ "from": ["А", "Б", "2.md"], "to": ["В", "Б", "2.md"] })
            ),
        ]
    );

    // Повтор (пачка после сверки, гонка с командой): `from` уже неизвестен —
    // только `vault:changed`, без `note:renamed` и без ошибки.
    f.events.handle(rename());
    let events = f.recorder.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "vault:changed");
    assert!(f.open.read_index().contains(&vp("В/Б/2.md")));
}

#[test]
fn created_existing_note_is_not_an_error() {
    let f = fixture(&["Есть.md"]);
    let created = || {
        changed(vec![TreeChange::Created {
            path: vp("Есть.md"),
            entry_kind: EntryKind::Note,
        }])
    };
    f.events.handle(created());
    f.events.handle(created());
    let events = f.recorder.take();
    assert_eq!(events.len(), 2, "только vault:changed: {events:?}");
    assert!(events.iter().all(|(name, _)| name == "vault:changed"));
}

#[test]
fn modified_note_gets_note_changed_with_disk_version() {
    let f = fixture(&["Заметка.md"]);
    std::fs::write(f.dir.path().join("Заметка.md"), "снаружи").expect("правка");

    f.events.handle(changed(vec![TreeChange::Modified {
        path: vp("Заметка.md"),
    }]));

    let events = f.recorder.take();
    assert_eq!(events[1].0, "note:changed");
    assert_eq!(events[1].1["path"], json!(["Заметка.md"]));
    assert_eq!(
        events[1].1["version"]["hash"],
        json!(blake3::hash("снаружи".as_bytes()).to_hex().to_string())
    );
    assert_eq!(events[1].1["version"]["size"], json!("снаружи".len()));
}

#[test]
fn new_note_from_outside_becomes_resolvable() {
    let f = fixture(&["Дневник.md"]);
    std::fs::write(f.dir.path().join("Новая.md"), "").expect("файл");

    f.events.handle(changed(vec![TreeChange::Created {
        path: vp("Новая.md"),
        entry_kind: EntryKind::Note,
    }]));

    assert_eq!(
        f.open
            .read_index()
            .links()
            .resolve(&vp("Дневник.md"), "Новая")
            .expect("разрешение"),
        Some(vp("Новая.md"))
    );
}
