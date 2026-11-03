//! Уточнение ссылок путём при повторе имени (задача 0.2-6.2; С2-3.6, Р11,
//! решение 111).
//!
//! Чистые части; поток, очередь планов, вопрос и ответ — в
//! `commands::relink_cmds` ([`DuplicateWatch`](crate::commands::DuplicateWatch)).
//!
//! **Что запускает уточнение.** Только изменение состава заметок: заметка
//! появилась, переименована, перемещена или удалена ([`Batch::observe`]).
//! Изменение текста (`TreeChange::Modified`) состав не меняет и уточнения не
//! запускает никогда — ни правка, ни откат правки Git, ни своя запись ядра
//! (свои записи наблюдатель узнаёт по журналу своих записей и `Modified` по
//! ним не выдаёт, ADR 0006). Поэтому «войны» нет: переписанная ядром
//! заметка, возвращённая снаружи к старому тексту, новой волны не вызывает.
//! Свои изменения состава (создание заметки, переименование в приложении)
//! приходят от наблюдателя так же, как внешние, — механизм один; их
//! действие на список заметок команда уже применила, это учтено.
//!
//! **Пачка** ([`Batch`]) — что изменилось в составе с последнего
//! уточнения: какие заметки списка появились (`Born`) и какие пришли с
//! другого пути (`From`). Размер — не больше числа заметок до и после
//! пачки. Пачка закрывается после затишья [`QUIET`] (поток), план — один
//! на пачку.
//!
//! **План** ([`prepare`] → [`snapshot`] → [`Prepared::finish`]): список
//! «до» — текущий список без появившихся заметок и с пришедшими на прежних
//! путях (наложение `Links::with_changes`, без копии общего списка). Ссылка
//! с целью `T` переписывается на кратчайший различающий путь заметки `P`,
//! на которую вела до пачки, если `P` есть и сейчас, `T` по-прежнему
//! подходит к `P` и при этом либо стала неоднозначной (до пачки подходила
//! одной заметке, теперь больше чем одной), либо «перескочила» бы на
//! появившуюся более старую заметку. Ссылки, которые ведут на ушедшую под
//! другое имя заметку, — дело обновления после переименования (С2-3.5,
//! настройка `rename_links`), здесь не трогаются. Смотрятся только ссылки с
//! именем цели, как у появившихся и пришедших заметок
//! (`VaultIndex::links_named`).
//!
//! **Обратного упрощения нет (Р11):** удаление повтора ничего не
//! переписывает — ушедшие заметки в имена пачки не входят.
//!
//! Ссылка, которую уточнить нельзя (новую цель не написать в ссылке или она
//! не ведёт однозначно на заметку), — в [`Found::unfixable`]: вызывающий
//! сообщает о ней сбоем `conflict` (Р20), а не молчит.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super::{NotePlan, PlannedLink, RelinkPlan};
use crate::commands::{EntryKind, TreeChange, VaultPath};
use crate::index::VaultIndex;
use crate::links::{Links, NotesView};
use crate::names::{self, NameKey};
use crate::vault::NoteRef;

/// Затишье: столько без новых изменений состава — и пачка закрывается
/// (С2-3.6).
pub const QUIET: Duration = Duration::from_secs(2);

/// До стольких заметок уточнение выполняется сразу; больше — вопрос
/// `links:pending {reason: duplicate}` (решение 111).
pub const MAX_IMMEDIATE: usize = 20;

/// Ключ пути — ключи имён компонентов (как в списке заметок): события
/// наблюдателя приходят в написании с диска, команды — в написании
/// интерфейса.
type Key = Vec<String>;

fn key(path: &VaultPath) -> Key {
    path.components()
        .iter()
        .map(|c| names::key(c).as_str().to_owned())
        .collect()
}

fn is_note(path: &VaultPath) -> bool {
    path.file_name().is_some_and(names::is_note)
}

/// `path` после переименования `from → to` (`path` — `from` или под ним).
fn moved(path: &VaultPath, from: &VaultPath, to: &VaultPath) -> VaultPath {
    VaultPath::from_components(
        to.components()
            .iter()
            .chain(&path.components()[from.components().len()..])
            .cloned(),
    )
}

/// Откуда заметка списка взялась в пачке.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    /// Появилась (создана, скопирована, пришла из не-заметки).
    Born,
    /// Была в списке до пачки по этому пути.
    From(VaultPath),
}

/// Изменения состава заметок с последнего уточнения.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    /// Ключ пути заметки сейчас → откуда она. Заметки, которых здесь нет,
    /// лежат там же, где до пачки.
    origin: HashMap<Key, Origin>,
    /// Пути (до пачки) заметок, ушедших за пачку, — для сложения пачек.
    /// Заметка, созданная снова по тому же пути, — новая заметка: наблюдатель
    /// замену файла на месте выдаёт как `Modified`, а не как удаление и
    /// создание.
    left: HashSet<Key>,
    /// Состав менялся.
    touched: bool,
}

impl Batch {
    /// Состав менялся с последнего уточнения.
    pub fn is_touched(&self) -> bool {
        self.touched
    }

    /// Учитывает изменение дерева `change`; `notes_under(путь)` — заметки
    /// списка по пути и под ним **до** этого изменения. `true` — изменение
    /// состава (текст не в счёт). Своё изменение, которое команда уже
    /// применила к списку (переименование: под `from` пусто, а под `to` —
    /// перенесённые), учитывается так же.
    pub fn observe(
        &mut self,
        change: &TreeChange,
        notes_under: &dyn Fn(&VaultPath) -> Vec<VaultPath>,
    ) -> bool {
        let composition = match change {
            TreeChange::Modified { .. } => false,
            TreeChange::Created { path, entry_kind } => {
                if *entry_kind == EntryKind::Note && is_note(path) {
                    self.appear(path);
                    true
                } else {
                    false
                }
            }
            TreeChange::Removed { path } => {
                let prefix = key(path);
                let mut keys: Vec<Key> = notes_under(path).iter().map(key).collect();
                keys.extend(
                    self.origin
                        .keys()
                        .filter(|k| k.starts_with(&prefix))
                        .cloned(),
                );
                keys.sort();
                keys.dedup();
                for k in &keys {
                    self.gone(k);
                }
                !keys.is_empty()
            }
            TreeChange::Renamed { from, to } => self.rename(from, to, notes_under),
        };
        self.touched |= composition;
        composition
    }

    /// Полная сверка: список был `old`, стал `new` (пути заметок). `true` —
    /// состав изменился.
    pub fn rescan(&mut self, old: &[VaultPath], new: &[VaultPath]) -> bool {
        let old_keys: HashSet<Key> = old.iter().map(key).collect();
        let new_keys: HashSet<Key> = new.iter().filter(|p| is_note(p)).map(key).collect();
        let mut gone: Vec<Key> = old_keys.difference(&new_keys).cloned().collect();
        gone.extend(
            self.origin
                .keys()
                .filter(|k| !new_keys.contains(*k))
                .cloned(),
        );
        gone.sort();
        gone.dedup();
        let changed = !gone.is_empty();
        for k in &gone {
            self.gone(k);
        }
        let mut born = false;
        for path in new.iter().filter(|p| is_note(p)) {
            if !old_keys.contains(&key(path)) {
                self.appear(path);
                born = true;
            }
        }
        self.touched |= changed || born;
        changed || born
    }

    /// Заметка появилась по пути `path`.
    fn appear(&mut self, path: &VaultPath) {
        self.origin.entry(key(path)).or_insert(Origin::Born);
    }

    /// Заметка по ключу пути `k` (сейчас) ушла.
    fn gone(&mut self, k: &Key) {
        match self.origin.remove(k) {
            Some(Origin::Born) => {}
            Some(Origin::From(before)) => {
                self.left.insert(key(&before));
            }
            None => {
                self.left.insert(k.clone());
            }
        }
    }

    fn rename(
        &mut self,
        from: &VaultPath,
        to: &VaultPath,
        notes_under: &dyn Fn(&VaultPath) -> Vec<VaultPath>,
    ) -> bool {
        let under = notes_under(from);
        let pairs: Vec<(VaultPath, VaultPath)> = if under.is_empty() {
            // Команда уже переименовала — перенесённые лежат под `to`.
            notes_under(to)
                .into_iter()
                .map(|now| (moved(&now, to, from), now))
                .collect()
        } else {
            under
                .into_iter()
                .map(|old| {
                    let new = moved(&old, from, to);
                    (old, new)
                })
                .collect()
        };
        if pairs.is_empty() {
            // `а.txt` → `а.md`: заметка появилась.
            if is_note(to) {
                self.appear(to);
                return true;
            }
            return false;
        }
        let entries: Vec<(VaultPath, Origin)> = pairs
            .into_iter()
            .map(|(old, new)| {
                let origin = self.origin.remove(&key(&old)).unwrap_or(Origin::From(old));
                (new, origin)
            })
            .collect();
        for (new, origin) in entries {
            if !is_note(&new) {
                // `а.md` → `а.txt`: заметка ушла (решение 90).
                if let Origin::From(before) = origin {
                    self.left.insert(key(&before));
                }
                continue;
            }
            let k = key(&new);
            match &origin {
                Origin::From(before) if key(before) == k => {}
                _ => {
                    self.origin.insert(k, origin);
                }
            }
        }
        true
    }
}

/// Пачка `newer` поверх пачки `older` — как одна пачка из изменений обеих
/// по порядку (неотвеченный вопрос присоединяется к следующей пачке).
pub fn compose(older: Batch, newer: Batch) -> Batch {
    let mut origin: HashMap<Key, Origin> = HashMap::new();
    let mut left: HashSet<Key> = older.left;
    let mut consumed: HashSet<Key> = HashSet::new();
    for (k, o) in &newer.origin {
        let first = match o {
            Origin::Born => Origin::Born,
            Origin::From(p) => {
                let pk = key(p);
                let first = older
                    .origin
                    .get(&pk)
                    .cloned()
                    .unwrap_or_else(|| Origin::From(p.clone()));
                consumed.insert(pk);
                first
            }
        };
        match &first {
            Origin::From(q) if key(q) == *k => {}
            _ => {
                origin.insert(k.clone(), first);
            }
        }
    }
    let arrived: HashSet<Key> = older.origin.keys().cloned().collect();
    for (k, o) in older.origin {
        if consumed.contains(&k) {
            continue;
        }
        if newer.left.contains(&k) {
            if let Origin::From(q) = o {
                left.insert(key(&q));
            }
            continue;
        }
        origin.entry(k).or_insert(o);
    }
    for k in newer.left {
        if !arrived.contains(&k) {
            left.insert(k);
        }
    }
    Batch {
        origin,
        left,
        touched: older.touched || newer.touched,
    }
}

/// План пачки, ещё без индекса.
#[derive(Debug)]
pub struct Prepared {
    /// Список заметок сейчас (собственная копия: план считается без замка
    /// списка).
    after: Links,
    /// Заметки списка «сейчас», которых не было по этому пути до пачки.
    removed: Vec<VaultPath>,
    /// Пришедшие заметки — на прежних путях, со своей датой создания.
    added: Vec<NoteRef>,
    /// Путь до пачки → путь сейчас (пришедшие заметки).
    forward: HashMap<VaultPath, VaultPath>,
    /// Ключи имён появившихся и пришедших заметок: только ссылки с таким
    /// именем цели могут стать неоднозначными или перескочить.
    names: HashSet<NameKey>,
}

/// План пачки `batch` по списку заметок сейчас `now` (без индекса).
pub fn prepare(batch: &Batch, now: Vec<NoteRef>) -> Prepared {
    let mut removed = Vec::new();
    let mut moved_in: Vec<(NoteRef, VaultPath)> = Vec::new();
    let mut names: HashSet<NameKey> = HashSet::new();
    let mut staying: HashSet<Key> = HashSet::new();
    for note in &now {
        match batch.origin.get(&key(&note.path)) {
            None => {
                staying.insert(key(&note.path));
            }
            Some(origin) => {
                removed.push(note.path.clone());
                names.extend(name_key(&note.path));
                if let Origin::From(before) = origin {
                    moved_in.push((
                        NoteRef {
                            path: before.clone(),
                            created_ms: note.created_ms,
                        },
                        note.path.clone(),
                    ));
                }
            }
        }
    }
    let mut added = Vec::new();
    let mut forward = HashMap::new();
    let mut taken: HashSet<Key> = HashSet::new();
    for (before, current) in moved_in {
        let k = key(&before.path);
        // Прежний путь занят оставшейся заметкой или другой пришедшей —
        // список «до» без повтора пути: такая заметка считается появившейся.
        if staying.contains(&k) || !taken.insert(k) {
            continue;
        }
        forward.insert(before.path.clone(), current);
        added.push(before);
    }
    Prepared {
        after: Links::new(now),
        removed,
        added,
        forward,
        names,
    }
}

/// Индекс для плана — только записи заметок, где есть ссылки на имена
/// пачки (их и только их читает план). Остальной индекс не копируется.
pub fn snapshot(index: &VaultIndex, prepared: &Prepared) -> VaultIndex {
    let mut sources: HashSet<&VaultPath> = HashSet::new();
    for name in &prepared.names {
        sources.extend(index.links_named(name).map(|(from, _)| from));
    }
    VaultIndex::from_entries(
        sources
            .into_iter()
            .filter_map(|path| Some((path.clone(), index.entry(path)?.clone()))),
    )
}

/// План уточнения и ссылки, которые уточнить нельзя.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// Что переписать.
    pub plan: RelinkPlan,
    /// Заметки со ссылками, которые стали неоднозначными, но уточнить их
    /// нельзя (сбой `conflict`, не тишина).
    pub unfixable: Vec<VaultPath>,
}

/// Решение по одной цели.
#[derive(Debug, Clone)]
enum Decision {
    Keep,
    Rewrite(VaultPath, String),
    Unfixable,
}

impl Prepared {
    /// Ничего не появилось и не пришло — плана нет.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// План по индексу `index` ([`snapshot`]; пути — как сейчас).
    pub fn finish(&self, index: &VaultIndex) -> Found {
        if self.is_empty() {
            return Found::default();
        }
        let before = self.after.with_changes(&self.removed, self.added.clone());
        let after = self.after.view();
        let mut decided: HashMap<&str, Decision> = HashMap::new();
        let mut by_note: HashMap<VaultPath, Vec<(usize, PlannedLink)>> = HashMap::new();
        let mut unfixable: HashSet<VaultPath> = HashSet::new();
        for name in &self.names {
            for (from, link) in index.links_named(name) {
                if !after.contains(from) {
                    continue;
                }
                let decision = decided
                    .entry(link.target.as_str())
                    .or_insert_with(|| self.decide(&before, &after, &link.target));
                match decision {
                    Decision::Keep => {}
                    Decision::Unfixable => {
                        unfixable.insert(from.clone());
                    }
                    Decision::Rewrite(points_to, new_target) => {
                        by_note.entry(from.clone()).or_default().push((
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
        let mut unfixable: Vec<VaultPath> = unfixable.into_iter().collect();
        unfixable.sort_by(|a, b| a.components().cmp(b.components()));
        Found {
            plan: RelinkPlan {
                notes,
                unmatched: Vec::new(),
            },
            unfixable,
        }
    }

    fn decide(&self, before: &NotesView<'_>, after: &NotesView<'_>, target: &str) -> Decision {
        let was = before.candidates(target);
        let Some(first) = was.first() else {
            return Decision::Keep;
        };
        let now = self.forward.get(first).unwrap_or(first);
        if !after.contains(now) {
            return Decision::Keep;
        }
        let hits = after.candidates(target);
        // Ссылка больше не подходит к своей заметке (её переименовали) —
        // это обновление после переименования, не уточнение.
        if !hits.contains(now) {
            return Decision::Keep;
        }
        let jumps = hits.first() != Some(now);
        let ambiguous = hits.len() > 1 && was.len() == 1;
        if !(jumps || ambiguous) {
            return Decision::Keep;
        }
        let Ok(new_target) = after.shortest_unique_path(now) else {
            return Decision::Unfixable;
        };
        let linkable = new_target
            .strip_prefix('/')
            .unwrap_or(&new_target)
            .split('/')
            .all(names::is_linkable);
        let leads_back = after.candidates(&new_target).as_slice() == std::slice::from_ref(now);
        if linkable && leads_back {
            Decision::Rewrite(now.clone(), new_target)
        } else {
            Decision::Unfixable
        }
    }
}

/// Ключ имени заметки без `.md`; не заметка — `None`.
fn name_key(path: &VaultPath) -> Option<NameKey> {
    crate::switcher::link_path(path)?
        .last()
        .map(|name| names::key(name))
}

#[cfg(test)]
#[path = "duplicates_tests.rs"]
mod tests;
