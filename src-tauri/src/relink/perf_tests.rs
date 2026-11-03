//! Замеры переписывания ссылок (задача 0.2-7.1, второй прогон — перед КТ2;
//! С2-8.3: переименование заметки со 100 ссылающимися ≤ 2 с, перенос папки
//! на 1 000 заметок с 5 000 ссылками ≤ 15 с, 50 000 — ≤ 20 с).
//!
//! **Каркас.** Сейчас меряется только план ([`plan()`]) на хранилище
//! генератора — справочно, без пределов: цели относятся ко всему пути «от
//! ответа до уведомления», а выполнение плана с журналом (0.2-4.5) и
//! команды (0.2-4.6) ещё не в `main`. Во втором прогоне сюда добавляются
//! выполнение плана (запись файлов) и пределы через
//! `Report::measure(.., checked)`.
//!
//! Методика и помощники — `index::perf_tests`.

use std::sync::RwLock;

use super::plan;
use crate::commands::{PathRename, VaultPath};
use crate::index::perf_tests::{REPEATS, Report, scales, timed};
use crate::index::{VaultIndex, rebuild};
use crate::links::Links;
use crate::vault::Vault;

fn path(s: &str) -> VaultPath {
    VaultPath::from_components(s.split('/'))
}

/// План переименования «узла» и переноса папки на 1 000 заметок
/// (справочно, каркас второго прогона).
#[test]
#[ignore = "замер: --release --ignored perf_ (шаг CI «Замеры ядра»)"]
fn perf_relink_plan_skeleton() {
    for notes in scales() {
        let mut report = Report::new(&format!(
            "perf-core план переписывания ссылок (каркас, справочно), {notes} заметок"
        ));
        let dir = tempfile::tempdir().expect("папка");
        let root = dir.path().join("vault");
        let manifest =
            gen_vault::generate(&root, &gen_vault::Config::new(notes, 7)).expect("генератор");
        let vault = Vault::open(&root).expect("хранилище");
        let index = RwLock::new(VaultIndex::new());
        rebuild(&vault, &index, &|_| {}).expect("собран");
        let index = index.into_inner().expect("индекс");
        let links = Links::new(vault.notes().expect("заметки"));

        let mut cases = Vec::new();
        if let Some(hub) = &manifest.hub {
            let to = hub.path.replace("Узел", "Узел переименован");
            cases.push(("переименование «узла»", hub.path.clone(), to));
        }
        if let Some(folder) = &manifest.folder_1000 {
            cases.push((
                "перенос папки на 1 000 заметок",
                folder.path.clone(),
                format!(
                    "Перенесено/{}",
                    folder.path.rsplit('/').next().unwrap_or("п")
                ),
            ));
        }
        for (what, from, to) in cases {
            let renames = [PathRename {
                from: path(&from),
                to: path(&to),
            }];
            let mut took = Vec::new();
            let mut counts = (0, 0);
            for _ in 0..REPEATS {
                let (ms, planned) = timed(|| plan(&index, &links, &renames, &[]));
                counts = (planned.note_count(), planned.link_count());
                took.push(ms);
            }
            report.measure(
                &format!("план: {what} ({} заметок, {} ссылок)", counts.0, counts.1),
                &took,
                "мс",
                None,
                false,
            );
        }
        report.finish();
    }
}
