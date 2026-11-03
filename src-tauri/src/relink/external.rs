//! Внешние переименования → обновление ссылок (задача 0.2-6.1; С2-3.5
//! «в Проводнике», Р10, решение 110).
//!
//! Две чистые части; очередь, вопрос пользователю и выполнение — в
//! `commands::relink_cmds` ([`ExternalRenames`](crate::commands::ExternalRenames)):
//! - [`confirmed`] — какие переименования пачки наблюдателя внешние. Только
//!   `TreeChange::Renamed`: наблюдатель выдаёт его лишь для
//!   **подтверждённого** переименования — тот же идентификатор файла и та
//!   же дата создания (ADR 0006, решение 89). «Удалили и создали» приходит
//!   как `Removed` + `Created` и ссылки не трогает. Свои переименования
//!   приложения узнаются по журналу своих переименований
//!   ([`OwnWrites::is_own_rename`], задача 0.2-4.2) и пропускаются.
//! - [`plan`] — план по **текущему** списку заметок и индексу: «до»
//!   восстанавливается обратным ходом переименований пачки. Так план можно
//!   посчитать в любой момент — когда индекс сверен (пачка, пришедшая во
//!   время сборки, ждёт), после ответа пользователя (состав мог
//!   измениться) или после записи ввода окна. Переименование, которое уже
//!   нельзя отыграть назад (переименованного больше нет, на старом месте
//!   появилась другая заметка, заметка ушла в не-заметку или пришла из
//!   не-заметки), из плана выпадает: по нему ничего не переписывается.

use std::collections::HashMap;

use super::RelinkPlan;
use crate::commands::{PathRename, TreeChange, VaultPath};
use crate::index::VaultIndex;
use crate::links::Links;
use crate::own_writes::OwnWrites;
use crate::vault::NoteRef;

/// Внешние подтверждённые переименования пачки `changes` — по порядку пачки
/// (пути «откуда» — с учётом предыдущих переименований той же пачки, как
/// их выдаёт наблюдатель). Свои (`own`) пропускаются; внешнее, уводящее
/// «куда» своей записи, эту запись забывает
/// ([`OwnWrites::forget_rename_by_external`]) — по порядку, поэтому своё
/// событие раньше внешнего в той же пачке ещё узнаётся.
pub fn confirmed(changes: &[TreeChange], own: &OwnWrites) -> Vec<PathRename> {
    let mut out = Vec::new();
    for change in changes {
        let TreeChange::Renamed { from, to } = change else {
            continue;
        };
        if own.is_own_rename(from, to) {
            continue;
        }
        own.forget_rename_by_external(from);
        out.push(PathRename {
            from: from.clone(),
            to: to.clone(),
        });
    }
    out
}

/// План для внешних переименований `renames` (по порядку) по списку
/// заметок `now` и индексу `index` **после** них. Возвращает план и
/// переименования, по которым он посчитан (выпавшие — см. описание
/// модуля). Диск не читается.
pub fn plan(
    index: &VaultIndex,
    now: &[NoteRef],
    renames: &[PathRename],
) -> (RelinkPlan, Vec<PathRename>) {
    let Reversed {
        before,
        kept,
        moved,
    } = reverse(now, renames);
    if kept.is_empty() {
        return (RelinkPlan::default(), kept);
    }
    // Индекс «до»: записи переименованных заметок — под прежними путями.
    let index_before = VaultIndex::from_entries(index.entries().map(|(path, entry)| {
        let path = moved.get(&key(path)).unwrap_or(path).clone();
        (path, entry.clone())
    }));
    let plan = super::plan(&index_before, &Links::new(before), &kept, &[]);
    (plan, kept)
}

/// Список заметок «до» и что для этого отыграно назад.
struct Reversed {
    before: Vec<NoteRef>,
    /// Отыгранные переименования, по порядку пачки.
    kept: Vec<PathRename>,
    /// Ключ текущего пути переименованной заметки → путь до пачки.
    moved: HashMap<Vec<String>, VaultPath>,
}

/// Обратный ход: переименования отыгрываются назад от последнего к первому
/// на копии списка заметок.
fn reverse(now: &[NoteRef], renames: &[PathRename]) -> Reversed {
    let mut notes: Vec<NoteRef> = now.to_vec();
    let mut keys: Vec<Vec<String>> = notes.iter().map(|note| key(&note.path)).collect();
    // Текущий путь каждой заметки — для индекса.
    let origin: Vec<VaultPath> = notes.iter().map(|note| note.path.clone()).collect();
    let mut touched = vec![false; notes.len()];
    let mut kept = Vec::new();
    for rename in renames.iter().rev() {
        let (from, to) = (key(&rename.from), key(&rename.to));
        let under_to: Vec<usize> = (0..notes.len())
            .filter(|&i| keys[i].starts_with(&to))
            .collect();
        if under_to.is_empty() {
            // Переименованного больше нет (или это не заметки).
            continue;
        }
        if (0..notes.len()).any(|i| !under_to.contains(&i) && keys[i].starts_with(&from)) {
            // На старом месте уже другая заметка.
            continue;
        }
        let back: Vec<VaultPath> = under_to
            .iter()
            .map(|&i| {
                VaultPath::from_components(
                    rename
                        .from
                        .components()
                        .iter()
                        .chain(&notes[i].path.components()[to.len()..])
                        .cloned(),
                )
            })
            .collect();
        if back.iter().any(|path| !is_note(path)) {
            // Заметка пришла из не-заметки (`а.txt` → `а.md`): до пачки её
            // не было.
            continue;
        }
        for (&i, path) in under_to.iter().zip(back) {
            keys[i] = key(&path);
            notes[i].path = path;
            touched[i] = true;
        }
        kept.push(rename.clone());
    }
    kept.reverse();
    let moved = (0..notes.len())
        .filter(|&i| touched[i] && notes[i].path != origin[i])
        .map(|i| (key(&origin[i]), notes[i].path.clone()))
        .collect();
    Reversed {
        before: notes,
        kept,
        moved,
    }
}

/// Путь по ключам имён компонентов (регистр и нормализация не важны).
fn key(path: &VaultPath) -> Vec<String> {
    path.components()
        .iter()
        .map(|c| crate::names::key(c).as_str().to_owned())
        .collect()
}

fn is_note(path: &VaultPath) -> bool {
    path.file_name().is_some_and(crate::names::is_note)
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;
