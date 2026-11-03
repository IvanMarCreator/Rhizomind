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
use crate::names::NameKey;
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
/// Диск не читается. Три шага, чтобы замок индекса держался только на
/// копию нужных записей: [`prepare`] (без индекса), [`snapshot`] (под
/// замком), [`Prepared::finish`] (без замков).
pub fn plan(
    index: &VaultIndex,
    now: &[NoteRef],
    renames: &[PathRename],
) -> (RelinkPlan, Vec<PathRename>, Vec<PathRename>) {
    let prepared = prepare(now, renames);
    let index = snapshot(index, &prepared);
    prepared.finish(&index)
}

/// Обратный ход сделан, индекс ещё не нужен.
pub struct Prepared {
    before: Links,
    /// Отыгранные переименования, по порядку пачки.
    kept: Vec<PathRename>,
    /// Выпавшие переименования, по порядку пачки.
    dropped: Vec<PathRename>,
    /// Итог пачки по каждой заметке «путь до → путь после» (проверен и
    /// свёрнут обратным ходом).
    moves: Vec<PathRename>,
    /// Путь переименованной заметки сейчас → путь до пачки.
    back: HashMap<VaultPath, VaultPath>,
    /// Ключи имён переименованных заметок до и после: только ссылки с
    /// таким именем цели могут попасть в план.
    names: HashSet<NameKey>,
}

/// Обратный ход по списку заметок `now` — O(R·log N + перемещённые
/// заметки): заметки ищутся в упорядоченном списке путей.
pub fn prepare(now: &[NoteRef], renames: &[PathRename]) -> Prepared {
    let Reversed {
        before,
        kept,
        dropped,
        moved,
    } = reverse(now, renames);
    let mut moves: Vec<PathRename> = moved
        .iter()
        .map(|(current, before)| PathRename {
            from: before.clone(),
            to: current.clone(),
        })
        .collect();
    moves.sort_by(|a, b| a.from.components().cmp(b.from.components()));
    let names = moves
        .iter()
        .flat_map(|m| [name_key(&m.from), name_key(&m.to)])
        .flatten()
        .collect();
    Prepared {
        before: Links::new(before),
        kept,
        dropped,
        moves,
        back: moved,
        names,
    }
}

/// Индекс «до» для плана — только записи заметок, в которых есть ссылки
/// на имена переименованных заметок (их и только их читает план), под
/// путями до пачки. Копия — это записи этих заметок; остальной индекс не
/// копируется.
pub fn snapshot(index: &VaultIndex, prepared: &Prepared) -> VaultIndex {
    let mut sources: HashSet<&VaultPath> = HashSet::new();
    for name in &prepared.names {
        sources.extend(index.links_named(name).map(|(from, _)| from));
    }
    VaultIndex::from_entries(sources.into_iter().filter_map(|path| {
        let entry = index.entry(path)?.clone();
        let path = prepared.back.get(path).unwrap_or(path).clone();
        Some((path, entry))
    }))
}

impl Prepared {
    /// План по индексу «до» `index` ([`snapshot`]).
    pub fn finish(self, index: &VaultIndex) -> (RelinkPlan, Vec<PathRename>, Vec<PathRename>) {
        if self.kept.is_empty() {
            return (RelinkPlan::default(), self.kept, self.dropped);
        }
        let plan = super::plan_moves(index, &self.before, &self.moves);
        (plan, self.kept, self.dropped)
    }
}

/// Список заметок «до» и что для этого отыграно назад.
struct Reversed {
    before: Vec<NoteRef>,
    kept: Vec<PathRename>,
    dropped: Vec<PathRename>,
    /// Путь переименованной заметки сейчас → путь до пачки.
    moved: HashMap<VaultPath, VaultPath>,
}

/// Пути с ключом, начинающимся с `prefix` (сам путь и всё под ним): в
/// упорядоченном списке они идут подряд, начиная с `prefix`.
fn under<'a>(
    by_key: &'a BTreeMap<Vec<String>, Vec<usize>>,
    prefix: &'a [String],
) -> impl Iterator<Item = (&'a Vec<String>, &'a Vec<usize>)> + 'a {
    by_key
        .range::<[String], _>((Bound::Included(prefix), Bound::Unbounded))
        .take_while(move |(key, _)| key.starts_with(prefix))
}

/// Обратный ход: переименования отыгрываются назад от последнего к первому
/// на копии списка заметок; заметки ищутся по ключу пути ([`key`]).
fn reverse(now: &[NoteRef], renames: &[PathRename]) -> Reversed {
    let mut notes: Vec<NoteRef> = now.to_vec();
    let mut by_key: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    for (i, note) in notes.iter().enumerate() {
        by_key.entry(key(&note.path)).or_default().push(i);
    }
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for rename in renames.iter().rev() {
        let (from, to) = (key(&rename.from), key(&rename.to));
        let found: Vec<(Vec<String>, usize)> = under(&by_key, &to)
            .flat_map(|(key, ids)| ids.iter().map(|&i| (key.clone(), i)))
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
            if let Some(ids) = by_key.get_mut(&old_key) {
                ids.retain(|&j| j != i);
                if ids.is_empty() {
                    by_key.remove(&old_key);
                }
            }
            by_key.entry(key(&path)).or_default().push(i);
            notes[i].path = path;
        }
        kept.push(rename.clone());
    }
    kept.reverse();
    dropped.reverse();
    let moved = (0..notes.len())
        .filter(|&i| notes[i].path != now[i].path)
        .map(|i| (now[i].path.clone(), notes[i].path.clone()))
        .collect();
    Reversed {
        before: notes,
        kept,
        dropped,
        moved,
    }
}

/// Ключ имени заметки без `.md`; не заметка — `None` (как в плане).
fn name_key(path: &VaultPath) -> Option<NameKey> {
    crate::switcher::link_path(path)?
        .last()
        .map(|name| crate::names::key(name))
}

/// Ключ пути для обратного хода. Windows и macOS: ключи имён компонентов
/// (регистр и нормализация Unicode не важны — пути событий ОС и списка
/// заметок могут различаться написанием, двойников нет). Linux: точное
/// написание — `Встреча.md` и `встреча.md` там две разные заметки, и
/// ключ имени склеил бы их (ложный `conflict`); пути событий наблюдателя
/// и списка заметок там совпадают побайтно.
fn key(path: &VaultPath) -> Vec<String> {
    path.components()
        .iter()
        .map(|c| {
            if cfg!(target_os = "linux") {
                c.clone()
            } else {
                crate::names::key(c).as_str().to_owned()
            }
        })
        .collect()
}

fn is_note(path: &VaultPath) -> bool {
    path.file_name().is_some_and(crate::names::is_note)
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;
