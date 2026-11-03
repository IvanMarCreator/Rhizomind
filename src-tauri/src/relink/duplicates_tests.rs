//! Уточнение ссылок при повторе имени (задача 0.2-6.2): от пачки
//! наблюдателя (`VaultEvents`) до файлов на диске и событий окна, и чистые
//! части — пачка, сложение пачек, план.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::commands::{
    NoteList, OpenVault, RelinkAnswerRequest, RelinkToken, RenameLinks, TreeChange,
    VaultChangedPayload, VaultId, VaultSummary, event, start_index,
};
use crate::events::{Emit, VaultEvents};
use crate::own_writes::OwnWrites;
use crate::vault::{OpenOptions, Vault};
use crate::watcher::WatcherEvent;

const FLUSH_SAVED: u8 = 0;
const FLUSH_REFUSED: u8 = 1;

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
    /// События обновления ссылок без хода переписывания.
    fn links_events(&self) -> Vec<(String, Value)> {
        self.events
            .lock()
            .expect("журнал")
            .iter()
            .filter(|(name, _)| name.starts_with("links:") && name != event::LINKS_PROGRESS)
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

/// Короткое затишье в тестах (кроме `quiet_2s_one_plan`).
const TEST_QUIET: Duration = Duration::from_millis(100);

/// Пауза, за которую затишье точно прошло бы.
const AFTER_QUIET: Duration = Duration::from_millis(600);

fn fixture(files: &[(&str, &str)], mode: RenameLinks, ready: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("папка");
    for (rel, text) in files {
        let path = rel
            .split('/')
            .fold(dir.path().to_path_buf(), |p, c| p.join(c));
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
        std::fs::write(&path, text).expect("файл");
        // Разные даты создания: порядок «самая старая первой» — по порядку
        // в списке `files`, а не по путям при равной дате.
        std::thread::sleep(Duration::from_millis(5));
    }
    let vault = Vault::open_with(
        dir.path(),
        OpenOptions {
            own_writes: OwnWrites::new(),
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
    open.relink.set_duplicate_quiet(TEST_QUIET);
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
        let path = self.abs(rel);
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("папки");
        std::fs::write(path, text).expect("запись");
    }

    /// Заметка появилась снаружи (копия в Проводнике): файл и пачка
    /// наблюдателя.
    fn create_outside(&self, rels: &[&str]) {
        let mut changes = Vec::new();
        for rel in rels {
            self.write(rel, "# копия\n");
            changes.push(created(rel));
        }
        self.events.handle(changed(changes));
    }

    /// Правка текста снаружи (или откат Git): файл и `Modified`.
    fn edit_outside(&self, rel: &str, text: &str) {
        self.write(rel, text);
        self.events
            .handle(changed(vec![TreeChange::Modified { path: vp(rel) }]));
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

fn created(rel: &str) -> TreeChange {
    TreeChange::Created {
        path: vp(rel),
        entry_kind: EntryKind::Note,
    }
}

fn changed(changes: Vec<TreeChange>) -> WatcherEvent {
    WatcherEvent::Changed(VaultChangedPayload {
        rescan: false,
        changes,
    })
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(name, _)| name.as_str()).collect()
}

const MEETING: (&str, &str) = ("Альфа/Встреча.md", "# Встреча\n");
const LIST: (&str, &str) = ("Список.md", "См. [[Встреча]] и [[Встреча|встречи]].\r\n");
const LIST_CLARIFIED: &str = "См. [[Альфа/Встреча]] и [[Альфа/Встреча|встречи]].\r\n";

/// Ждёт уточнения после появления повтора и проверяет его итог.
fn clarified(f: &Fixture) {
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("duplicate"));
    assert_eq!(updated["updated"], json!(1));
    assert_eq!(updated["failed"], json!([]));
    assert_eq!(f.read("Список.md"), LIST_CLARIFIED);
    assert_eq!(
        names(&f.window.links_events()),
        vec![event::LINKS_FLUSH_REQUESTED, event::LINKS_UPDATED],
        "ввод записан до переписывания, вопроса нет"
    );
}

/// Появилась заметка с уже существующим именем — ссылки, которые стали
/// неоднозначными, уточнены путём той заметки, на которую вели; заголовок
/// и псевдоним сохранены, байты вне целей (CRLF) — как были.
#[test]
fn new_duplicate_clarifies() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.create_outside(&["Бета/Встреча.md"]);
    clarified(&f);
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

fn referring(count: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|i| (format!("Ссылки/{i:02}.md"), "[[Встреча]]\n".to_owned()))
        .collect()
}

fn with_refs(count: usize, mode: RenameLinks) -> Fixture {
    let refs = referring(count);
    let mut files: Vec<(&str, &str)> = refs.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    files.push(MEETING);
    fixture(&files, mode, true)
}

/// До 20 заметок — сразу; 21 — вопрос `links:pending {duplicate}`, до
/// ответа ничего не переписано; «Уточнить» — переписаны все 21.
#[test]
fn twenty_immediately_twentyone_asks() {
    let f = with_refs(20, RenameLinks::Ask);
    f.create_outside(&["Бета/Встреча.md"]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(20));
    assert!(
        !names(&f.window.links_events()).contains(&event::LINKS_PENDING),
        "20 — без вопроса"
    );

    let f = with_refs(21, RenameLinks::Ask);
    f.create_outside(&["Бета/Встреча.md"]);
    let pending = f.window.wait(event::LINKS_PENDING);
    assert_eq!(pending["reason"], json!("duplicate"));
    assert_eq!(pending["notes"], json!(21));
    assert_eq!(pending["links"], json!(21));
    assert_eq!(pending["renames"], json!([]));
    std::thread::sleep(AFTER_QUIET);
    assert_eq!(names(&f.window.links_events()), vec![event::LINKS_PENDING]);
    assert_eq!(
        f.read("Ссылки/00.md"),
        "[[Встреча]]\n",
        "до ответа — как было"
    );

    f.window.clear();
    f.answer(pending["token"].as_str().expect("знак"), true)
        .expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("duplicate"));
    assert_eq!(updated["updated"], json!(21));
    assert_eq!(f.read("Ссылки/20.md"), "[[Альфа/Встреча]]\n");
}

/// «Не трогать» — ничего не переписано, следующая пачка вопрос не
/// повторяет.
#[test]
fn answer_keep_rewrites_nothing() {
    let f = with_refs(21, RenameLinks::Ask);
    f.create_outside(&["Бета/Встреча.md"]);
    let pending = f.window.wait(event::LINKS_PENDING);
    f.window.clear();
    f.answer(pending["token"].as_str().expect("знак"), false)
        .expect("ответ");
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Ссылки/00.md"), "[[Встреча]]\n");
    f.create_outside(&["Другое.md"]);
    f.window.quiet_for(AFTER_QUIET);
}

/// Новый вопрос о повторах заменяет старый: старый знак погашен, приходит
/// один новый `links:pending` на обе пачки; ответ по старому знаку задаёт
/// вопрос заново, по новому — уточняет всё.
#[test]
fn new_question_replaces_old() {
    let refs = referring(21);
    let mut files: Vec<(&str, &str)> = refs.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    files.push(MEETING);
    let others: Vec<(String, String)> = (0..21)
        .map(|i| (format!("Другие/{i:02}.md"), "[[План]]\n".to_owned()))
        .collect();
    files.extend(others.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    files.push(("Альфа/План.md", "# План\n"));
    let f = fixture(&files, RenameLinks::Ask, true);

    f.create_outside(&["Бета/Встреча.md"]);
    let first = f.window.wait(event::LINKS_PENDING);
    f.window.clear();
    f.create_outside(&["Бета/План.md"]);
    let second = f.window.wait(event::LINKS_PENDING);
    assert_eq!(second["notes"], json!(42), "одна пачка из двух");
    assert_ne!(first["token"], second["token"]);
    std::thread::sleep(AFTER_QUIET);
    assert_eq!(names(&f.window.links_events()), vec![event::LINKS_PENDING]);

    f.window.clear();
    f.answer(first["token"].as_str().expect("знак"), true)
        .expect("старый знак");
    let third = f.window.wait(event::LINKS_PENDING);
    assert_eq!(third["notes"], json!(42));
    assert_eq!(
        f.read("Ссылки/00.md"),
        "[[Встреча]]\n",
        "по старому знаку — ничего"
    );

    f.window.clear();
    f.answer(third["token"].as_str().expect("знак"), true)
        .expect("ответ");
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["updated"], json!(42));
    assert_eq!(f.read("Другие/00.md"), "[[Альфа/План]]\n");
}

/// Правка текста никогда не запускает уточнение — даже если в хранилище
/// уже есть неоднозначная ссылка (повтор появился, пока приложение было
/// закрыто) и правка добавляет ещё одну.
#[test]
fn text_edit_never_triggers() {
    let f = fixture(
        &[MEETING, ("Бета/Встреча.md", "# Встреча\n"), LIST],
        RenameLinks::Update,
        true,
    );
    f.edit_outside("Список.md", "См. [[Встреча]], [[Встреча]].\n");
    f.edit_outside("Бета/Встреча.md", "# Встреча\n[[Встреча]]\n");
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), "См. [[Встреча]], [[Встреча]].\n");
}

/// Откат уточнения Git (старый текст вернулся) — новой волны нет.
#[test]
fn revert_by_git_no_new_wave() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.create_outside(&["Бета/Встреча.md"]);
    clarified(&f);
    f.window.clear();
    f.edit_outside("Список.md", LIST.1);
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Своя запись ядра (переписанная заметка) — новой волны нет, даже если
/// наблюдатель о ней сообщит.
#[test]
fn own_rewrite_no_new_wave() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.create_outside(&["Бета/Встреча.md"]);
    clarified(&f);
    f.window.clear();
    f.events.handle(changed(vec![TreeChange::Modified {
        path: vp("Список.md"),
    }]));
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), LIST_CLARIFIED);
}

/// «Без войны»: 100 циклов «правка / откат» снаружи — ни одной записи ядра
/// и ни одного события обновления ссылок.
#[test]
fn no_war_100_edit_revert_cycles() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Update, true);
    f.create_outside(&["Бета/Встреча.md"]);
    clarified(&f);
    f.window.clear();
    for cycle in 0..100 {
        let text = if cycle % 2 == 0 {
            LIST.1
        } else {
            LIST_CLARIFIED
        };
        f.edit_outside("Список.md", text);
        f.edit_outside("Бета/Встреча.md", "[[Встреча]]\n");
        assert_eq!(f.read("Список.md"), text, "цикл {cycle}");
    }
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), LIST_CLARIFIED);
    assert_eq!(f.read("Бета/Встреча.md"), "[[Встреча]]\n");
}

/// Три пачки с паузами меньше 2 с — один план после 2 с затишья: одна
/// запись ввода и одно `links:updated` на все три.
#[test]
fn quiet_2s_one_plan() {
    let f = fixture(
        &[
            ("А/Встреча.md", "x"),
            ("А/План.md", "x"),
            ("А/Итоги.md", "x"),
            ("1.md", "[[Встреча]]"),
            ("2.md", "[[План]]"),
            ("3.md", "[[Итоги]]"),
        ],
        RenameLinks::Ask,
        true,
    );
    f.open.relink.set_duplicate_quiet(QUIET);
    f.create_outside(&["Б/Встреча.md"]);
    std::thread::sleep(Duration::from_millis(700));
    f.create_outside(&["Б/План.md"]);
    std::thread::sleep(Duration::from_millis(700));
    f.create_outside(&["Б/Итоги.md"]);
    let last = Instant::now();
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(
        f.window.links_events(),
        Vec::<(String, Value)>::new(),
        "ещё не затихло"
    );
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert!(last.elapsed() >= QUIET);
    assert_eq!(updated["updated"], json!(3));
    std::thread::sleep(AFTER_QUIET);
    assert_eq!(
        names(&f.window.links_events()),
        vec![event::LINKS_FLUSH_REQUESTED, event::LINKS_UPDATED]
    );
    assert_eq!(f.read("1.md"), "[[А/Встреча]]");
    assert_eq!(f.read("3.md"), "[[А/Итоги]]");
}

/// Повтор исчез (копию удалили) — уточнённые ссылки остаются с путём (Р11).
#[test]
fn duplicate_removed_paths_stay() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.create_outside(&["Бета/Встреча.md"]);
    clarified(&f);
    f.window.clear();
    std::fs::remove_file(f.abs("Бета/Встреча.md")).expect("удаление");
    f.events.handle(changed(vec![TreeChange::Removed {
        path: vp("Бета/Встреча.md"),
    }]));
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), LIST_CLARIFIED);
}

/// Уточнение не зависит от настройки «При переименовании…».
#[test]
fn independent_of_rename_links_setting() {
    for mode in [RenameLinks::Ask, RenameLinks::Update] {
        let f = fixture(&[MEETING, LIST], mode, true);
        f.create_outside(&["Бета/Встреча.md"]);
        clarified(&f);
    }
}

/// Создание заметки в приложении — тот же механизм: команда уже добавила
/// заметку в список, наблюдатель сообщает о ней следом.
#[test]
fn in_app_create_same_mechanism() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.write("Бета/Встреча.md", "");
    f.open.apply_own(&[created("Бета/Встреча.md")]);
    f.events.handle(changed(vec![created("Бета/Встреча.md")]));
    clarified(&f);
}

/// Переименование в приложении в уже существующее имя — тот же механизм
/// (своё переименование команда уже применила к списку).
#[test]
fn in_app_rename_same_mechanism() {
    let f = fixture(
        &[MEETING, LIST, ("Бета/Заметка.md", "x")],
        RenameLinks::Ask,
        true,
    );
    let rename = TreeChange::Renamed {
        from: vp("Бета/Заметка.md"),
        to: vp("Бета/Встреча.md"),
    };
    f.open
        .vault
        .own_writes()
        .record_rename(&vp("Бета/Заметка.md"), &vp("Бета/Встреча.md"));
    std::fs::rename(f.abs("Бета/Заметка.md"), f.abs("Бета/Встреча.md")).expect("переименование");
    f.open.apply_own(std::slice::from_ref(&rename));
    f.events.handle(changed(vec![rename]));
    clarified(&f);
}

/// Перенос заметки с уже повторяющимся именем — ссылка была неоднозначной
/// и до пачки: ничего не переписывается (и не уводится на другую заметку).
#[test]
fn move_of_existing_duplicate_rewrites_nothing() {
    let f = fixture(
        &[MEETING, ("Бета/Встреча.md", "x"), LIST],
        RenameLinks::Ask,
        true,
    );
    std::fs::create_dir_all(f.abs("Гамма")).expect("папка");
    std::fs::rename(f.abs("Альфа/Встреча.md"), f.abs("Гамма/Встреча.md")).expect("перенос");
    f.events.handle(changed(vec![TreeChange::Renamed {
        from: vp("Альфа/Встреча.md"),
        to: vp("Гамма/Встреча.md"),
    }]));
    f.window.quiet_for(AFTER_QUIET);
    assert_eq!(f.read("Список.md"), LIST.1);
}

/// Пока индекс не собран и не сверен — пачка ждёт.
#[test]
fn waits_for_index_ready() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, false);
    f.create_outside(&["Бета/Встреча.md"]);
    f.window.quiet_for(AFTER_QUIET);
    f.start_index();
    clarified(&f);
}

/// Ввод окна не записан — ничего не переписано, но и не молча: `busy` и
/// знак «Повторить».
#[test]
fn flush_refused_reports_busy_with_retry() {
    let f = fixture(&[MEETING, LIST], RenameLinks::Ask, true);
    f.window.flush.store(FLUSH_REFUSED, Ordering::SeqCst);
    f.create_outside(&["Бета/Встреча.md"]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("duplicate"));
    assert_eq!(updated["updated"], json!(0));
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "busy" }])
    );
    assert_eq!(f.read("Список.md"), LIST.1);
    f.window.flush.store(FLUSH_SAVED, Ordering::SeqCst);
    f.window.clear();
    f.answer(updated["retry_token"].as_str().expect("знак повтора"), true)
        .expect("повтор");
    let retried = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(retried["updated"], json!(1));
    assert_eq!(f.read("Список.md"), LIST_CLARIFIED);
}

/// Ссылку нельзя уточнить (путь не написать в ссылке) — сбой `conflict`,
/// а не тишина.
#[test]
fn unfixable_reports_conflict() {
    let f = fixture(
        &[("Альфа#1/Встреча.md", "x"), ("Список.md", "[[Встреча]]")],
        RenameLinks::Ask,
        true,
    );
    f.create_outside(&["Бета/Встреча.md"]);
    let updated = f.window.wait(event::LINKS_UPDATED);
    assert_eq!(updated["reason"], json!("duplicate"));
    assert_eq!(
        updated["failed"],
        json!([{ "path": ["Список.md"], "reason": "conflict" }])
    );
    assert_eq!(updated["retry_token"], Value::Null);
    assert_eq!(f.read("Список.md"), "[[Встреча]]");
}

// --- Чистые части ---

fn list(paths: &[&str]) -> NoteList {
    NoteList::new(
        paths
            .iter()
            .enumerate()
            .map(|(i, p)| NoteRef {
                path: vp(p),
                created_ms: i as i64,
            })
            .collect(),
    )
}

/// Наблюдает изменения так, как это делает `VaultEvents`.
fn observe_all(list: &mut NoteList, batch: &mut Batch, changes: &[TreeChange]) {
    list.apply_observed(changes, &|_| Some(1_000), &mut |list, change| {
        batch.observe(change, &|p| list.notes_under(p));
    });
}

#[test]
fn modified_is_not_composition() {
    let mut batch = Batch::default();
    let notes = list(&["а.md"]);
    assert!(
        !batch.observe(&TreeChange::Modified { path: vp("а.md") }, &|p| notes
            .notes_under(p))
    );
    assert!(!batch.is_touched());
}

#[test]
fn rename_and_back_is_nothing() {
    let mut notes = list(&["А/а.md"]);
    let mut batch = Batch::default();
    observe_all(
        &mut notes,
        &mut batch,
        &[
            TreeChange::Renamed {
                from: vp("А"),
                to: vp("Б"),
            },
            TreeChange::Renamed {
                from: vp("Б"),
                to: vp("А"),
            },
        ],
    );
    let prepared = prepare(&batch, notes.note_refs());
    assert!(prepared.is_empty());
}

/// Удалили и создали заново (в разных пачках наблюдателя) — новая заметка;
/// замену файла на месте наблюдатель выдаёт как `Modified`.
#[test]
fn removed_and_created_again_is_new_note() {
    let mut notes = list(&["а.md"]);
    let mut batch = Batch::default();
    observe_all(
        &mut notes,
        &mut batch,
        &[TreeChange::Removed { path: vp("а.md") }, created("а.md")],
    );
    assert!(!prepare(&batch, notes.note_refs()).is_empty());
}

fn arb_change() -> impl Strategy<Value = TreeChange> {
    let path = prop::sample::select(vec![
        "а.md", "б.md", "П/а.md", "П/б.md", "Р/а.md", "П", "Р", "в.txt",
    ]);
    prop_oneof![
        path.clone().prop_map(created),
        path.clone()
            .prop_map(|p| TreeChange::Removed { path: vp(p) }),
        (path.clone(), path).prop_map(|(a, b)| TreeChange::Renamed {
            from: vp(a),
            to: vp(b),
        }),
    ]
}

/// Изменения, которые бывают на диске: создаётся то, чего нет;
/// переименовывается существующее на свободное место.
fn valid_after(start: &[&str], done: &[TreeChange], changes: &[TreeChange]) -> Vec<TreeChange> {
    let mut notes = list(start);
    notes.apply(done, &|_| Some(1_000));
    let mut out = Vec::new();
    for change in changes {
        let ok = match change {
            TreeChange::Created { path, .. } => notes.notes_under(path).is_empty(),
            TreeChange::Removed { path } => !notes.notes_under(path).is_empty(),
            TreeChange::Renamed { from, to } => {
                !notes.notes_under(from).is_empty()
                    && notes.notes_under(to).is_empty()
                    && !to.components().starts_with(from.components())
            }
            TreeChange::Modified { .. } => true,
        };
        if ok {
            notes.apply(std::slice::from_ref(change), &|_| Some(1_000));
            out.push(change.clone());
        }
    }
    out
}

fn valid(start: &[&str], changes: &[TreeChange]) -> Vec<TreeChange> {
    valid_after(start, &[], changes)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Неотвеченная пачка, сложенная со следующей, — то же, что одна пачка
    /// из изменений обеих.
    #[test]
    fn compose_equals_one_batch(
        first in prop::collection::vec(arb_change(), 0..6),
        second in prop::collection::vec(arb_change(), 0..6),
    ) {
        let start = ["а.md", "П/а.md", "П/б.md"];
        let first = valid(&start, &first);
        let second = valid_after(&start, &first, &second);
        let mut whole = list(&start);
        let mut one = Batch::default();
        observe_all(&mut whole, &mut one, &first);
        observe_all(&mut whole, &mut one, &second);

        let mut split = list(&start);
        let (mut a, mut b) = (Batch::default(), Batch::default());
        observe_all(&mut split, &mut a, &first);
        observe_all(&mut split, &mut b, &second);
        let joined = compose(a, b);
        prop_assert_eq!(&joined.origin, &one.origin);
        prop_assert_eq!(&joined.left, &one.left);
    }
}
