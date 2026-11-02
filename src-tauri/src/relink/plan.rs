//! План переписывания ссылок (задача 0.2-4.3; С2-3.5, С2-3.6, С2-5.4 шаг 1,
//! Р2, Р3, Р11, Р17 п. 9–10).
//!
//! Чистая функция индекса, списка заметок «до» и переименований: диск не
//! читается, общее состояние не меняется. Правило «ссылка ведёт туда же»:
//! ссылка, которая до операции вела на заметку `P` (по правилам С3.4),
//! переписывается на цель, как у автодополнения для `P` после операции
//! (Р3), только если без правки она вела бы не туда (неразрешённая или на
//! другую заметку — в том числе ссылка на другую заметку, которую
//! «перехватила» бы переименованная) или стала бы неоднозначной, хотя была
//! однозначной (С2-3.6). Остальные ссылки не трогаются: перенос заметки с
//! уникальным именем и смена регистра ничего не меняют (Р2), уточнённые
//! путём ссылки обратно не упрощаются (Р11).
//!
//! Какие ссылки смотреть: только те, у которых ключ имени цели — прежнее
//! или новое имя перемещённой заметки ([`VaultIndex::links_named`]).
//! Ссылка с другим именем цели не может ни вести на перемещённую заметку,
//! ни быть перехваченной ею. Решение зависит только от текста цели (С3.4:
//! заметка со ссылкой на выбор не влияет), поэтому считается один раз на
//! цель.

use std::collections::{HashMap, HashSet};

use super::{NotePlan, PlannedLink, RelinkPlan};
use crate::commands::{PathRename, VaultPath};
use crate::index::VaultIndex;
use crate::links::{Links, NotesView};
use crate::names::{self, NameKey};

/// План для переименований `renames` (заметок и папок): ссылки на
/// переименованное, ссылки на другие заметки, которые «перескочили» бы, и
/// ставшие неоднозначными (С2-3.5, С2-3.6). `before` — список заметок до
/// изменения; список «после» строится наложением
/// ([`Links::after_renames`]), без копии общего списка. Диск не читается.
///
/// `renames` — события по порядку (как их видит наблюдатель): цепочки
/// `А → Б`, `Б → В` сворачиваются в `А → В`, «туда и обратно» — в ничто.
/// Путь «откуда» в другом регистре или форме Unicode сопоставляется по
/// ключу — но только если «куда» ещё нет в списке «до»: иначе это повтор
/// уже выполненного переименования (событие пришло ещё раз), и такое
/// переименование пропускается. Заметка, перенесённая в
/// не-заметку (`.md` → `.txt`), из списка «после» пропадает: ссылки на неё
/// не переписываются (как при удалении).
///
/// Заметки плана — по пути (после изменения), ссылки заметки — по порядку
/// в тексте; одна и та же ссылка, повторённая в тексте, — столько раз,
/// сколько повторена. Новая цель, которую нельзя написать в ссылке
/// (`names::is_linkable`) или которая не ведёт на заметку однозначно
/// (Linux: две заметки с одним ключом пути), в план не попадает: правкой
/// ссылку не исправить.
pub fn plan(index: &VaultIndex, before: &Links, renames: &[PathRename]) -> RelinkPlan {
    let renames: Vec<PathRename> = collapse(renames)
        .into_iter()
        .filter_map(|rename| spelled(before, rename))
        .collect();
    if renames.is_empty() {
        return RelinkPlan::default();
    }
    let after = before.after_renames(&renames);
    let moves: HashMap<&VaultPath, &VaultPath> =
        after.moves().iter().map(|(from, to)| (from, to)).collect();
    if moves.is_empty() {
        return RelinkPlan::default();
    }
    let names: HashSet<NameKey> = moves
        .iter()
        .flat_map(|(from, to)| [name_key(from), name_key(to)])
        .flatten()
        .collect();
    let rules = Rules {
        before: before.view(),
        after: &after,
        moves: &moves,
    };
    let mut decided: HashMap<&str, Option<(VaultPath, String)>> = HashMap::new();
    let mut by_note: HashMap<VaultPath, Vec<(usize, PlannedLink)>> = HashMap::new();
    for name in &names {
        for (from, link) in index.links_named(name) {
            let note = match moves.get(from) {
                Some(&to) if !after.contains(to) => continue,
                Some(&to) => to,
                None => from,
            };
            let decision = decided
                .entry(link.target.as_str())
                .or_insert_with(|| rules.decide(&link.target));
            if let Some((points_to, new_target)) = decision {
                by_note.entry(note.clone()).or_default().push((
                    link.target_span.start,
                    PlannedLink {
                        old_target: link.target.clone(),
                        new_target: new_target.clone(),
                        points_to: points_to.clone(),
                    },
                ));
            }
        }
    }
    let mut notes: Vec<NotePlan> = by_note
        .into_iter()
        .map(|(path, mut links)| {
            links.sort_by_key(|(at, _)| *at);
            NotePlan {
                path,
                links: links.into_iter().map(|(_, link)| link).collect(),
            }
        })
        .collect();
    notes.sort_by(|a, b| a.path.components().cmp(b.path.components()));
    RelinkPlan { notes }
}

/// Списки «до» и «после» и перемещения — для решения по одной цели.
struct Rules<'a> {
    before: NotesView<'a>,
    after: &'a NotesView<'a>,
    moves: &'a HashMap<&'a VaultPath, &'a VaultPath>,
}

impl Rules<'_> {
    /// Куда должна вести ссылка с целью `target` и на что её переписать;
    /// `None` — переписывать не нужно (или нечем).
    fn decide(&self, target: &str) -> Option<(VaultPath, String)> {
        let was = self.before.candidates(target);
        let first = was.first()?;
        let now = self.moves.get(first).map_or(first, |to| *to);
        if !self.after.contains(now) {
            return None;
        }
        let hits = self.after.candidates(target);
        let jumps = hits.first() != Some(now);
        let ambiguous = hits.len() > 1 && was.len() == 1;
        if !(jumps || ambiguous) {
            return None;
        }
        let new_target = self.after.shortest_unique_path(now).ok()?;
        let linkable = new_target
            .strip_prefix('/')
            .unwrap_or(&new_target)
            .split('/')
            .all(names::is_linkable);
        let leads_back = self.after.candidates(&new_target).as_slice() == std::slice::from_ref(now);
        (linkable && leads_back).then(|| (now.clone(), new_target))
    }
}

/// Переименование в написании списка «до» ([`Links::after_renames`]
/// сравнивает пути точно). «Откуда» нет в точном написании — сопоставление
/// по ключу ([`Links::spelled`]), если «куда» ещё нет в списке; если есть —
/// это повтор уже выполненного переименования, и по ключу «откуда» попало
/// бы на другую заметку: `None`. Нечего переименовывать — тоже `None`.
fn spelled(before: &Links, rename: PathRename) -> Option<PathRename> {
    if before.has_path(&rename.from) {
        return Some(rename);
    }
    if before.has_path(&rename.to) {
        return None;
    }
    let from = before.spelled(&rename.from)?;
    Some(PathRename { from, ..rename })
}

/// Ключ имени заметки без `.md`; не заметка — `None`.
fn name_key(path: &VaultPath) -> Option<NameKey> {
    crate::switcher::link_path(path)?
        .last()
        .map(|name| names::key(name))
}

/// Есть ли `path` внутри папки `folder` (строго глубже).
fn inside<'p>(path: &'p [String], folder: &[String]) -> Option<&'p [String]> {
    (path.len() > folder.len() && path.starts_with(folder)).then(|| &path[folder.len()..])
}

/// Сворачивает события по порядку в итоговые «откуда → куда» для
/// [`Links::after_renames`] (там первое переименование, задевающее
/// заметку, главнее): переименование того, что уже переименовано, меняет
/// «куда» прежнего; переименование заметки внутри переименованной папки —
/// своё переименование от исходного пути, раньше папки; переименования,
/// вернувшие всё на место, убираются.
fn collapse(renames: &[PathRename]) -> Vec<PathRename> {
    let mut out: Vec<PathRename> = Vec::new();
    for event in renames {
        let from = event.from.components();
        if from.is_empty() {
            continue;
        }
        let mut renamed_itself = false;
        let mut inner: Option<(usize, PathRename)> = None;
        for (i, done) in out.iter_mut().enumerate() {
            let current = done.to.components();
            if current == from {
                done.to = event.to.clone();
                renamed_itself = true;
            } else if let Some(rest) = inside(current, from) {
                done.to = VaultPath::from_components(event.to.components().iter().chain(rest));
            } else if let Some(rest) = inside(from, current) {
                inner.get_or_insert_with(|| {
                    let original = done.from.components().iter().chain(rest);
                    (
                        i,
                        PathRename {
                            from: VaultPath::from_components(original),
                            to: event.to.clone(),
                        },
                    )
                });
            }
        }
        if renamed_itself {
            continue;
        }
        match inner {
            Some((i, rename)) => out.insert(i, rename),
            None => out.push(event.clone()),
        }
    }
    out.retain(|rename| rename.from != rename.to);
    out
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
