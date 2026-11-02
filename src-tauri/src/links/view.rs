//! Список заметок «до» и «после» изменения без перестройки общего списка
//! (выпуск 0.2, задача 0.2-4.1; С2-5.1, С2-3.5, С2-3.6, Р2, Р3).
//!
//! [`NotesView`] — наложение поверх [`Links`]: общий список (только чтение)
//! плюс «убраны» (номера заметок общего списка) и «добавлены» (свои записи с
//! ключами). Разрешение, все кандидаты и кратчайший различающий путь
//! считаются по группе заметок одного ключа имени: группа общего списка без
//! убранных и добавленные с тем же ключом. Поэтому вид «после» стоит
//! O(k) памяти и времени на k перемещённых заметок (ключи имён считаются
//! только для них), а не O(n) на всё хранилище; переименование папки ещё
//! один раз проходит общий список, сравнивая начала путей. Вид без изменений
//! (`Links::view`) ничего не выделяет.
//!
//! Порядок при равной дате создания — порядок путей ядра (`KeyedPath`), при
//! равных путях — общий список раньше добавленных, внутри — по номеру. Это
//! тот же порядок, что у списка, собранного заново из «оставшиеся, затем
//! добавленные» (`natural_ranks` сортирует устойчиво), — проверяет
//! `proptest` `resolve_same_as_full_rebuild`.

use std::collections::{HashMap, HashSet};

use super::{Links, Note, link_text, parse_target};
use crate::commands::{CoreError, CoreResult, ErrorCode, PathRename, VaultPath};
use crate::names::{self, NameKey};
use crate::vault::NoteRef;

/// Заметка вида: из общего списка или добавленная.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Id {
    /// Номер в общем списке.
    Base(usize),
    /// Номер среди добавленных.
    Added(usize),
}

/// Список заметок, на который смотрят разрешение и кратчайший путь: общий
/// список [`Links`] с изменениями поверх. Общий список не меняется и не
/// копируется.
#[derive(Debug)]
pub struct NotesView<'a> {
    base: &'a Links,
    removed: HashSet<usize>,
    added: Vec<Note>,
    added_by_name: HashMap<NameKey, Vec<usize>>,
    moves: Vec<(VaultPath, VaultPath)>,
}

impl Links {
    /// Вид на текущий список заметок без изменений — состояние «до».
    pub fn view(&self) -> NotesView<'_> {
        NotesView {
            base: self,
            removed: HashSet::new(),
            added: Vec::new(),
            added_by_name: HashMap::new(),
            moves: Vec::new(),
        }
    }

    /// Все заметки, к которым подходит цель `target` (как в
    /// [`Links::resolve`]): самая старая первой, при равной дате — по
    /// порядку путей ядра. Больше одной — ссылка неоднозначна (С2-3.6).
    pub fn candidates(&self, target: &str) -> Vec<VaultPath> {
        self.view().candidates(target)
    }

    /// Вид «после»: без заметок `removed` (пути, которых нет в списке,
    /// пропускаются) и с заметками `added` (не `.md` пропускаются, как в
    /// [`Links::replace_notes`]). Пути `added` не должны совпадать с
    /// оставшимися — список с повтором пути на диске не бывает.
    pub fn with_changes(&self, removed: &[VaultPath], added: Vec<NoteRef>) -> NotesView<'_> {
        let mut view = self.view();
        for path in removed {
            if let Some(Id::Base(i)) = view.find(path) {
                view.removed.insert(i);
            }
        }
        for note in added {
            view.add(note);
        }
        view
    }

    /// Вид «после» пачки переименований заметок и папок. Все пути «откуда» —
    /// в списке «до» (пачка выполняется разом: `А → Б`, `Б → А` — обмен);
    /// заметку, которую задевают несколько переименований, переносит первое.
    /// `from` — заметка списка, иначе папка: переносятся все заметки внутри.
    /// Заметка переезжает со своей датой создания. Переименование того,
    /// чего нет в списке, ничего не меняет.
    pub fn after_renames(&self, renames: &[PathRename]) -> NotesView<'_> {
        self.after_replacing(&[], renames)
    }

    /// Как [`Links::after_renames`], но заметки `replaced` (пути списка
    /// «до») затёрты переименованиями пачки — перенос с заменой: их нет в
    /// списке «после», и переименования их не переносят.
    pub fn after_replacing(&self, replaced: &[VaultPath], renames: &[PathRename]) -> NotesView<'_> {
        let gone: HashSet<usize> = replaced
            .iter()
            .filter_map(|path| match self.view().find(path) {
                Some(Id::Base(i)) => Some(i),
                _ => None,
            })
            .collect();
        let mut targets: HashMap<usize, VaultPath> = HashMap::new();
        for &i in &gone {
            targets.insert(i, VaultPath::root());
        }
        for rename in renames {
            let from = rename.from.components();
            if from.is_empty() {
                continue;
            }
            if let Some(Id::Base(i)) = self.view().find(&rename.from) {
                targets.entry(i).or_insert_with(|| rename.to.clone());
                continue;
            }
            for (i, entry) in self.entries.iter().enumerate() {
                let path = entry.note.path.components();
                if path.len() > from.len() && path.starts_with(from) {
                    targets.entry(i).or_insert_with(|| {
                        VaultPath::from_components(
                            rename.to.components().iter().chain(&path[from.len()..]),
                        )
                    });
                }
            }
        }
        let mut moved: Vec<(usize, VaultPath)> = targets
            .into_iter()
            .filter(|(i, _)| !gone.contains(i))
            .collect();
        moved.sort_by_key(|(i, _)| *i);
        let mut view = self.view();
        view.removed.extend(gone);
        for (i, to) in moved {
            let note = &self.entries[i].note;
            view.removed.insert(i);
            view.add(NoteRef {
                path: to.clone(),
                created_ms: note.created_ms,
            });
            view.moves.push((note.path.clone(), to));
        }
        view
    }
}

impl Links {
    /// Есть ли в списке заметка или папка (папка — есть заметка внутри) с
    /// ровно этим путём. Корень — нет.
    pub fn has_path(&self, path: &VaultPath) -> bool {
        let want = path.components();
        !want.is_empty()
            && (self.view().find(path).is_some()
                || self.entries.iter().any(|entry| {
                    let have = entry.note.path.components();
                    have.len() > want.len() && have.starts_with(want)
                }))
    }

    /// Занят ли путь `path` (заметка или папка) в списке — по ключам
    /// компонентов, как в Windows (`Бета/встреча.md` занят заметкой
    /// `Бета/Встреча.md`), не считая заметок в `freed` и внутри них (точные
    /// пути списка: то, что уходит или затирается той же пачкой).
    pub fn occupied(&self, path: &VaultPath, freed: &[VaultPath]) -> bool {
        let want = path.components();
        let folder: Vec<NameKey> = want.iter().map(|c| names::key(c)).collect();
        let note: Option<Vec<NameKey>> = crate::switcher::link_path(path)
            .map(|parts| parts.iter().map(|c| names::key(c)).collect());
        self.entries.iter().any(|entry| {
            let have = entry.note.path.components();
            let keys = entry.note.keys();
            let hit = (have.len() == want.len() && note.as_deref() == Some(keys))
                || (have.len() > want.len() && keys[..want.len()] == folder[..]);
            hit && !freed.iter().any(|f| {
                let f = f.components();
                have == f || (have.len() > f.len() && have.starts_with(f))
            })
        })
    }

    /// Путь `path` в написании списка: точное совпадение (заметка или
    /// папка), иначе единственная заметка или папка с теми же ключами
    /// компонентов. Несколько написаний с одним ключом (Linux: `А.md` и
    /// `а.md` рядом) или ничего — `None`. Пути из событий ОС могут
    /// отличаться от списка регистром или формой Unicode (NFC/NFD), а
    /// [`Links::after_renames`] сравнивает точно.
    pub fn spelled(&self, path: &VaultPath) -> Option<VaultPath> {
        if self.has_path(path) {
            return Some(path.clone());
        }
        let want = path.components();
        if want.is_empty() {
            return None;
        }
        let folder: Vec<NameKey> = want.iter().map(|c| names::key(c)).collect();
        let note: Option<Vec<NameKey>> = crate::switcher::link_path(path)
            .map(|parts| parts.iter().map(|c| names::key(c)).collect());
        let mut found: Option<VaultPath> = None;
        for entry in &self.entries {
            let have = entry.note.path.components();
            let keys = entry.note.keys();
            let hit = if have.len() == want.len() && note.as_deref() == Some(keys) {
                entry.note.path.clone()
            } else if have.len() > want.len() && keys[..want.len()] == folder[..] {
                VaultPath::from_components(have[..want.len()].iter().cloned())
            } else {
                continue;
            };
            match &found {
                Some(seen) if *seen != hit => return None,
                _ => found = Some(hit),
            }
        }
        found
    }
}

impl NotesView<'_> {
    fn add(&mut self, note: NoteRef) {
        let Some(note) = Note::new(note) else { return };
        if let Some(name) = note.keys().last() {
            self.added_by_name
                .entry(name.clone())
                .or_default()
                .push(self.added.len());
        }
        self.added.push(note);
    }

    fn note(&self, id: Id) -> &Note {
        match id {
            Id::Base(i) => &self.base.entries[i].note,
            Id::Added(i) => &self.added[i],
        }
    }

    /// Заметки вида с ключом имени `name`.
    fn group<'v>(&'v self, name: &NameKey) -> impl Iterator<Item = Id> + 'v {
        let base = self.base.by_name.get(name).into_iter().flatten();
        let added = self.added_by_name.get(name).into_iter().flatten();
        base.filter(|&&i| !self.removed.contains(&i))
            .map(|&i| Id::Base(i))
            .chain(added.map(|&i| Id::Added(i)))
    }

    /// Заметка вида с ровно этим путём.
    fn find(&self, path: &VaultPath) -> Option<Id> {
        let link_path = crate::switcher::link_path(path)?;
        let name = names::key(link_path.last()?);
        self.group(&name).find(|&id| &self.note(id).path == path)
    }

    /// Порядок «самая старая первой»: дата создания, путь, номер.
    fn order(&self, a: Id, b: Id) -> std::cmp::Ordering {
        let (x, y) = (self.note(a), self.note(b));
        x.created_ms
            .cmp(&y.created_ms)
            .then_with(|| x.keyed.cmp(&y.keyed))
            .then(a.cmp(&b))
    }

    fn matches(&self, target: &str) -> Vec<Id> {
        let Ok(target) = parse_target(target) else {
            return Vec::new();
        };
        let want: Vec<NameKey> = target
            .folders
            .iter()
            .chain(std::iter::once(&target.name))
            .map(|c| names::key(c))
            .collect();
        let Some(name) = want.last() else {
            return Vec::new();
        };
        let mut hits: Vec<Id> = self
            .group(name)
            .filter(|&id| {
                let keys = self.note(id).keys();
                if target.from_root {
                    keys == want.as_slice()
                } else {
                    keys.ends_with(&want)
                }
            })
            .collect();
        hits.sort_by(|&a, &b| self.order(a, b));
        hits
    }

    /// Куда ведёт цель `target` по этому списку — правила
    /// [`Links::resolve`]; `None` — неразрешённая.
    pub fn resolve(&self, target: &str) -> Option<VaultPath> {
        let hits = self.matches(target);
        hits.first().map(|&id| self.note(id).path.clone())
    }

    /// Все заметки, к которым подходит цель, — как [`Links::candidates`].
    pub fn candidates(&self, target: &str) -> Vec<VaultPath> {
        self.matches(target)
            .into_iter()
            .map(|id| self.note(id).path.clone())
            .collect()
    }

    /// Кратчайший различающий путь заметки по этому списку — правила
    /// [`Links::shortest_unique_path`]; заметки нет — `not_found`.
    pub fn shortest_unique_path(&self, note: &VaultPath) -> CoreResult<String> {
        self.find(note)
            .map(|id| self.shortest_of(id))
            .ok_or_else(|| CoreError::new(ErrorCode::NotFound))
    }

    /// Есть ли заметка с ровно этим путём.
    pub fn contains(&self, note: &VaultPath) -> bool {
        self.find(note).is_some()
    }

    /// Перемещения заметок вида (`after_renames`): «путь до → путь после»
    /// в порядке списка «до».
    pub fn moves(&self) -> &[(VaultPath, VaultPath)] {
        &self.moves
    }

    /// Новый путь заметки, перемещённой `after_renames`.
    pub fn moved_to(&self, before: &VaultPath) -> Option<&VaultPath> {
        self.moves
            .iter()
            .find(|(from, _)| from == before)
            .map(|(_, to)| to)
    }

    pub(super) fn shortest_of(&self, id: Id) -> String {
        let me = self.note(id);
        let keys = me.keys();
        let others: Vec<&[NameKey]> = keys
            .last()
            .into_iter()
            .flat_map(|name| self.group(name))
            .filter(|&other| other != id)
            .map(|other| self.note(other).keys())
            .collect();
        let n = keys.len();
        if others.is_empty() {
            link_text(&me.link_path[n - 1..])
        } else if others.iter().any(|o| o.ends_with(keys)) {
            // Мой путь — конец пути другой заметки: только путь от корня.
            format!("/{}", link_text(&me.link_path))
        } else if others.iter().any(|o| keys.ends_with(o)) {
            // Путь другой заметки — конец моего: полный путь (С3.4).
            link_text(&me.link_path)
        } else {
            let k = (2..=n)
                .find(|&k| !others.iter().any(|o| o.ends_with(&keys[n - k..])))
                .unwrap_or(n);
            link_text(&me.link_path[n - k..])
        }
    }
}
