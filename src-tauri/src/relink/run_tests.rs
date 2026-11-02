//! Тесты 0.2-4.5: выполнение плана на настоящем хранилище во временной
//! папке — журнал, запись с проверкой версии, пропуски, повтор и доделка.

use std::path::{Path, PathBuf};

use super::*;
use crate::commands::{RelinkFailure, RelinkFailureReason, Settings};
use crate::index::VaultIndex;
use crate::links::Links;
use crate::own_writes::OwnWrites;
use crate::relink::{RelinkPlan, plan};
use crate::vault::{OpenOptions, Vault};

/// Хранилище во временной папке: `<tmp>/vault` — корень, `<tmp>/outside` —
/// за контуром.
struct Fx {
    dir: tempfile::TempDir,
    vault: Vault,
    own: OwnWrites,
}

fn vp(rel: &str) -> VaultPath {
    VaultPath::from_components(rel.split('/'))
}

impl Fx {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let dir = tempfile::tempdir().expect("временная папка");
        std::fs::create_dir(dir.path().join("vault")).expect("корень");
        std::fs::create_dir(dir.path().join("outside")).expect("снаружи");
        std::fs::write(dir.path().join("outside/secret.md"), "секрет\n").expect("секрет");
        let own = OwnWrites::new();
        let vault = Vault::open_with(
            &dir.path().join("vault"),
            OpenOptions {
                own_writes: own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");
        let fx = Self { dir, vault, own };
        for (rel, bytes) in files {
            fx.put(rel, bytes);
        }
        fx
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("vault")
    }

    fn outside(&self) -> PathBuf {
        self.dir.path().join("outside")
    }

    fn file(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root(), |path, part| path.join(part))
    }

    fn journal(&self) -> PathBuf {
        self.root().join(".rhizomind").join("link-update.json")
    }

    fn put(&self, rel: &str, bytes: impl AsRef<[u8]>) {
        let path = self.file(rel);
        std::fs::create_dir_all(path.parent().expect("папка")).expect("папки");
        std::fs::write(path, bytes).expect("запись");
    }

    fn bytes(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.file(rel)).expect("чтение")
    }

    fn text(&self, rel: &str) -> String {
        String::from_utf8(self.bytes(rel)).expect("utf-8")
    }

    fn links(&self) -> Links {
        Links::new(self.vault.notes().expect("заметки"))
    }

    fn index(&self) -> VaultIndex {
        let notes = self.vault.notes().expect("заметки");
        VaultIndex::from_entries(notes.iter().filter_map(|note| {
            crate::index::read_entry(&self.vault, note)
                .ok()
                .map(|entry| (note.path.clone(), entry))
        }))
    }

    /// Переименования на диске по очереди и план по состоянию «до».
    fn rename(&self, steps: &[(&str, &str)]) -> (RelinkPlan, Vec<PathRename>) {
        let before = self.links();
        let index = self.index();
        let mut renames = Vec::new();
        for (from, new_name) in steps {
            let to = self
                .vault
                .rename(&vp(from), new_name)
                .expect("переименование");
            renames.push(PathRename { from: vp(from), to });
        }
        (plan(&index, &before, &renames, &[]), renames)
    }

    fn run(&self, plan: &RelinkPlan, renames: &[PathRename]) -> RunReport {
        let links = self.links();
        run(&self.vault, &links.view(), plan, renames, &mut |_, _| {})
    }

    fn resume(&self) -> Option<RunReport> {
        let links = self.links();
        resume_on_open(&self.vault, &links, &mut |_, _| {})
    }

    fn reopen(&mut self) {
        self.own = OwnWrites::new();
        self.vault = Vault::open_with(
            &self.root(),
            OpenOptions {
                own_writes: self.own.clone(),
                ..Default::default()
            },
        )
        .expect("хранилище");
    }

    /// Все файлы под временной папкой (хранилище и «снаружи»), кроме
    /// временных файлов записи, с содержимым.
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        walk(self.dir.path(), &mut out);
        let base = self.dir.path().to_path_buf();
        out.retain(|(path, _)| {
            let rel = path.strip_prefix(&base).expect("внутри временной папки");
            !rel.starts_with(Path::new("vault").join(".rhizomind").join("tmp"))
        });
        assert!(!out.is_empty(), "снимок не пуст");
        out.sort();
        out
    }
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("папка").flatten() {
        let path = entry.path();
        let kind = entry.file_type().expect("вид");
        if kind.is_dir() {
            walk(&path, out);
        } else if kind.is_file() {
            out.push((path.clone(), std::fs::read(&path).expect("файл")));
        } else {
            out.push((path, b"<link>".to_vec()));
        }
    }
}

fn failed(path: &str, reason: RelinkFailureReason) -> RelinkFailure {
    RelinkFailure {
        path: vp(path),
        reason,
    }
}

fn updated_paths(report: &RunReport) -> Vec<String> {
    let mut paths: Vec<String> = report
        .updated
        .iter()
        .map(|(path, _)| path.components().join("/"))
        .collect();
    paths.sort();
    paths
}

fn retry_paths(report: &RunReport) -> Vec<String> {
    report
        .retry
        .notes
        .iter()
        .map(|note| note.path.components().join("/"))
        .collect()
}

/// Базовое хранилище: «Встреча» и «Список» со ссылкой на неё.
fn meeting(extra: &[(&str, &[u8])]) -> Fx {
    let mut files: Vec<(&str, &[u8])> = vec![
        ("Встреча.md", "# Встреча\n".as_bytes()),
        ("Список.md", "до [[Встреча]] после\n".as_bytes()),
    ];
    files.extend_from_slice(extra);
    Fx::new(&files)
}

fn file_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("ссылка на файл");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).expect("ссылка на файл");
}

fn dir_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("ссылка на папку");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).expect("ссылка на папку");
}

#[test]
fn rewrites_links_and_removes_the_journal() {
    let fx = meeting(&[(
        "Другая.md",
        "[[Встреча|встречи]] и [[Встреча#Итоги]]\r\n".as_bytes(),
    )]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);

    let report = fx.run(&plan, &renames);

    assert_eq!(fx.text("Список.md"), "до [[Планёрка]] после\n");
    assert_eq!(
        fx.text("Другая.md"),
        "[[Планёрка|встречи]] и [[Планёрка#Итоги]]\r\n"
    );
    assert_eq!(updated_paths(&report), ["Другая.md", "Список.md"]);
    assert!(report.failed.is_empty());
    assert!(report.retry.notes.is_empty());
    assert!(!fx.journal().exists(), "журнал удалён в конце");
    let outcome = report.outcome(None);
    assert_eq!((outcome.updated, outcome.failed.len()), (2, 0));
}

#[test]
fn external_edit_between_read_and_write_retried() {
    let fx = meeting(&[]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    let path = fx.file("Список.md");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        std::fs::write(&path, "до [[Встреча]] после\nчужая правка\n").expect("правка");
    }));

    let report = fx.run(&plan, &renames);

    assert_eq!(
        fx.text("Список.md"),
        "до [[Планёрка]] после\nчужая правка\n",
        "чужая правка сохранена, ссылка переписана на свежем тексте"
    );
    assert_eq!(updated_paths(&report), ["Список.md"]);
    assert!(report.failed.is_empty());
    let names: Vec<_> = std::fs::read_dir(fx.root())
        .expect("корень")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".rhizomind")
        .collect();
    assert_eq!(names.len(), 2, "копий конфликта нет: {names:?}");
}

/// Снаружи меняют файл после каждой подготовки записи.
fn edit_after_every_temp(path: PathBuf, left: usize) {
    if left == 0 {
        return;
    }
    crate::vault::atomic::after_temp_written(Box::new(move || {
        let mut text = std::fs::read_to_string(&path).expect("чтение");
        text.push_str("правка\n");
        std::fs::write(&path, text).expect("правка");
        edit_after_every_temp(path, left - 1);
    }));
}

#[test]
fn three_retries_then_busy() {
    let fx = meeting(&[]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    edit_after_every_temp(fx.file("Список.md"), 3);

    let report = fx.run(&plan, &renames);

    assert_eq!(
        fx.text("Список.md"),
        "до [[Встреча]] после\nправка\nправка\nправка\n",
        "все чужие правки на месте, файл не переписан"
    );
    assert_eq!(
        report.failed,
        [failed("Список.md", RelinkFailureReason::Busy)]
    );
    assert_eq!(retry_paths(&report), ["Список.md"]);
    assert!(report.updated.is_empty());
    assert!(!fx.journal().exists());
}

#[test]
fn too_large_skipped() {
    let mut big = "[[Встреча]]\n".as_bytes().to_vec();
    big.extend(std::iter::repeat_n(b'x', 1024 * 1024 + 10));
    let fx = meeting(&[("Большая.md", &big)]);
    fx.vault
        .write_settings(&Settings {
            max_edit_size_bytes: 1024 * 1024,
            ..Settings::default()
        })
        .expect("настройки");
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    assert_eq!(plan.note_count(), 2, "большая заметка в плане");

    let report = fx.run(&plan, &renames);

    assert_eq!(fx.bytes("Большая.md"), big, "не тронута");
    assert_eq!(
        report.failed,
        [failed("Большая.md", RelinkFailureReason::TooLarge)]
    );
    assert_eq!(updated_paths(&report), ["Список.md"]);
    assert!(report.retry.notes.is_empty(), "не повторяется");
}

#[test]
fn not_utf8_skipped() {
    let broken = b"[[\xD0\x92\xD1\x81\xD1\x82\xD1\x80\xD0\xB5\xD1\x87\xD0\xB0]] \xFF\n";
    let fx = meeting(&[("Битая.md", broken)]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    assert_eq!(plan.note_count(), 2, "битая заметка в плане");

    let report = fx.run(&plan, &renames);

    assert_eq!(fx.bytes("Битая.md"), broken, "не тронута");
    assert_eq!(
        report.failed,
        [failed("Битая.md", RelinkFailureReason::NotUtf8)]
    );
    assert!(report.retry.notes.is_empty(), "не повторяется");
}

fn set_readonly(path: &Path, readonly: bool) {
    let mut permissions = std::fs::metadata(path).expect("мета").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(readonly);
    std::fs::set_permissions(path, permissions).expect("права");
}

#[test]
fn readonly_skipped() {
    let fx = meeting(&[]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    set_readonly(&fx.file("Список.md"), true);

    let report = fx.run(&plan, &renames);

    set_readonly(&fx.file("Список.md"), false);
    assert_eq!(fx.text("Список.md"), "до [[Встреча]] после\n");
    assert_eq!(
        report.failed,
        [failed("Список.md", RelinkFailureReason::Busy)]
    );
    assert_eq!(retry_paths(&report), ["Список.md"]);
}

#[test]
fn retry_only_busy_and_io() {
    let mut big = "[[Встреча]]\n".as_bytes().to_vec();
    big.extend(std::iter::repeat_n(b'x', 1024 * 1024 + 10));
    let broken = b"[[\xD0\x92\xD1\x81\xD1\x82\xD1\x80\xD0\xB5\xD1\x87\xD0\xB0]] \xFF\n";
    let fx = meeting(&[("Большая.md", &big), ("Битая.md", broken)]);
    fx.vault
        .write_settings(&Settings {
            max_edit_size_bytes: 1024 * 1024,
            ..Settings::default()
        })
        .expect("настройки");
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    set_readonly(&fx.file("Список.md"), true);

    let first = fx.run(&plan, &renames);

    assert_eq!(first.failed.len(), 3, "{:?}", first.failed);
    assert_eq!(retry_paths(&first), ["Список.md"], "только занятые");
    let outcome = first.outcome(None);
    assert_eq!(outcome.failed.len(), 3);

    // Файл освободили и поправили снаружи: ссылки находятся заново.
    set_readonly(&fx.file("Список.md"), false);
    fx.put(
        "Список.md",
        "новое [[Встреча]] начало\nдо [[Встреча]] после\n",
    );
    let second = fx.run(&first.retry, &renames);

    assert_eq!(
        fx.text("Список.md"),
        "новое [[Планёрка]] начало\nдо [[Планёрка]] после\n"
    );
    assert_eq!(updated_paths(&second), ["Список.md"]);
    assert!(second.failed.is_empty());
    assert_eq!(fx.bytes("Большая.md"), big);
    assert_eq!(fx.bytes("Битая.md"), broken);
}

#[test]
fn crash_midway_resumed_on_open() {
    let fx_files: Vec<(String, String)> = (1..=4)
        .map(|i| (format!("Заметка {i}.md"), format!("{i}: [[Встреча]]\n")))
        .collect();
    let files: Vec<(&str, &[u8])> = fx_files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_bytes()))
        .collect();
    let mut fx = meeting(&files);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    assert_eq!(plan.note_count(), 5);

    crash_after(2);
    let partial = fx.run(&plan, &renames);
    assert_eq!(partial.updated.len(), 2, "упали после двух заметок");
    assert!(fx.journal().is_file(), "журнал остался");

    fx.reopen();
    let report = fx.resume().expect("доделка");

    for (path, _) in &fx_files {
        assert!(
            fx.text(path).contains("[[Планёрка]]"),
            "{path}: {}",
            fx.text(path)
        );
    }
    assert_eq!(fx.text("Список.md"), "до [[Планёрка]] после\n");
    assert!(report.failed.is_empty());
    assert!(!fx.journal().exists(), "журнала нет");
    assert!(fx.resume().is_none(), "второй раз доделывать нечего");
}

/// Перестановка папок: новая цель одной ссылки совпадает со старой целью
/// другой в той же заметке (ревью 0.2-4.3, «Для 4.5 и 4.6»).
#[test]
fn resume_rewrites_only_still_old_targets() {
    let both = "[[Альфа/Встреча]] и [[Бета/Встреча]]\n";
    let mut fx = Fx::new(&[
        ("Альфа/Встреча.md", b"# A\n"),
        ("Бета/Встреча.md", b"# B\n"),
        ("X.md", both.as_bytes()),
        ("Y.md", both.as_bytes()),
        ("Z.md", both.as_bytes()),
    ]);
    let (plan, renames) = fx.rename(&[("Альфа", "Гамма"), ("Бета", "Альфа")]);
    assert_eq!(plan.note_count(), 3);

    crash_after(1);
    fx.run(&plan, &renames);
    let done = "[[Гамма/Встреча]] и [[Альфа/Встреча]]\n";
    assert_eq!(fx.text("X.md"), done, "X записана до падения");
    // Пока программа была закрыта, пользователь сам поправил одну ссылку в Z.
    fx.put("Z.md", "[[Гамма/Встреча]] и [[Бета/Встреча]] правка\n");

    fx.reopen();
    let report = fx.resume().expect("доделка");

    assert_eq!(
        fx.text("X.md"),
        done,
        "записанная заметка не переписана снова"
    );
    assert_eq!(fx.text("Y.md"), done);
    assert_eq!(
        fx.text("Z.md"),
        "[[Гамма/Встреча]] и [[Альфа/Встреча]] правка\n"
    );
    assert!(report.failed.is_empty());
    assert!(!fx.journal().exists());
}

#[test]
fn own_writes_recorded_no_external_event() {
    let fx = meeting(&[("Другая.md", "[[Встреча]]\n".as_bytes())]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);

    let report = fx.run(&plan, &renames);

    assert_eq!(report.updated.len(), 2);
    for (path, version) in &report.updated {
        let abs = fx.file(&path.components().join("/"));
        assert!(
            fx.own.contains(path, &version.hash, version.size),
            "{path:?}: своя версия в журнале"
        );
        assert!(
            fx.own.is_own(path, &abs),
            "{path:?}: наблюдатель узнает свою запись"
        );
        let on_disk = blake3::hash(&std::fs::read(&abs).expect("файл"))
            .to_hex()
            .to_string();
        assert_eq!(on_disk, version.hash);
    }
}

#[test]
fn journal_never_outside_service_dir() {
    // Во время выполнения журнал — только `.rhizomind/link-update.json`;
    // после — нигде; файлов вне `.rhizomind/` не прибавилось.
    let fx = meeting(&[("Другая.md", "[[Встреча]]\n".as_bytes())]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    let before: Vec<PathBuf> = fx.snapshot().into_iter().map(|(p, _)| p).collect();
    let journal = fx.journal();
    let mut seen = 0;
    let links = fx.links();
    run(&fx.vault, &links.view(), &plan, &renames, &mut |_, _| {
        assert!(journal.is_file(), "журнал в служебной папке");
        seen += 1;
    });
    assert!(seen > 0);
    let after: Vec<PathBuf> = fx.snapshot().into_iter().map(|(p, _)| p).collect();
    assert_eq!(before, after, "новых файлов нет, журнал удалён");

    // Журнал подменён ссылкой наружу: ни записи, ни чтения через неё.
    let fx = meeting(&[]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    let target = fx.outside().join("journal.json");
    std::fs::write(&target, "{}").expect("цель");
    file_link(&target, &fx.journal());
    let snapshot = fx.snapshot();

    let report = fx.run(&plan, &renames);

    assert_eq!(fx.snapshot(), snapshot, "ничего не записано");
    assert_eq!(
        report.failed,
        [failed("Список.md", RelinkFailureReason::Io)]
    );
    assert_eq!(retry_paths(&report), ["Список.md"]);
    assert!(fx.resume().is_none());
    assert_eq!(fx.snapshot(), snapshot, "доделка не читает ссылку наружу");
}

/// Подменённый журнал — недоверенные данные: без паники, без записи вне
/// хранилища и в служебную папку, только ссылки, которые после правки
/// однозначно ведут на заметку из журнала.
#[test]
fn forged_journal_is_untrusted() {
    let fx = meeting(&[("Планёрка.md", "# П\n".as_bytes())]);
    fx.vault
        .write_settings(&Settings::default())
        .expect("настройки");
    dir_link(&fx.outside(), &fx.file("Наружу"));
    let link = |path: &str, old: &str, new: &str, to: &str| {
        let path: Vec<&str> = path.split('/').collect();
        let to: Vec<&str> = to.split('/').collect();
        serde_json::json!({
            "path": path,
            "links": [{ "old_target": old, "new_target": new, "points_to": to }],
        })
    };
    let journal = |notes: Vec<serde_json::Value>| {
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "renames": [],
            "writing": null,
            "notes": notes,
        }))
        .expect("json")
    };
    let mut huge = vec![b' '; 33 * 1024 * 1024];
    huge.extend_from_slice(b"{}");
    let cases: Vec<Vec<u8>> = vec![
        b"not json".to_vec(),
        b"[1,2,3]".to_vec(),
        br#"{"format_version": 2, "renames": [], "writing": null, "notes": []}"#.to_vec(),
        br#"{"format_version": [1,1,1,1,1,1,1,1], "renames": [], "notes": []}"#.to_vec(),
        br#"{"format_version": 1, "renames": [], "writing": null, "notes": [], "x": 1}"#.to_vec(),
        huge,
        journal(vec![
            link("../outside/secret.md", "Встреча", "Планёрка", "Планёрка.md"),
            link("..", "Встреча", "Планёрка", "Планёрка.md"),
            link(
                ".rhizomind/settings.json",
                "Встреча",
                "Планёрка",
                "Планёрка.md",
            ),
            link("Наружу/secret.md", "секрет", "Планёрка", "Планёрка.md"),
            link("", "Встреча", "Планёрка", "Планёрка.md"),
            // Новая цель не ведёт туда, куда должна, — не переписывается.
            link("Список.md", "Встреча", "Другое", "Планёрка.md"),
            link("Список.md", "Встреча", "Встреча", "Планёрка.md"),
        ]),
    ];
    for (i, bytes) in cases.into_iter().enumerate() {
        std::fs::write(fx.journal(), &bytes).expect("журнал");
        let snapshot = fx.snapshot();

        let _ = fx.resume();

        let mut after = fx.snapshot();
        let journal_path = fx.journal();
        let expected: Vec<_> = snapshot
            .into_iter()
            .filter(|(p, _)| *p != journal_path)
            .collect();
        after.retain(|(p, _)| *p != journal_path);
        assert_eq!(after, expected, "случай {i}: ничего не изменилось");
        assert!(!fx.journal().exists(), "случай {i}: журнал убран");
    }
    assert_eq!(
        std::fs::read(fx.outside().join("secret.md")).expect("секрет"),
        "секрет\n".as_bytes()
    );
}

/// Дата создания сохраняется (ADR 0006, уточнение 25.10.2026): на Windows —
/// `ReplaceFileW`. На Linux дату создания не задать — тест только для
/// Windows (CI `windows-latest`), не `ignore`: шаг CI с `--ignored` на
/// Linux его бы запустил.
#[cfg(windows)]
#[test]
fn creation_date_preserved() {
    let fx = meeting(&[]);
    let created = std::fs::metadata(fx.file("Список.md"))
        .expect("мета")
        .created()
        .expect("дата создания");
    std::thread::sleep(std::time::Duration::from_millis(50));
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);

    let report = fx.run(&plan, &renames);

    assert_eq!(updated_paths(&report), ["Список.md"]);
    assert_eq!(
        std::fs::metadata(fx.file("Список.md"))
            .expect("мета")
            .created()
            .expect("дата создания"),
        created
    );
}

/// Файл, открытый другой программой без общего доступа (`share_mode(0)`,
/// как Word или Excel держат свои файлы), — `busy`: файл не тронут, его можно
/// повторить. Только Windows: на Linux такого запрета нет.
#[cfg(windows)]
#[test]
fn busy_file_on_windows_skipped_untouched() {
    use std::os::windows::fs::OpenOptionsExt;
    let fx = meeting(&[]);
    let (plan, renames) = fx.rename(&[("Встреча.md", "Планёрка.md")]);
    let before = fx.bytes("Список.md");
    let links = fx.links();
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(fx.file("Список.md"))
        .expect("чужая программа открыла файл");

    let report = run(&fx.vault, &links.view(), &plan, &renames, &mut |_, _| {});

    drop(holder);
    assert_eq!(
        report.failed,
        [failed("Список.md", RelinkFailureReason::Busy)]
    );
    assert_eq!(retry_paths(&report), ["Список.md"]);
    assert_eq!(fx.bytes("Список.md"), before, "не испорчен");
    assert!(!fx.journal().exists());
}

/// Ссылка с прежней целью, которая и так однозначно ведёт на нужную
/// заметку (вторую «Встречу» удалили, пока программа была закрыта), при
/// доделке не переписывается: лишней правки текста пользователя нет.
#[test]
fn resume_leaves_links_that_already_lead_there() {
    let fx = Fx::new(&[
        ("Папка/Встреча.md", b"# B\n"),
        ("Список.md", "[[Встреча]] и [[Встреча]]\n".as_bytes()),
    ]);
    let plan = [crate::relink::NotePlan {
        path: vp("Список.md"),
        links: vec![crate::relink::PlannedLink {
            old_target: "Встреча".into(),
            new_target: "Папка/Встреча".into(),
            points_to: vp("Папка/Встреча.md"),
        }],
    }];
    crate::relink::journal::write(&fx.vault, &[], None, &plan).expect("журнал");

    let report = fx.resume().expect("доделка");

    assert_eq!(fx.text("Список.md"), "[[Встреча]] и [[Встреча]]\n");
    assert!(report.updated.is_empty() && report.failed.is_empty());
    assert!(!fx.journal().exists());
}
