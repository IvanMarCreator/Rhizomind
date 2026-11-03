//! Замеры переписывания ссылок на уровне ядра (задача 0.2-7.1, второй
//! прогон — перед КТ2; С2-8.3: переименование заметки со 100 ссылающимися
//! ≤ 2 с, перенос папки на 1 000 заметок с 5 000 ссылками ≤ 15 с, 50 000 —
//! ≤ 20 с).
//!
//! Пределы С2-8.3 относятся ко всему пути «от ответа до уведомления» — его
//! меряет `commands::note_list_perf::perf_relink_rename_move_via_commands`
//! (через команды окна, с проверкой пределов). Здесь — из чего этот путь
//! складывается, справочно: план ([`plan()`]) и выполнение плана
//! ([`run()`]: чтение, переписывание и атомарная запись заметок с журналом
//! `.rhizomind/link-update.json`), а также план и выполнение 10 000
//! внешних переименований (ревью безопасности 0.2-6.1).
//!
//! Методика и помощники — `index::perf_tests`.

use std::path::Path;
use std::sync::RwLock;

use super::{external, plan, run};
use crate::commands::{PathRename, VaultPath};
use crate::index::perf_tests::{REPEATS, Report, scales, timed};
use crate::index::{VaultIndex, rebuild};
use crate::links::Links;
use crate::vault::Vault;

fn path(s: &str) -> VaultPath {
    VaultPath::from_components(s.split('/'))
}

/// Индекс хранилища сейчас (перед каждым повтором: прошлый повтор
/// переписал заметки).
fn index_of(vault: &Vault) -> VaultIndex {
    let index = RwLock::new(VaultIndex::new());
    rebuild(vault, &index, &|_| {}).expect("собран");
    index.into_inner().expect("индекс")
}

/// Копия папки `from` в `to` со всеми вложенными.
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

/// План и выполнение плана (запись заметок с журналом) для «узла» и папки
/// на 1 000 заметок — справочно; предел — у замера через команды. Каждый
/// повтор — новая копия хранилища генератора (выполнение переписывает
/// заметки, а после первого переноса ссылки в папку уже короткие).
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_relink_plan_and_run() {
    for notes in scales() {
        let mut report = Report::new(&format!(
            "perf-core план и выполнение переписывания ссылок (справочно), {notes} заметок"
        ));
        let dir = tempfile::tempdir().expect("папка");
        let pristine = dir.path().join("pristine");
        let manifest =
            gen_vault::generate(&pristine, &gen_vault::Config::new(notes, 7)).expect("генератор");
        std::fs::create_dir_all(pristine.join("Перенесено")).expect("папка");

        type Act = Box<dyn Fn(&Vault) -> VaultPath>;
        let mut cases: Vec<(&str, PathRename, Act)> = Vec::new();
        if let Some(hub) = &manifest.hub {
            let from = path(&hub.path);
            let name = hub.path.rsplit('/').next().expect("имя");
            let new_name = name.replace(".md", " переименован.md");
            let to = path(&hub.path.replace(name, &new_name));
            let old = from.clone();
            cases.push((
                "переименование «узла»",
                PathRename { from, to },
                Box::new(move |v| v.rename(&old, &new_name).expect("переименование")),
            ));
        }
        if let Some(folder) = &manifest.folder_1000 {
            let from = path(&folder.path);
            let to = path(&format!("Перенесено/{}", folder.path));
            let old = from.clone();
            cases.push((
                "перенос папки на 1 000 заметок",
                PathRename { from, to },
                Box::new(move |v| v.move_entry(&old, &path("Перенесено")).expect("перенос")),
            ));
        }
        for (what, rename, act) in &cases {
            let (mut planned, mut executed, mut total) = (Vec::new(), Vec::new(), Vec::new());
            let mut counts = (0, 0);
            for repeat in 0..REPEATS {
                let root = dir.path().join(format!("vault-{repeat}"));
                copy_dir(&pristine, &root);
                let vault = Vault::open(&root).expect("хранилище");
                let index = index_of(&vault);
                let before = Links::new(vault.notes().expect("заметки"));
                let renames = [rename.clone()];
                let (plan_ms, relink) = timed(|| plan(&index, &before, &renames, &[]));
                counts = (relink.note_count(), relink.link_count());
                let (run_ms, outcome) = timed(|| {
                    let to = act(&vault);
                    assert_eq!(to, renames[0].to, "{what}");
                    let after = Links::new(vault.notes().expect("заметки"));
                    run(&vault, &after.view(), &relink, &renames, &mut |_, _| {})
                });
                assert!(outcome.failed.is_empty(), "{what}: {:?}", outcome.failed);
                assert_eq!(outcome.updated.len(), counts.0, "{what}");
                planned.push(plan_ms);
                executed.push(run_ms);
                total.push(plan_ms + run_ms);
                drop(vault);
                let _ = std::fs::remove_dir_all(&root);
            }
            let label = format!("{what} ({} заметок, {} ссылок)", counts.0, counts.1);
            report.measure(&format!("{label}: план"), &planned, "мс", None, false);
            report.measure(
                &format!("{label}: переименование на диске + выполнение с журналом"),
                &executed,
                "мс",
                None,
                false,
            );
            report.measure(&format!("{label}: всего"), &total, "мс", None, false);
        }
        report.finish();
    }
}

/// План и выполнение 10 000 внешних переименований на 10 000 заметок,
/// каждая ссылается на следующую (как `external_tests::
/// perf_external_plan_10000_renames`, ревью безопасности 0.2-6.1), —
/// справочно. Каждый повтор — новое хранилище (выполнение переписывает
/// все заметки).
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_relink_external_10000() {
    const N: usize = 10_000;
    let mut report = Report::new(&format!(
        "perf-core внешние переименования (справочно), {N} переименований на {N} заметок"
    ));
    let (mut planned, mut locked, mut executed) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..REPEATS {
        let dir = tempfile::tempdir().expect("папка");
        // На диске — уже после переименований «н» → «м»; ссылки — на «н».
        for i in 0..N {
            let text = format!("[[н{}]]\n", (i + 1) % N);
            std::fs::write(dir.path().join(format!("м{i}.md")), text).expect("заметка");
        }
        let vault = Vault::open(dir.path()).expect("хранилище");
        let index = index_of(&vault);
        let now = vault.notes().expect("заметки");
        let renames: Vec<PathRename> = (0..N)
            .map(|i| PathRename {
                from: path(&format!("н{i}.md")),
                to: path(&format!("м{i}.md")),
            })
            .collect();
        let (plan_ms, (relink, kept, lock_ms)) = timed(|| {
            let prepared = external::prepare(&now, &renames);
            let (lock_ms, snapshot) = timed(|| external::snapshot(&index, &prepared));
            let (relink, kept, dropped) = prepared.finish(&snapshot);
            assert!(dropped.is_empty());
            (relink, kept, lock_ms)
        });
        assert_eq!(relink.link_count(), N);
        let after = Links::new(now);
        let (run_ms, outcome) =
            timed(|| run(&vault, &after.view(), &relink, &kept, &mut |_, _| {}));
        assert!(outcome.failed.is_empty(), "{:?}", &outcome.failed[..1]);
        assert_eq!(outcome.updated.len(), N);
        planned.push(plan_ms);
        locked.push(lock_ms);
        executed.push(run_ms);
    }
    report.measure(
        "план (prepare + snapshot + finish)",
        &planned,
        "мс",
        None,
        false,
    );
    report.measure(
        "  из него под замком индекса (snapshot)",
        &locked,
        "мс",
        None,
        false,
    );
    report.measure(
        "выполнение плана: 10 000 заметок с журналом",
        &executed,
        "мс",
        None,
        false,
    );
    report.finish();
}
