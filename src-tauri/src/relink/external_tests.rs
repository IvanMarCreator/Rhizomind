//! Внешние переименования → обновление ссылок (задача 0.2-6.1): от пачки
//! наблюдателя (`VaultEvents`) до файлов на диске и событий окна.
//!
//! Переименование «в Проводнике» — переименование на диске и пачка
//! наблюдателя с `TreeChange::Renamed` (подтверждённое: наблюдатель узнал
//! тот же файл, ADR 0006). Сценарии с настоящим наблюдателем и `MoveFileExW`
//! — в конце файла, на Windows.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::commands::{
    OpenVault, RelinkAnswerRequest, RelinkToken, RenameLinks, TreeChange, VaultChangedPayload,
    VaultId, VaultPath, VaultSummary, event, start_index,
};
use crate::events::{Emit, VaultEvents};
use crate::own_writes::OwnWrites;
use crate::vault::{OpenOptions, Vault};
use crate::watcher::WatcherEvent;

/// Как окно отвечает на `links:flush-requested`.
const FLUSH_SAVED: u8 = 0;
const FLUSH_REFUSED: u8 = 1;
const FLUSH_SILENT: u8 = 2;

/// Окно хранилища в тестах: запоминает события и отвечает на запрос записи
/// ввода, как настоящее окно (`links_flush_done`).
#[derive(Default)]
struct TestWindow {
    events: Mutex<Vec<(String, Value)>>,
    open: OnceLock<Weak<OpenVault>>,
    flush: AtomicU8,
}

impl Emit for TestWindow {
    fn emit(&self, name: &str, payload: Value) {
        if name == event::LINKS_FLUSH_REQUESTED
            && self.flush.load(Ordering::SeqCst) != FLUSH_SILENT
            && let Some(open) = self.open.get().and_then(Weak::upgrade)
        {
            let token = RelinkToken(payload["token"].as_str().expect("знак").to_owned());
            let saved = self.flush.load(Ordering::SeqCst) == FLUSH_SAVED;
            assert!(open.relink.flush_done(&token, saved), "ответ принят");
        }
        self.events
            .lock()
            .expect("журнал")
            .push((name.to_owned(), payload));
    }
}

impl TestWindow {
    /// События обновления ссылок (без событий индекса и дерева).
    fn links_events(&self) -> Vec<(String, Value)> {
        self.events
            .lock()
            .expect("журнал")
            .iter()
            .filter(|(name, _)| name.starts_with("links:"))
            .cloned()
            .collect()
    }

    /// Ждёт событие `name` (не дольше 20 с) и возвращает его данные.
    fn wait(&self, name: &str) -> Value {
        let start = Instant::now();
        loop {
            if let Some((_, payload)) = self.links_events().into_iter().find(|(n, _)| n == name) {
                return payload;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "нет события {name}: {:?}",
                self.links_events()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// За `pause` не пришло ни одного события обновления ссылок.
    fn quiet_for(&self, pause: Duration) {
        std::thread::sleep(pause);
        assert_eq!(self.links_events(), Vec::<(String, Value)>::new());
    }

    fn clear(&self) {
        self.events.lock().expect("журнал").clear();
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    open: Arc<OpenVault>,
    window: Arc<TestWindow>,
    events: Arc<VaultEvents>,
}

const LABEL: &str = "окно-хранилища";

/// Хранилище с заметками `files` (путь, текст) и настройкой `mode`;
/// `ready` — индекс собран и сверен.
fn fixture(files: &[(&str, &str)], mode: RenameLinks, ready: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("папка");
    for (rel, text) in files {
        let path = rel
            .split('/')
            .fold(dir.path().to_path_buf(), |p, c| p.join(c));
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
        std::fs::write(&path, text).expect("файл");
    }
    let own = OwnWrites::new();
    let vault = Vault::open_with(
        dir.path(),
        OpenOptions {
            own_writes: own,
            ..Default::default()
        },
    )
    .expect("хранилище");
    let mut settings = vault.read_settings().expect("настройки");
    settings.rename_links = mode;
    vault.write_settings(&settings).expect("настройки");
    let open = Arc::new(
        OpenVault::new(
            VaultSummary {
                id: VaultId("t".into()),
                name: "t".into(),
                path: String::new(),
            },
            vault,
        )
        .expect("заметки"),
    );
    open.relink.bind_window(LABEL);
    let window = Arc::new(TestWindow::default());
    let _ = window.open.set(Arc::downgrade(&open));
    let events = Arc::new(VaultEvents::new(Arc::clone(&open), window.clone()));
    let fixture = Fixture {
        dir,
        open,
        window,
        events,
    };
    if ready {
        fixture.start_index();
    }
    fixture
}

impl Fixture {
    fn start_index(&self) {
        start_index(&self.open, self.window.clone());
        self.open.index.wait_settled();
    }

    fn abs(&self, rel: &str) -> std::path::PathBuf {
        rel.split('/')
            .fold(self.dir.path().to_path_buf(), |p, c| p.join(c))
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.abs(rel)).expect("заметка")
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.abs(rel), text).expect("запись");
    }

    /// Переименование «в Проводнике»: на диске и пачкой наблюдателя.
    fn rename_outside(&self, pairs: &[(&str, &str)]) {
        let mut changes = Vec::new();
        for (from, to) in pairs {
            std::fs::rename(self.abs(from), self.abs(to)).expect("переименование");
            changes.push(TreeChange::Renamed {
                from: vp(from),
                to: vp(to),
            });
        }
        self.events.handle(changed(changes));
    }

    fn answer(&self, token: &str, apply: bool) -> crate::commands::CoreResult<()> {
        crate::commands::relink_answer_blocking(
            &self.open,
            &*self.window,
            LABEL,
            RelinkAnswerRequest {
                token: RelinkToken(token.to_owned()),
                apply,
            },
        )
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

/// Имена событий по порядку, без хода переписывания (`links:progress`).
fn names(events: &[(String, Value)]) -> Vec<&str> {
    events
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != event::LINKS_PROGRESS)
        .collect()
}

const MEETING: (&str, &str) = ("Встреча.md", "# Встреча\n");
const LIST: (&str, &str) = (
    "Список.md",
    "См. [[Встреча]] и [[Встреча#Итоги|итоги]].\r\n",
);

/// «Всегда обновлять»: подтверждённое переименование снаружи — запись ввода
/// окна, затем переписывание и одно `links:updated` с именами.
#[test]
fn confirmed_rename_updates_links() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);

    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(updated["failed"], json!([]));
    assert_eq!(updated["retry_token"], Value::Null);
    assert_eq!(
        updated["renames"],
        json!([{ "from": ["Встреча.md"], "to": ["Планёрка.md"] }])
    );
    assert_eq!(updated["renames_total"], json!(1));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Планёрка]] и [[Планёрка#Итоги|итоги]].\r\n"
    );
    let events = f.window.links_events();
    assert_eq!(
        names(&events),
        vec![event::LINKS_FLUSH_REQUESTED, event::LINKS_UPDATED],
        "ввод записан до переписывания, вопроса нет"
    );
    let changed = f
        .window
        .events
        .lock()
        .expect("журнал")
        .iter()
        .any(|(name, payload)| {
            name == event::NOTE_CHANGED && payload["path"] == json!(["Список.md"])
        });
    assert!(changed, "note:changed для переписанной заметки");
}

/// «Удалили и создали» (Git, распаковка архива) — не переименование:
/// ссылки не трогаются, вопроса нет.
#[test]
fn delete_and_create_is_not_rename() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    std::fs::remove_file(f.abs("Встреча.md")).expect("удаление");
    f.write("Планёрка.md", MEETING.1);
    f.events.handle(changed(vec![
        TreeChange::Removed {
            path: vp("Встреча.md"),
        },
        TreeChange::Created {
            path: vp("Планёрка.md"),
            entry_kind: crate::commands::EntryKind::Note,
        },
    ]));
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Своё переименование (команда приложения, здесь — «без обновления»)
/// наблюдатель тоже видит: по журналу своих переименований оно не
/// обрабатывается второй раз — ни вопроса, ни переписывания.
#[test]
fn own_rename_not_processed_twice() {
    for mode in [RenameLinks::Update, RenameLinks::Ask] {
        let f = fixture(&[MEETING, LIST], mode, true);
        let to = f
            .open
            .vault
            .rename(&vp("Встреча.md"), "Планёрка.md")
            .expect("переименование");
        let event = TreeChange::Renamed {
            from: vp("Встреча.md"),
            to: to.clone(),
        };
        // Отбор пачки по журналу своих переименований: своё — не внешнее.
        assert_eq!(
            crate::relink::external::confirmed(
                std::slice::from_ref(&event),
                f.open.vault.own_writes()
            ),
            Vec::new(),
            "{mode:?}"
        );
        // Событие наблюдателя о том же переименовании — два раза (повтор
        // пачки) и раньше, чем команда учла его в списке заметок: по
        // списку заметок оно ещё выглядит переименованием заметки.
        for _ in 0..2 {
            f.events.handle(changed(vec![event.clone()]));
        }
        f.open.apply_own(&[event]);
        f.window.quiet_for(Duration::from_millis(400));
        assert_eq!(f.read("Список.md"), LIST.1, "{mode:?}");
    }
}

/// Гонка: событие наблюдателя о своём переименовании пришло раньше, чем
/// команда вернула ответ, — всё равно своё.
#[test]
fn own_rename_event_before_command_reply() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    let events = Arc::clone(&f.events);
    crate::vault::after_disk_rename(move || {
        events.handle(changed(vec![TreeChange::Renamed {
            from: vp("Встреча.md"),
            to: vp("Планёрка.md"),
        }]));
    });
    f.open
        .vault
        .rename(&vp("Встреча.md"), "Планёрка.md")
        .expect("переименование");
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// «Всегда спрашивать»: пачка из трёх переименований — одно `links:pending`
/// с тремя именами; файлы не меняются до ответа.
#[test]
fn batch_of_3_renames_one_pending() {
    let f = fixture(
        &[
            ("А.md", ""),
            ("Б.md", ""),
            ("В.md", ""),
            ("Список.md", "[[А]] [[Б]] [[В]] [[А]]\n"),
            ("Ещё.md", "[[Б]]\n"),
        ],
        RenameLinks::Ask,
        true,
    );
    f.rename_outside(&[("А.md", "А2.md"), ("Б.md", "Б2.md"), ("В.md", "В2.md")]);

    let pending = f.window.wait(event::LINKS_PENDING);
    std::thread::sleep(Duration::from_millis(300));
    let events = f.window.links_events();
    assert_eq!(
        names(&events),
        vec![event::LINKS_PENDING],
        "одно уведомление"
    );
    assert_eq!(pending["reason"], json!("external_rename"));
    assert_eq!(pending["notes"], json!(2));
    assert_eq!(pending["links"], json!(5));
    assert_eq!(pending["renames_total"], json!(3));
    assert_eq!(
        pending["renames"],
        json!([
            { "from": ["А.md"], "to": ["А2.md"] },
            { "from": ["Б.md"], "to": ["Б2.md"] },
            { "from": ["В.md"], "to": ["В2.md"] },
        ])
    );
    assert!(RelinkToken(pending["token"].as_str().expect("знак").to_owned()).is_well_formed());
    assert_eq!(f.read("Список.md"), "[[А]] [[Б]] [[В]] [[А]]\n");
    assert_eq!(f.read("Ещё.md"), "[[Б]]\n");
}

/// «Не обновлять» — ничего не переписано, записи ввода нет, знак сгорел.
#[test]
fn answer_keep_rewrites_nothing() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let pending = f.window.wait(event::LINKS_PENDING);
    let token = pending["token"].as_str().expect("знак").to_owned();
    f.window.clear();

    f.answer(&token, false).expect("ответ");
    f.window.quiet_for(Duration::from_millis(300));
    assert_eq!(f.read("Список.md"), LIST.1);
    assert!(f.answer(&token, true).is_err(), "знак одноразовый");
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Ответ пришёл, когда состав ссылок уже другой: план считается заново —
/// ссылка, добавленная после вопроса, тоже обновлена.
#[test]
fn answer_after_more_changes_replanned() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let pending = f.window.wait(event::LINKS_PENDING);
    assert_eq!(pending["notes"], json!(1));
    let token = pending["token"].as_str().expect("знак").to_owned();

    f.write("Ещё.md", "И ещё [[Встреча]].\n");
    f.events.handle(changed(vec![TreeChange::Created {
        path: vp("Ещё.md"),
        entry_kind: crate::commands::EntryKind::Note,
    }]));
    f.window.clear();

    f.answer(&token, true).expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(updated["updated"], json!(2));
    assert_eq!(
        names(&f.window.links_events()),
        vec![event::LINKS_FLUSH_REQUESTED, event::LINKS_UPDATED]
    );
    assert_eq!(
        f.read("Список.md"),
        "См. [[Планёрка]] и [[Планёрка#Итоги|итоги]].\r\n"
    );
    assert_eq!(f.read("Ещё.md"), "И ещё [[Планёрка]].\n");
}

/// Пока индекс строится, пачка ждёт: ни вопроса, ни записи; индекс готов —
/// ссылки обновлены.
#[test]
fn waits_for_index_ready() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, false);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);

    f.start_index();
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Планёрка]] и [[Планёрка#Итоги|итоги]].\r\n"
    );
}

/// Инвариант 4: «Всегда обновлять», а окно не записало ввод — ничего не
/// переписано; файл уже переименован снаружи, поэтому `links:updated` со
/// сбоем `busy` и знаком «Повторить»; повтор с записанным вводом обновляет.
#[test]
fn flush_refused_reports_busy_with_retry() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.window.flush.store(FLUSH_REFUSED, Ordering::SeqCst);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);

    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(updated["updated"], json!(0));
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "busy" }])
    );
    assert_eq!(
        updated["renames"],
        json!([{ "from": ["Встреча.md"], "to": ["Планёрка.md"] }])
    );
    assert_eq!(f.read("Список.md"), LIST.1);
    let retry = updated["retry_token"]
        .as_str()
        .expect("знак повтора")
        .to_owned();

    f.window.flush.store(FLUSH_SAVED, Ordering::SeqCst);
    f.window.clear();
    f.answer(&retry, true).expect("повтор");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Планёрка]] и [[Планёрка#Итоги|итоги]].\r\n"
    );
}

/// Окно не ответило на запрос записи ввода за предел — так же, как отказ.
#[test]
fn flush_timeout_reports_busy() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.window.flush.store(FLUSH_SILENT, Ordering::SeqCst);
    f.open.relink.set_flush_timeout(Duration::from_millis(100));
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "busy" }])
    );
    assert!(updated["retry_token"].is_string());
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// «Всегда спрашивать» и «Обновить ссылки», а ввод не записан — то же:
/// `busy` со знаком повтора, файлы не тронуты.
#[test]
fn answer_apply_flush_refused_reports_busy() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let token = f.window.wait(event::LINKS_PENDING)["token"]
        .as_str()
        .expect("знак")
        .to_owned();
    f.window.flush.store(FLUSH_REFUSED, Ordering::SeqCst);
    f.window.clear();
    f.answer(&token, true).expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "busy" }])
    );
    assert!(updated["retry_token"].is_string());
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Переписывать нечего (уникальное имя, перенос в другую папку — Р2) —
/// ни вопроса, ни записи ввода.
#[test]
fn nothing_to_rewrite_no_question() {
    let f = fixture(
        &[MEETING, LIST, ("Архив/Старое.md", "")],
        RenameLinks::Ask,
        true,
    );
    f.rename_outside(&[("Встреча.md", "Архив/Встреча.md")]);
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Переименование папки снаружи: ссылки с путём через неё обновлены.
#[test]
fn folder_rename_updates_path_links() {
    let f = fixture(
        &[
            ("Альфа/Встреча.md", ""),
            ("Бета/Встреча.md", ""),
            ("Список.md", "[[Альфа/Встреча]]\n"),
        ],
        RenameLinks::Update,
        true,
    );
    f.rename_outside(&[("Альфа", "Гамма")]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(f.read("Список.md"), "[[Гамма/Встреча]]\n");
}

/// Вопрос и ответ — только окну, которому выдан знак: знак `links:pending`
/// не годится для переименования, а знак чужого окна не принимается.
#[test]
fn pending_token_bound_to_window() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let token = f.window.wait(event::LINKS_PENDING)["token"]
        .as_str()
        .expect("знак")
        .to_owned();
    let foreign = crate::commands::relink_answer_blocking(
        &f.open,
        &*f.window,
        "чужое-окно",
        RelinkAnswerRequest {
            token: RelinkToken(token.clone()),
            apply: true,
        },
    );
    assert!(foreign.is_err());
    assert_eq!(f.read("Список.md"), LIST.1);
    // Знак сгорел, но вопрос не потерян: своё окно по сгоревшему знаку
    // получает свежий вопрос, а не обновление без вопроса.
    f.window.clear();
    f.answer(&token, true).expect("ответ");
    let fresh = f.window.wait(event::LINKS_PENDING);
    assert_ne!(fresh["token"], json!(token));
    assert_eq!(f.read("Список.md"), LIST.1);
}

fn token_of(payload: &Value, field: &str) -> String {
    payload[field].as_str().expect("знак").to_owned()
}

/// QA: две неотвеченные пачки «Всегда спрашивать» — А→Б, затем Б→В.
/// Вопрос один на всю цепочку: старый знак погашен, новый `links:pending`
/// с обоими переименованиями; ответ обновляет `[[А]]` сразу в `[[В]]`.
#[test]
fn qa_chain_two_unanswered_batches() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let first = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    f.window.clear();
    f.rename_outside(&[("Планёрка.md", "Итоги.md")]);
    let pending = f.window.wait(event::LINKS_PENDING);
    let second = token_of(&pending, "token");
    assert_ne!(first, second);
    assert_eq!(pending["notes"], json!(1));
    assert_eq!(pending["renames_total"], json!(2));
    assert_eq!(
        pending["renames"],
        json!([
            { "from": ["Встреча.md"], "to": ["Планёрка.md"] },
            { "from": ["Планёрка.md"], "to": ["Итоги.md"] },
        ])
    );
    f.window.clear();
    f.answer(&second, true).expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(updated["failed"], json!([]));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Итоги]] и [[Итоги#Итоги|итоги]].\r\n"
    );
    // Погашенный знак после ответа на цепочку — ничего не делает.
    f.window.clear();
    f.answer(&first, true).expect("погашенный знак");
    f.window.quiet_for(Duration::from_millis(200));
}

/// Ответ по погашенному знаку, пока цепочка ещё без ответа, — не тишина и
/// не обновление без вопроса: свежий `links:pending` на всю цепочку.
#[test]
fn superseded_token_reissues_pending() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let first = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    f.window.clear();
    f.rename_outside(&[("Планёрка.md", "Итоги.md")]);
    let second = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    f.window.clear();
    f.answer(&first, true).expect("ответ");
    let fresh = f.window.wait(event::LINKS_PENDING);
    assert_eq!(fresh["renames_total"], json!(2));
    let third = token_of(&fresh, "token");
    assert!(third != first && third != second);
    assert_eq!(f.read("Список.md"), LIST.1);
    f.window.clear();
    f.answer(&third, true).expect("ответ");
    f.window.wait(event::LINKS_UPDATED);
    assert_eq!(
        f.read("Список.md"),
        "См. [[Итоги]] и [[Итоги#Итоги|итоги]].\r\n"
    );
}

/// QA: «Всегда обновлять», ввод не записан — `busy` и «Повторить»; снаружи
/// ещё Б→В (ввод снова не записан — новый знак на всю цепочку); повтор
/// считает план заново и обновляет `[[А]]` в `[[В]]`. Старый знак повтора
/// — свежий вопрос, а не `{updated: 0, failed: []}`.
#[test]
fn qa_retry_after_second_rename() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.window.flush.store(FLUSH_REFUSED, Ordering::SeqCst);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let first = token_of(&f.window.wait(event::LINKS_UPDATED), "retry_token");
    f.window.clear();
    f.rename_outside(&[("Планёрка.md", "Итоги.md")]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "busy" }])
    );
    assert_eq!(updated["renames_total"], json!(2));
    let second = token_of(&updated, "retry_token");
    assert_ne!(first, second);

    f.window.flush.store(FLUSH_SAVED, Ordering::SeqCst);
    f.window.clear();
    f.answer(&first, true).expect("старый знак");
    let fresh = f.window.wait(event::LINKS_PENDING);
    assert_eq!(fresh["renames_total"], json!(2));
    assert_eq!(f.read("Список.md"), LIST.1);

    f.window.clear();
    f.answer(&token_of(&fresh, "token"), true).expect("повтор");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Итоги]] и [[Итоги#Итоги|итоги]].\r\n"
    );
}

/// «Повторить» после нового переименования, когда ввод при нём записан:
/// новая пачка присоединяет неотвеченную цепочку и обновляет всё сразу.
#[test]
fn retry_chain_merged_into_next_batch() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.window.flush.store(FLUSH_REFUSED, Ordering::SeqCst);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let retry = token_of(&f.window.wait(event::LINKS_UPDATED), "retry_token");
    f.window.flush.store(FLUSH_SAVED, Ordering::SeqCst);
    f.window.clear();
    f.rename_outside(&[("Планёрка.md", "Итоги.md")]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(updated["renames_total"], json!(2));
    assert_eq!(
        f.read("Список.md"),
        "См. [[Итоги]] и [[Итоги#Итоги|итоги]].\r\n"
    );
    f.window.clear();
    f.answer(&retry, true).expect("погашенный знак");
    f.window.quiet_for(Duration::from_millis(200));
}

/// Знак вопроса просрочен: ядро считает план заново и шлёт свежий
/// `links:pending`, ничего не переписывая без ответа.
#[test]
fn expired_token_reissues_pending() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let token = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    f.open
        .relink
        .advance_tokens(crate::relink::tokens::TTL + Duration::from_secs(1));
    f.window.clear();
    f.answer(&token, true).expect("ответ");
    let fresh = f.window.wait(event::LINKS_PENDING);
    assert_ne!(token_of(&fresh, "token"), token);
    assert_eq!(fresh["notes"], json!(1));
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Знак просрочен, а переписывать по цепочке уже нечего — не тишина:
/// `links:updated` со сбоем `conflict` («проверьте ссылки вручную»).
#[test]
fn expired_token_empty_plan_reports_conflict() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let token = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    std::fs::remove_file(f.abs("Планёрка.md")).expect("удаление");
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Планёрка.md"),
    }]));
    f.open
        .relink
        .advance_tokens(crate::relink::tokens::TTL + Duration::from_secs(1));
    f.window.clear();
    f.answer(&token, true).expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Планёрка.md"], "reason": "conflict" }])
    );
    assert_eq!(updated["retry_token"], Value::Null);
}

/// Переименованную заметку удалили до ответа: переименование выпало из
/// плана — `conflict`, а не `Ok` без событий.
#[test]
fn answer_after_renamed_note_deleted_reports_conflict() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md")]);
    let token = token_of(&f.window.wait(event::LINKS_PENDING), "token");
    std::fs::remove_file(f.abs("Планёрка.md")).expect("удаление");
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Планёрка.md"),
    }]));
    f.window.clear();
    f.answer(&token, true).expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Планёрка.md"], "reason": "conflict" }])
    );
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Замер (ревью безопасности 0.2-6.1): план для 10 000 внешних
/// переименований на 10 000 заметок, каждая ссылается на следующую.
/// Отладочная сборка — справочно; выпускная (perf-core) — не дольше 1 с.
#[test]
#[ignore = "замер: --release --ignored perf_ (perf-core)"]
fn perf_external_plan_10000_renames() {
    const N: usize = 10_000;
    let dir = tempfile::tempdir().expect("папка");
    for i in 0..N {
        let text = format!("[[н{}]]\n", (i + 1) % N);
        std::fs::write(dir.path().join(format!("м{i}.md")), text).expect("заметка");
    }
    let vault = Vault::open(dir.path()).expect("хранилище");
    let index = std::sync::RwLock::new(crate::index::VaultIndex::new());
    crate::index::rebuild(&vault, &index, &|_| {}).expect("индекс");
    let index = index.into_inner().expect("индекс");
    let now = vault.notes().expect("заметки");
    let renames: Vec<crate::commands::PathRename> = (0..N)
        .map(|i| crate::commands::PathRename {
            from: vp(&format!("н{i}.md")),
            to: vp(&format!("м{i}.md")),
        })
        .collect();
    let start = Instant::now();
    let prepared = super::prepare(&now, &renames);
    let locked = Instant::now();
    let snapshot = super::snapshot(&index, &prepared);
    // Столько держится замок индекса при расчёте (снимок нужных записей).
    let under_lock = locked.elapsed();
    let (plan, kept, dropped) = prepared.finish(&snapshot);
    let took = start.elapsed();
    println!(
        "perf_external_plan_10000_renames: {} мс (под замком индекса {} мс), заметок {}, ссылок {}",
        took.as_millis(),
        under_lock.as_millis(),
        plan.note_count(),
        plan.link_count()
    );
    assert_eq!((kept.len(), dropped.len()), (N, 0));
    assert_eq!(plan.link_count(), N);
    if !cfg!(debug_assertions) {
        assert!(took <= Duration::from_secs(1), "{took:?}");
        assert!(under_lock <= Duration::from_millis(100), "{under_lock:?}");
    }
}

/// Переименование туда и обратно в одной пачке — переписывать нечего.
#[test]
fn rename_and_back_in_one_batch_is_nothing() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.rename_outside(&[("Встреча.md", "Планёрка.md"), ("Планёрка.md", "Встреча.md")]);
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);
}

// --- Настоящий наблюдатель (Windows, `MoveFileExW`) ---

/// Хранилище с настоящим наблюдателем: события — от `ReadDirectoryChangesW`
/// и сверки по идентификатору файла и дате создания (ADR 0006).
struct Watched {
    f: Fixture,
    _watcher: crate::watcher::VaultWatcher,
}

fn watched(files: &[(&str, &str)]) -> Watched {
    let f = fixture(files, RenameLinks::Update, true);
    let watcher = crate::watcher::VaultWatcher::start_with(
        f.open.vault.root(),
        f.open.vault.own_writes().clone(),
        VaultEvents::new(Arc::clone(&f.open), f.window.clone()).into_sink(),
    )
    .expect("наблюдатель");
    // Наблюдатель прошёл первый обход.
    std::thread::sleep(Duration::from_millis(500));
    Watched {
        f,
        _watcher: watcher,
    }
}

/// `std::fs::rename` в Windows — `MoveFileExW`.
fn move_file(f: &Fixture, from: &str, to: &str) {
    std::fs::rename(f.abs(from), f.abs(to)).expect("MoveFileExW");
}

#[test]
#[cfg_attr(not(windows), ignore = "Windows: MoveFileExW, CI windows-latest")]
fn windows_move_in_same_folder_is_rename() {
    let w = watched(&[MEETING, LIST]);
    move_file(&w.f, "Встреча.md", "Планёрка.md");
    let updated = w.f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(
        w.f.read("Список.md"),
        "См. [[Планёрка]] и [[Планёрка#Итоги|итоги]].\r\n"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "Windows: MoveFileExW, CI windows-latest")]
fn windows_move_to_other_folder_is_rename() {
    let w = watched(&[
        ("Альфа/Встреча.md", "# Встреча\n"),
        ("Бета/Другое.md", ""),
        ("Список.md", "[[Альфа/Встреча]]\n"),
    ]);
    move_file(&w.f, "Альфа/Встреча.md", "Бета/Встреча.md");
    let updated = w.f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("external_rename"));
    assert_eq!(w.f.read("Список.md"), "[[Встреча]]\n");
}

/// Замена в духе `git checkout`: старый файл удалён, новый с тем же
/// содержимым создан под другим именем — другой файл, не переименование.
#[test]
#[cfg_attr(not(windows), ignore = "Windows: MoveFileExW, CI windows-latest")]
fn windows_checkout_like_replace_is_not_rename() {
    let w = watched(&[MEETING, LIST]);
    std::fs::remove_file(w.f.abs("Встреча.md")).expect("удаление");
    w.f.write("Планёрка.md", MEETING.1);
    // Наблюдатель увидел оба изменения.
    let start = Instant::now();
    while w.f.open.read_notes().note(&vp("Планёрка.md")).is_none() {
        assert!(start.elapsed() < Duration::from_secs(20), "нет события");
        std::thread::sleep(Duration::from_millis(20));
    }
    w.f.window.quiet_for(Duration::from_millis(600));
    assert_eq!(w.f.read("Список.md"), LIST.1);
}

/// Обмен именами через временное имя (путь после одной заметки — путь до
/// другой): индекс «до» строится заново, ссылки ведут туда же.
#[test]
fn swap_names_keeps_links_pointing_same() {
    let f = fixture(
        &[
            ("А.md", "# А\n"),
            ("Б.md", "# Б\n"),
            ("Список.md", "[[А]] [[Б]]\n"),
            ("Из А.md", "[[Б]]\n"),
        ],
        RenameLinks::Update,
        true,
    );
    f.rename_outside(&[("А.md", "В.md"), ("Б.md", "А.md"), ("В.md", "Б.md")]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["failed"], json!([]));
    assert_eq!(f.read("Список.md"), "[[Б]] [[А]]\n");
    assert_eq!(f.read("Из А.md"), "[[А]]\n");
}

/// Смена только регистра снаружи — ссылки и так ведут туда же (Р2).
#[test]
fn case_only_rename_rewrites_nothing() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.rename_outside(&[("Встреча.md", "встреча.md")]);
    f.window.quiet_for(Duration::from_millis(400));
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Linux: `Встреча.md` и `встреча.md` — две разные заметки. Переименование
/// одной из них снаружи не склеивается с двойником по ключу — нет ложного
/// `conflict`.
#[cfg(target_os = "linux")]
#[test]
fn linux_case_twin_rename_is_not_conflict() {
    let f = fixture(
        &[
            ("Встреча.md", "# В\n"),
            ("встреча.md", "# в\n"),
            ("Список.md", "[[Встреча]]\n"),
        ],
        RenameLinks::Update,
        true,
    );
    f.rename_outside(&[("встреча.md", "Планёрка.md")]);
    std::thread::sleep(Duration::from_millis(400));
    for (name, payload) in f.window.links_events() {
        if name == event::LINKS_UPDATED {
            assert_eq!(payload["failed"], json!([]), "{payload}");
        }
    }
    let (_, _, dropped) = crate::relink::external::plan(
        &crate::index::VaultIndex::new(),
        &f.open.read_notes().note_refs(),
        &[crate::commands::PathRename {
            from: vp("встреча.md"),
            to: vp("Планёрка.md"),
        }],
    );
    assert_eq!(dropped, Vec::new());
}
