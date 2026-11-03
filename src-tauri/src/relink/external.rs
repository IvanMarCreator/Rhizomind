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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;

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
/// заметок `now` и индексу `index` **после** них. Возвращает план,
/// переименования, по которым он посчитан, и выпавшие (см. описание
/// модуля) — по ним вызывающий сообщает о сбое `conflict`, а не молчит.
/// Диск не читается.
///
/// Стоимость — O(R·log N + перемещённые заметки) на обратный ход (поиск
/// по ключу в упорядоченном списке путей) и сам [`plan()`](super::plan);
/// индекс копируется, только если путь перемещённой заметки после пачки
/// совпал с путём другой перемещённой заметки до пачки (обмен именами,
/// цикл) — иначе план по текущему индексу тот же.
pub fn plan(
    index: &VaultIndex,
    now: &[NoteRef],
    renames: &[PathRename],
) -> (RelinkPlan, Vec<PathRename>, Vec<PathRename>) {
    let Reversed {
        before,
        kept,
        dropped,
        moved,
    } = reverse(now, renames);
    if kept.is_empty() {
        return (RelinkPlan::default(), kept, dropped);
    }
    let before_keys: HashSet<&Vec<String>> =
        moved.values().map(|moved| &moved.before_key).collect();
    // Смена только регистра совпадает по ключу сама с собой — это не обмен.
    let collision = moved
        .iter()
        .any(|(current, moved)| *current != moved.before_key && before_keys.contains(current));
    // Итог пачки по каждой заметке «путь до → путь после»: проверен и
    // свёрнут обратным ходом.
    let mut moves: Vec<PathRename> = moved
        .values()
        .map(|moved| PathRename {
            from: moved.before.clone(),
            to: moved.current.clone(),
        })
        .collect();
    moves.sort_by(|a, b| a.from.components().cmp(b.from.components()));
    let before = Links::new(before);
    let plan = if collision {
        // Индекс «до»: записи переименованных заметок — под прежними путями.
        let index_before = VaultIndex::from_entries(index.entries().map(|(path, entry)| {
            let path = moved.get(&key(path)).map_or(path, |moved| &moved.before);
            (path.clone(), entry.clone())
        }));
        super::plan_moves(&index_before, &before, &moves)
    } else {
        super::plan_moves(index, &before, &moves)
    };
    (plan, kept, dropped)
}

/// Список заметок «до» и что для этого отыграно назад.
struct Reversed {
    before: Vec<NoteRef>,
    /// Отыгранные переименования, по порядку пачки.
    kept: Vec<PathRename>,
    /// Выпавшие переименования, по порядку пачки.
    dropped: Vec<PathRename>,
    /// Ключ текущего пути переименованной заметки → где она была.
    moved: HashMap<Vec<String>, Moved>,
}

/// Переименованная заметка: путь до пачки (и его ключ) и путь сейчас.
struct Moved {
    before_key: Vec<String>,
    before: VaultPath,
    current: VaultPath,
}

/// Ключи путей, начинающиеся с `prefix` (сам путь и всё под ним): в
/// упорядоченном списке они идут подряд, начиная с `prefix`.
fn under<'a>(
    by_key: &'a BTreeMap<Vec<String>, usize>,
    prefix: &'a [String],
) -> impl Iterator<Item = (&'a Vec<String>, &'a usize)> + 'a {
    by_key
        .range::<[String], _>((Bound::Included(prefix), Bound::Unbounded))
        .take_while(move |(key, _)| key.starts_with(prefix))
}

/// Обратный ход: переименования отыгрываются назад от последнего к первому
/// на копии списка заметок; заметки ищутся по ключу пути.
fn reverse(now: &[NoteRef], renames: &[PathRename]) -> Reversed {
    let mut notes: Vec<NoteRef> = now.to_vec();
    let origin: Vec<Vec<String>> = notes.iter().map(|note| key(&note.path)).collect();
    let mut by_key: BTreeMap<Vec<String>, usize> = origin
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), i))
        .collect();
    let mut touched = vec![false; notes.len()];
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for rename in renames.iter().rev() {
        let (from, to) = (key(&rename.from), key(&rename.to));
        let found: Vec<(Vec<String>, usize)> = under(&by_key, &to)
            .map(|(key, &i)| (key.clone(), i))
            .collect();
        // Переименованного больше нет; на старом месте уже другая заметка;
        // заметка пришла из не-заметки (`а.txt` → `а.md`).
        let occupied = || under(&by_key, &from).any(|(key, _)| !key.starts_with(&to));
        let back: Vec<VaultPath> = found
            .iter()
            .map(|&(_, i)| {
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
        if found.is_empty() || occupied() || back.iter().any(|path| !is_note(path)) {
            dropped.push(rename.clone());
            continue;
        }
        for ((old_key, i), path) in found.into_iter().zip(back) {
            by_key.remove(&old_key);
            by_key.insert(key(&path), i);
            notes[i].path = path;
            touched[i] = true;
        }
        kept.push(rename.clone());
    }
    kept.reverse();
    dropped.reverse();
    let moved = (0..notes.len())
        .filter(|&i| touched[i])
        .filter(|&i| notes[i].path != now[i].path)
        .map(|i| {
            (
                origin[i].clone(),
                Moved {
                    before_key: key(&notes[i].path),
                    before: notes[i].path.clone(),
                    current: now[i].path.clone(),
                },
            )
        })
        .collect();
    Reversed {
        before: notes,
        kept,
        dropped,
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
