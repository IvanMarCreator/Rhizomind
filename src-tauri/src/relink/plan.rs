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
/// `renames` — события по порядку (как их видит наблюдатель). Они
/// выполняются по одному на модели списка заметок (`accept`), и план
/// считается, как в [`plan_moves`], по итоговым перемещениям заметок:
/// цепочка `А → Б`, `Б → В` даёт `А → В`, «туда и обратно» — ничего,
/// заметка, вернувшаяся на место после переименования папки, остаётся.
/// `replaced` — заметки списка «до», которые пачка затёрла (перенос с
/// заменой в Проводнике: тот же идентификатор у переносимого файла); их нет
/// в списке «после», ссылки на них не переписываются (как при удалении).
/// Переименование, которое не сопоставить со списком «до» (см. `matched`:
/// «откуда» нет, «куда» занято без пометки о замене, повтор уже
/// выполненного), ничего не переписывает и возвращается в
/// [`RelinkPlan::unmatched`] — вызывающий сообщает о сбое. Заметка,
/// перенесённая в не-заметку (`.md` → `.txt`), из списка «после» пропадает
/// (как при удалении). Тот же вход — тот же план.
///
/// Допущение: «откуда», которого нет в точном написании, сопоставляется по
/// ключу. На Linux, где рядом бывают `А.md` и `а.md`, повтор уже
/// выполненного переименования может так попасть на двойника; в Windows и
/// macOS двойников нет.
///
/// Заметки плана — по пути (после изменения), ссылки заметки — по порядку
/// в тексте; одна и та же ссылка, повторённая в тексте, — столько раз,
/// сколько повторена. Новая цель, которую нельзя написать в ссылке
/// (`names::is_linkable`) или которая не ведёт на заметку однозначно
/// (Linux: две заметки с одним ключом пути), в план не попадает: правкой
/// ссылку не исправить.
///
/// Требование к выполнению плана (0.2-4.5): ссылка уже переписана, если её
/// цель в свежем тексте однозначно ведёт в `points_to` по списку «после».
/// Переписывать только ссылки с целью, равной `old_target`, которые туда
/// однозначно не ведут. Журнал хранит по заметке тройки (`old_target`,
/// `new_target`, `points_to`). Сравнивать цели без этой проверки нельзя:
/// после переименования папки новая цель одной ссылки может совпасть со
/// старой целью другой в той же заметке (`[[Альфа/Встреча]]`).
pub fn plan(
    index: &VaultIndex,
    before: &Links,
    renames: &[PathRename],
    replaced: &[VaultPath],
) -> RelinkPlan {
    let replaced: Vec<VaultPath> = replaced
        .iter()
        .filter_map(|path| before.spelled(path))
        .collect();
    let (moves, unmatched) = accept(before, renames, &replaced);
    let after = before.after_replacing(&replaced, &moves);
    plan_after(index, before, &replaced, &after, unmatched)
}

/// Как [`plan`], но для уже проверенных и свёрнутых перемещений отдельных
/// заметок: `moves` — пары «путь заметки в списке `before` → путь после»,
/// без цепочек и папок (внешние переименования, 0.2-6.1: их проверяет и
/// сворачивает обратный ход по списку заметок). Проверка событий по
/// порядку (`accept`) пропускается — она квадратична по числу событий, а
/// здесь событий может быть десятки тысяч.
pub fn plan_moves(index: &VaultIndex, before: &Links, moves: &[PathRename]) -> RelinkPlan {
    let after = before.after_renames(moves);
    plan_after(index, before, &[], &after, Vec::new())
}

/// Общая часть [`plan`] и [`plan_moves`]: решения по ссылкам при списке
/// «после» `after`.
fn plan_after(
    index: &VaultIndex,
    before: &Links,
    replaced: &[VaultPath],
    after: &NotesView<'_>,
    unmatched: Vec<PathRename>,
) -> RelinkPlan {
    let moves: HashMap<&VaultPath, &VaultPath> =
        after.moves().iter().map(|(from, to)| (from, to)).collect();
    let names: HashSet<NameKey> = moves
        .iter()
        .flat_map(|(from, to)| [name_key(from), name_key(to)])
        .flatten()
        .collect();
    let rules = Rules {
        before: before.view(),
        after,
        moves: &moves,
    };
    let mut decided: HashMap<&str, Option<(VaultPath, String)>> = HashMap::new();
    let mut by_note: HashMap<VaultPath, Vec<(usize, PlannedLink)>> = HashMap::new();
    for name in &names {
        for (from, link) in index.links_named(name) {
            let note = match moves.get(from) {
                Some(&to) => to,
                None => from,
            };
            if replaced.contains(from) || !after.contains(note) {
                continue;
            }
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
    RelinkPlan { notes, unmatched }
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

/// Проверяет события по порядку на состоянии «список до, затёртые
/// `replaced` убраны, принятые события выполнены» — как их выполнил бы диск
/// (Windows): «откуда» есть (точно, иначе единственное написание по ключам
/// компонентов: регистр, NFC/NFD в путях событий ОС); «куда» не занято по
/// ключу ничем, кроме переносимого (смена регистра — можно), в том числе
/// заметкой, которую принесло прошлое событие пачки; не внутрь себя;
/// «откуда» по ключу не совпадает с «куда» в точном написании (повтор уже
/// выполненной смены регистра).
/// Возвращает итоговые перемещения заметок после принятых событий
/// («путь в списке до → путь после», вернувшиеся на место не входят) и
/// непринятые события как есть. Занятость проверяется по состоянию после
/// прошлых событий, поэтому звено цепочки на занятую цель помечается, а
/// обмен через временное имя проходит.
fn accept(
    before: &Links,
    renames: &[PathRename],
    replaced: &[VaultPath],
) -> (Vec<PathRename>, Vec<PathRename>) {
    let mut state = Overlay {
        before,
        gone: replaced.to_vec(),
        arrived: Vec::new(),
    };
    let mut unmatched = Vec::new();
    for event in renames {
        if state.step(event).is_none() {
            unmatched.push(event.clone());
        }
    }
    let moves = state
        .arrived
        .into_iter()
        .filter(|(origin, now)| origin != now)
        .map(|(from, to)| PathRename { from, to })
        .collect();
    (moves, unmatched)
}

/// Состояние пачки поверх списка «до»: ушедшие заметки списка и заметки,
/// перенесённые принятыми событиями («путь в списке до», «путь сейчас»).
struct Overlay<'a> {
    before: &'a Links,
    gone: Vec<VaultPath>,
    arrived: Vec<(VaultPath, VaultPath)>,
}

fn keys(path: &[String]) -> Vec<NameKey> {
    path.iter().map(|c| names::key(c)).collect()
}

/// `path` — это `at` или внутри `at`.
fn at_or_inside(path: &[String], at: &[String]) -> bool {
    path.len() >= at.len() && path.starts_with(at)
}

impl Overlay<'_> {
    /// Выполняет событие; `None` — невыполнимо (состояние не меняется).
    fn step(&mut self, event: &PathRename) -> Option<()> {
        let (from, to) = (event.from.components(), event.to.components());
        if from.is_empty() || to.is_empty() {
            return None;
        }
        let (from_keys, to_keys) = (keys(from), keys(to));
        if to_keys.len() > from_keys.len() && to_keys.starts_with(&from_keys) {
            return None;
        }
        let gone = &self.gone;
        let present = |path: &VaultPath| {
            !gone
                .iter()
                .any(|g| at_or_inside(path.components(), g.components()))
        };
        let (exact, keyed) = self.before.notes_at(&event.from);
        let exact_b: Vec<VaultPath> = exact.into_iter().filter(|p| present(p)).collect();
        let keyed_b: Vec<VaultPath> = keyed.into_iter().filter(|p| present(p)).collect();
        let exact_a: Vec<usize> = (0..self.arrived.len())
            .filter(|&i| at_or_inside(self.arrived[i].1.components(), from))
            .collect();
        let keyed_a: Vec<usize> = (0..self.arrived.len())
            .filter(|&i| keys(self.arrived[i].1.components()).starts_with(&from_keys))
            .collect();
        let (moving_b, moving_a) = if exact_b.is_empty() && exact_a.is_empty() {
            (keyed_b, keyed_a)
        } else {
            (exact_b, exact_a)
        };
        let prefix = |path: &VaultPath| {
            VaultPath::from_components(path.components()[..from.len()].iter().cloned())
        };
        let spellings: HashSet<VaultPath> = moving_b
            .iter()
            .chain(moving_a.iter().map(|&i| &self.arrived[i].1))
            .map(prefix)
            .collect();
        if spellings.len() != 1 {
            return None;
        }
        let canon = spellings.into_iter().next()?;
        if canon == event.to && event.from != event.to {
            // «Откуда» в другом написании уже стоит как «куда»: повтор
            // выполненной смены регистра.
            return None;
        }
        let mut freed = self.gone.clone();
        freed.extend(moving_b.iter().cloned());
        let busy_b = self.before.occupied(&event.to, &freed);
        let busy_a = (0..self.arrived.len()).any(|i| {
            !moving_a.contains(&i) && keys(self.arrived[i].1.components()).starts_with(&to_keys)
        });
        if busy_b || busy_a {
            return None;
        }
        let moved = |path: &VaultPath| {
            VaultPath::from_components(to.iter().chain(&path.components()[from.len()..]).cloned())
        };
        for &i in &moving_a {
            self.arrived[i].1 = moved(&self.arrived[i].1);
        }
        self.arrived
            .extend(moving_b.iter().map(|path| (path.clone(), moved(path))));
        self.gone.extend(moving_b);
        Some(())
    }
}

/// Ключ имени заметки без `.md`; не заметка — `None`.
fn name_key(path: &VaultPath) -> Option<NameKey> {
    crate::switcher::link_path(path)?
        .last()
        .map(|name| names::key(name))
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
