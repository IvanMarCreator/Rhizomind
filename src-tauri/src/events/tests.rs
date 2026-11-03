//! Перевод сообщений наблюдателя в события окна — с подменным отправителем.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::commands::{EntryKind, VaultId, VaultSummary};
use crate::vault::Vault;

/// Отправитель, который запоминает события.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

/// Запись журнала [`Recorder`] о смене заголовка окна (не событие).
const TITLE: &str = "<заголовок окна>";

impl Emit for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0
            .lock()
            .expect("журнал")
            .push((event.to_owned(), payload));
    }

    fn set_title(&self, title: &str) {
        self.0
            .lock()
            .expect("журнал")
            .push((TITLE.to_owned(), json!(title)));
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
    assert!(!f.open.read_notes().contains(&vp("Архив/1.md")));
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
    assert!(f.open.read_notes().contains(&vp("В/Б/2.md")));
}

#[test]
fn note_renamed_to_a_non_note_is_deleted_for_the_window() {
    // Решение оркестратора 90: `a.md` → `a.txt` — заметка ушла из заметок,
    // как удаление; новый путь в списке заметок не держится.
    let f = fixture(&["Папка/a.md", "b.md"]);
    std::fs::rename(
        f.dir.path().join("Папка").join("a.md"),
        f.dir.path().join("Папка").join("a.txt"),
    )
    .expect("переименование");

    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Папка/a.md"),
        to: vp("Папка/a.txt"),
    }]));

    let events = f.recorder.take();
    assert_eq!(events[0].0, "vault:changed");
    assert_eq!(
        events[1..].to_vec(),
        vec![(
            "note:deleted".to_owned(),
            json!({ "path": ["Папка", "a.md"] })
        )]
    );
    let index = f.open.read_notes();
    assert!(!index.contains(&vp("Папка/a.txt")));
    assert!(!index.contains(&vp("Папка/a.md")));
    assert!(index.contains(&vp("b.md")));
}

#[test]
fn non_note_renamed_back_to_a_note_becomes_a_note_again() {
    let f = fixture(&["a.txt"]);
    std::fs::rename(f.dir.path().join("a.txt"), f.dir.path().join("a.md")).expect("переименование");

    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("a.txt"),
        to: vp("a.md"),
    }]));

    let events = f.recorder.take();
    assert_eq!(events.len(), 1, "только vault:changed: {events:?}");
    assert!(f.open.read_notes().contains(&vp("a.md")));
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

/// Задача 1.9, решение 89: переименована и изменена в одной склейке —
/// окну сначала `note:renamed`, потом `note:changed` по новому пути.
#[test]
fn renamed_and_modified_note_gets_note_renamed_then_note_changed() {
    let f = fixture(&["a.md"]);
    std::fs::create_dir(f.dir.path().join("Архив")).expect("папка");
    std::fs::rename(
        f.dir.path().join("a.md"),
        f.dir.path().join("Архив").join("b.md"),
    )
    .expect("перенос");
    std::fs::write(f.dir.path().join("Архив").join("b.md"), "снаружи").expect("правка");

    f.events.handle(changed(vec![
        TreeChange::Renamed {
            from: vp("a.md"),
            to: vp("Архив/b.md"),
        },
        TreeChange::Modified {
            path: vp("Архив/b.md"),
        },
    ]));

    let events = f.recorder.take();
    let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["vault:changed", "note:renamed", "note:changed"]);
    assert_eq!(
        events[1].1,
        json!({ "from": ["a.md"], "to": ["Архив", "b.md"] })
    );
    assert_eq!(events[2].1["path"], json!(["Архив", "b.md"]));
    assert_eq!(
        events[2].1["version"]["hash"],
        json!(blake3::hash("снаружи".as_bytes()).to_hex().to_string())
    );
    assert!(f.open.read_notes().contains(&vp("Архив/b.md")));
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
            .read_notes()
            .links()
            .resolve(&vp("Дневник.md"), "Новая")
            .expect("разрешение"),
        Some(vp("Новая.md"))
    );
}

// ---- Заголовок окна следует за показанной заметкой (задача 2.12, решение 82) ----

fn app_title() -> String {
    crate::main_window_title()
}

/// Заголовки окна из журнала.
fn titles(recorder: &Recorder) -> Vec<String> {
    recorder
        .take()
        .into_iter()
        .filter(|(name, _)| name == TITLE)
        .map(|(_, title)| title.as_str().expect("строка").to_owned())
        .collect()
}

/// Показать заметку и вернуть поставленный заголовок.
fn show(f: &Fixture, path: Option<&str>) -> Result<Vec<String>, crate::commands::CoreError> {
    let set = Mutex::new(Vec::new());
    f.open.show_note(path.map(vp), &|title: &str| {
        set.lock().expect("журнал").push(title.to_owned());
    })?;
    Ok(set.into_inner().expect("журнал"))
}

#[test]
fn shown_note_sets_window_title_from_path() {
    let f = fixture(&["Архив/Итоги года.md"]);
    assert_eq!(
        show(&f, Some("Архив/Итоги года.md")).expect("заметка"),
        vec![format!("Итоги года — t — {}", app_title())]
    );
    assert_eq!(
        show(&f, None).expect("без заметки"),
        vec![format!("t — {}", app_title())]
    );
}

#[test]
fn shown_note_must_be_a_note_of_this_vault() {
    use crate::commands::ErrorCode;
    let f = fixture(&["Архив/Итоги года.md", "Архив/схема.png"]);
    show(&f, Some("Архив/Итоги года.md")).expect("заметка");
    for (path, code) in [
        ("Архив", ErrorCode::NotANote),
        ("Архив/схема.png", ErrorCode::NotANote),
        ("Нет такой.md", ErrorCode::NotFound),
        ("Архив/Только в другом хранилище.md", ErrorCode::NotFound),
    ] {
        let error = show(&f, Some(path)).expect_err(path);
        assert_eq!(error.code, code, "{path}");
    }
    // Отказ не меняет показанную заметку: переименование снаружи ведёт её.
    std::fs::rename(
        f.dir.path().join("Архив").join("Итоги года.md"),
        f.dir.path().join("Архив").join("Итоги.md"),
    )
    .expect("переименование");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Архив/Итоги года.md"),
        to: vp("Архив/Итоги.md"),
    }]));
    assert_eq!(
        titles(&f.recorder),
        vec![format!("Итоги — t — {}", app_title())]
    );
}

#[test]
fn renamed_from_outside_shown_note_and_its_folder_update_title() {
    let f = fixture(&["Архив/Итоги года.md", "Другое.md"]);
    show(&f, Some("Архив/Итоги года.md")).expect("заметка");

    // Чужая заметка — заголовок не трогается.
    std::fs::rename(
        f.dir.path().join("Другое.md"),
        f.dir.path().join("Прочее.md"),
    )
    .expect("переименование");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Другое.md"),
        to: vp("Прочее.md"),
    }]));
    assert!(titles(&f.recorder).is_empty());

    // Папка заметки — путь следует, имя то же: заголовок тот же, не ставится.
    std::fs::rename(f.dir.path().join("Архив"), f.dir.path().join("Старое"))
        .expect("переименование");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Архив"),
        to: vp("Старое"),
    }]));
    assert!(titles(&f.recorder).is_empty());

    // Сама заметка (по новому пути папки).
    std::fs::rename(
        f.dir.path().join("Старое").join("Итоги года.md"),
        f.dir.path().join("Старое").join("Итоги 2026.md"),
    )
    .expect("переименование");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Старое/Итоги года.md"),
        to: vp("Старое/Итоги 2026.md"),
    }]));
    assert_eq!(
        titles(&f.recorder),
        vec![format!("Итоги 2026 — t — {}", app_title())]
    );
}

#[test]
fn removed_from_outside_shown_note_leaves_vault_title() {
    let f = fixture(&["Архив/Итоги года.md", "Другое.md"]);
    show(&f, Some("Архив/Итоги года.md")).expect("заметка");
    std::fs::remove_file(f.dir.path().join("Другое.md")).expect("удаление");
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Другое.md"),
    }]));
    assert!(titles(&f.recorder).is_empty());

    std::fs::remove_dir_all(f.dir.path().join("Архив")).expect("удаление");
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Архив")
    }]));
    assert_eq!(titles(&f.recorder), vec![format!("t — {}", app_title())]);
    // Повтор — заметки уже нет, заголовок не ставится снова.
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Архив")
    }]));
    assert!(titles(&f.recorder).is_empty());
}

#[test]
fn rescan_without_shown_note_leaves_vault_title() {
    let f = fixture(&["Итоги года.md"]);
    show(&f, Some("Итоги года.md")).expect("заметка");
    std::fs::remove_file(f.dir.path().join("Итоги года.md")).expect("удаление");
    f.events.handle(WatcherEvent::Changed(VaultChangedPayload {
        rescan: true,
        changes: vec![],
    }));
    assert_eq!(titles(&f.recorder), vec![format!("t — {}", app_title())]);
}

#[test]
fn shown_note_replaced_in_place_keeps_title() {
    // Сохранение «через замену»: старый файл переименован в резервный,
    // новый записан на его место — заметка та же, заголовок не меняется.
    let f = fixture(&["Итоги года.md"]);
    show(&f, Some("Итоги года.md")).expect("заметка");
    std::fs::rename(
        f.dir.path().join("Итоги года.md"),
        f.dir.path().join("Итоги года.md~"),
    )
    .expect("в резервный");
    std::fs::write(f.dir.path().join("Итоги года.md"), b"new").expect("новый");
    f.events.handle(changed(vec![
        TreeChange::Renamed {
            from: vp("Итоги года.md"),
            to: vp("Итоги года.md~"),
        },
        TreeChange::Created {
            path: vp("Итоги года.md"),
            entry_kind: EntryKind::Note,
        },
    ]));
    assert_eq!(titles(&f.recorder), Vec::<String>::new());
}

#[test]
fn shown_note_name_is_spelled_as_in_the_vault() {
    // Путь от интерфейса сравнивается по ключу имени (регистр, нормализация),
    // а имя в заголовке — в написании из списка заметок, не из запроса.
    let f = fixture(&["Архив/Итоги года.md"]);
    assert_eq!(
        show(&f, Some("АРХИВ/ИТОГИ ГОДА.md")).expect("заметка"),
        vec![format!("Итоги года — t — {}", app_title())]
    );
    // Переименование снаружи находит заметку и по пути в написании хранилища.
    std::fs::rename(
        f.dir.path().join("Архив").join("Итоги года.md"),
        f.dir.path().join("Архив").join("Итоги.md"),
    )
    .expect("переименование");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Архив/Итоги года.md"),
        to: vp("Архив/Итоги.md"),
    }]));
    assert_eq!(
        titles(&f.recorder),
        vec![format!("Итоги — t — {}", app_title())]
    );
}

// ---- Индекс по пачкам наблюдателя (задача 0.2-1.4) -----------------------------

/// Отправитель, который запоминает события индекса со временем отправки.
#[derive(Default)]
struct Timed(Mutex<Vec<(String, std::time::Instant)>>);

impl Emit for Timed {
    fn emit(&self, event: &str, _payload: Value) {
        self.0
            .lock()
            .expect("журнал")
            .push((event.to_owned(), std::time::Instant::now()));
    }
}

impl Timed {
    fn times(&self, name: &str) -> Vec<std::time::Instant> {
        self.0
            .lock()
            .expect("журнал")
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, at)| *at)
            .collect()
    }
}

const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

fn wait_until(what: &str, check: impl Fn() -> bool) {
    let start = std::time::Instant::now();
    while !check() {
        assert!(start.elapsed() < WAIT, "не дождались: {what}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// [`fixture`] с запущенным индексом; события индекса — в [`Timed`].
fn indexed(files: &[(&str, &str)]) -> (Fixture, Arc<Timed>) {
    let f = fixture(&[]);
    for (rel, text) in files {
        write(&f, rel, text);
    }
    *f.open.write_notes() = crate::commands::NoteList::new(f.open.vault.notes().expect("заметки"));
    let timed = Arc::new(Timed::default());
    crate::commands::start_index(&f.open, timed.clone());
    wait_until("индекс готов", || {
        f.open.index.status().state == crate::commands::IndexState::Ready
    });
    (f, timed)
}

fn write(f: &Fixture, rel: &str, text: &str) {
    let path = rel
        .split('/')
        .fold(f.dir.path().to_path_buf(), |p, c| p.join(c));
    std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
    std::fs::write(&path, text).expect("файл");
}

fn backlinks_of(f: &Fixture, rel: &str) -> Vec<String> {
    let list = f.open.read_notes();
    f.open
        .index
        .backlinks_of(&list, &vp(rel))
        .iter()
        .map(|p| p.components().join("/"))
        .collect()
}

/// Заметку сохранили снаружи: после пачки наблюдателя индекс обновлён,
/// окну — `index:changed`.
#[test]
fn index_updated_after_external_save() {
    let (f, timed) = indexed(&[("Узел.md", ""), ("А.md", "без ссылок")]);
    assert!(backlinks_of(&f, "Узел.md").is_empty());
    let before = timed.times(crate::commands::event::INDEX_CHANGED).len();

    write(&f, "А.md", "теперь [[Узел]]");
    f.events
        .handle(changed(vec![TreeChange::Modified { path: vp("А.md") }]));
    assert_eq!(backlinks_of(&f, "Узел.md"), ["А.md"]);
    wait_until("index:changed", || {
        timed.times(crate::commands::event::INDEX_CHANGED).len() > before
    });

    // Новая заметка снаружи и удаление — тоже.
    write(&f, "Папка/Б.md", "![[Узел]]");
    f.events.handle(changed(vec![TreeChange::Created {
        path: vp("Папка/Б.md"),
        entry_kind: EntryKind::Note,
    }]));
    assert_eq!(backlinks_of(&f, "Узел.md"), ["А.md", "Папка/Б.md"]);
    std::fs::rename(f.dir.path().join("Папка"), f.dir.path().join("Новая")).expect("перенос");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Папка"),
        to: vp("Новая"),
    }]));
    assert_eq!(backlinks_of(&f, "Узел.md"), ["А.md", "Новая/Б.md"]);
    std::fs::remove_file(f.dir.path().join("А.md")).expect("удаление");
    f.events
        .handle(changed(vec![TreeChange::Removed { path: vp("А.md") }]));
    assert_eq!(backlinks_of(&f, "Узел.md"), ["Новая/Б.md"]);
}

/// Правка без ссылок и тегов `index:changed` не даёт; частые правки — не
/// чаще 4 событий в секунду, последняя всё равно доходит.
#[test]
fn index_changed_throttled_4_per_second() {
    let (f, timed) = indexed(&[("Узел.md", ""), ("А.md", "")]);
    let name = crate::commands::event::INDEX_CHANGED;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let before = timed.times(name).len();

    write(&f, "Узел.md", "просто текст без ссылок");
    f.events.handle(changed(vec![TreeChange::Modified {
        path: vp("Узел.md"),
    }]));
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(timed.times(name).len(), before, "правка без ссылок и тегов");

    let start = std::time::Instant::now();
    let mut last_change = start;
    for i in 0..30 {
        // Положение ссылки меняется каждый раз — каждая правка видна виду.
        write(&f, "А.md", &format!("{} [[Узел]]", "x".repeat(i)));
        f.events
            .handle(changed(vec![TreeChange::Modified { path: vp("А.md") }]));
        last_change = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    wait_until(
        "событие после последней правки",
        || {
            timed
                .times(name)
                .last()
                .is_some_and(|at| *at >= last_change)
        },
    );
    let times: Vec<_> = timed.times(name).into_iter().skip(before).collect();
    let interval = std::time::Duration::from_millis(1000 / 4);
    for pair in times.windows(2) {
        let gap = pair[1].duration_since(pair[0]);
        // Время пишется после отправки, а не в момент решения: запас на
        // задержку потока (медленные машины CI). Пять событий в секунду и
        // чаще всё равно не проходят.
        assert!(
            gap + std::time::Duration::from_millis(50) >= interval,
            "{gap:?}"
        );
    }
    let seconds = last_change.duration_since(start).as_secs_f64();
    assert!(
        times.len() as f64 <= seconds * 4.0 + 2.0,
        "{} событий за {seconds:.1} с",
        times.len()
    );
}

/// Своя запись обновляет индекс сразу; то же изменение от наблюдателя
/// (повтор) не даёт второго `index:changed`.
#[test]
fn own_write_updates_index_without_duplicate() {
    let (f, timed) = indexed(&[("Узел.md", ""), ("А.md", "")]);
    let name = crate::commands::event::INDEX_CHANGED;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let before = timed.times(name).len();

    let version = f.open.vault.file_version(&vp("А.md")).expect("версия");
    f.open
        .vault
        .write_note(&vp("А.md"), "своя [[Узел]]", &version)
        .expect("запись");
    // Так делает `note_write` после записи.
    f.open
        .apply_own(&[TreeChange::Modified { path: vp("А.md") }]);
    assert_eq!(backlinks_of(&f, "Узел.md"), ["А.md"]);

    // Повтор от наблюдателя.
    f.events
        .handle(changed(vec![TreeChange::Modified { path: vp("А.md") }]));
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert_eq!(timed.times(name).len(), before + 1);
    assert_eq!(backlinks_of(&f, "Узел.md"), ["А.md"]);
}
