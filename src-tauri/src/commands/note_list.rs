//! Список заметок хранилища — общий для разрешения ссылок (`links`) и
//! быстрого перехода (`switcher`) (ревью 0.4: «общий индекс заметок»; с 0.2 — «список заметок», чтобы не путать с индексом хранилища `crate::index`, план 0.2, Т3).
//!
//! Обновляется двумя путями, оба идемпотентны:
//! - сразу после своей команды окна (создание, переименование, перемещение,
//!   удаление, копия конфликта) — ссылки видят изменение без ожидания
//!   наблюдателя;
//! - по пачке наблюдателя (`vault:changed`), в том числе после полной сверки.
//!
//! Повтор того же изменения ничего не ломает: `created` уже известной
//! заметки — без изменений, `renamed` с неизвестным `from` — пропускается
//! (кроме случая, когда `to` — заметка, которой ещё нет: она добавляется),
//! `removed` неизвестного — пропускается. Переименование заметки в
//! не-заметку (`a.md` → `a.txt`) убирает её из списка (решение 90).
//!
//! Ключ заметки — ключи имён компонентов пути (`names::key`): событие
//! наблюдателя приходит в написании с диска, команда — в написании
//! интерфейса.
//!
//! Ссылки и быстрый переход после пачки обновляются по изменениям
//! (задача 0.2-7.3, С2-5.2): в них уходят только убранные и добавленные
//! заметки с их местами в списке, ключи остальных не пересчитываются.
//! Порядок в ссылках и быстром переходе — порядок этого списка, как при
//! полной перестройке; совпадение с ней проверяет `proptest`
//! `incremental_equals_rebuild`.

use std::collections::BTreeMap;

use super::events::TreeChange;
use super::types::{EntryKind, VaultPath};
use crate::links::Links;
use crate::switcher::Switcher;
use crate::vault::NoteRef;

/// Путь по ключам имён компонентов.
type PathKey = Vec<String>;

/// Заметка списка: путь и дата создания.
type Note = (VaultPath, i64);

/// Ключи, которые задела пачка, и их заметки до пачки (`None` — не было).
type Touched = BTreeMap<PathKey, Option<Note>>;

fn path_key(path: &VaultPath) -> PathKey {
    path.components()
        .iter()
        .map(|c| crate::names::key(c).as_str().to_owned())
        .collect()
}

/// Заметки хранилища и построенные по ним ссылки и быстрый переход.
#[derive(Debug, Default)]
pub struct NoteList {
    /// Ключ пути → путь (как на диске или как прислала команда) и дата
    /// создания.
    notes: BTreeMap<PathKey, Note>,
    links: Links,
    switcher: Switcher,
}

impl NoteList {
    /// Список по полному списку заметок (`Vault::notes`).
    pub fn new(notes: Vec<NoteRef>) -> Self {
        let mut list = Self::default();
        list.replace(notes);
        list
    }

    /// Заменяет список целиком (открытие, полная сверка). Не заметки (не
    /// `.md`) пропускаются, как в ссылках и быстром переходе.
    pub fn replace(&mut self, notes: Vec<NoteRef>) {
        self.notes = notes
            .into_iter()
            .filter(|note| is_note_path(&note.path))
            .map(|note| (path_key(&note.path), (note.path, note.created_ms)))
            .collect();
        self.rebuild();
    }

    /// Заметки списка с датой создания — копия для отдельного списка
    /// ссылок (выполнение плана переписывания без замка, 0.2-4.6).
    pub fn note_refs(&self) -> Vec<NoteRef> {
        self.notes
            .values()
            .map(|(path, created_ms)| NoteRef {
                path: path.clone(),
                created_ms: *created_ms,
            })
            .collect()
    }

    /// Разрешение ссылок.
    pub fn links(&self) -> &Links {
        &self.links
    }

    /// Быстрый переход.
    pub fn switcher(&self) -> &Switcher {
        &self.switcher
    }

    /// Есть ли заметка с таким путём (по ключам имён).
    pub fn contains(&self, path: &VaultPath) -> bool {
        self.notes.contains_key(&path_key(path))
    }

    /// Путь заметки в написании списка (как на диске или как прислала
    /// команда), найденный по ключам имён; `None` — такой заметки нет.
    pub fn note(&self, path: &VaultPath) -> Option<&VaultPath> {
        self.notes.get(&path_key(path)).map(|(path, _)| path)
    }

    /// Заметки по пути `path` и под ним (путь заметки или папки), в
    /// написании списка.
    pub fn notes_under(&self, path: &VaultPath) -> Vec<VaultPath> {
        let prefix = path_key(path);
        self.notes
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .map(|(_, (path, _))| path.clone())
            .collect()
    }

    /// Применяет изменения дерева по порядку. `created_ms` — дата создания
    /// новой заметки с диска (`None` — файла уже нет, заметка не
    /// добавляется). Возвращает, изменился ли список.
    pub fn apply(
        &mut self,
        changes: &[TreeChange],
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
    ) -> bool {
        self.apply_observed(changes, created_ms, &mut |_, _| {})
    }

    /// [`NoteList::apply`], но перед каждым изменением `before` видит
    /// список заметок до него (так выводятся события `note:*`). Ссылки и
    /// быстрый переход обновляются один раз в конце — по разнице между
    /// списком до пачки и после.
    pub fn apply_observed(
        &mut self,
        changes: &[TreeChange],
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
        before: &mut dyn FnMut(&NoteList, &TreeChange),
    ) -> bool {
        let mut changed = false;
        let mut touched = Touched::new();
        for change in changes {
            before(self, change);
            changed |= self.apply_one(change, created_ms, &mut touched);
        }
        if changed {
            self.update(touched);
        }
        changed
    }

    fn apply_one(
        &mut self,
        change: &TreeChange,
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
        touched: &mut Touched,
    ) -> bool {
        match change {
            TreeChange::Created { path, entry_kind } => {
                *entry_kind == EntryKind::Note
                    && is_note_path(path)
                    && self.insert(path, created_ms, touched)
            }
            TreeChange::Modified { .. } => false,
            TreeChange::Removed { path } => self.remove_under(path, touched).is_some(),
            TreeChange::Renamed { from, to } => match self.remove_under(from, touched) {
                Some(moved) => {
                    let from_len = from.components().len();
                    for (old, created) in moved {
                        let new = VaultPath::from_components(
                            to.components()
                                .iter()
                                .chain(&old.components()[from_len..])
                                .cloned(),
                        );
                        // Переименование в не-заметку (`a.md` → `a.txt`) —
                        // уход из заметок, как удаление (решение 90).
                        if is_note_path(&new) {
                            let key = path_key(&new);
                            let old = self.notes.insert(key.clone(), (new, created));
                            touched.entry(key).or_insert(old);
                        }
                    }
                    true
                }
                // `from` неизвестен (изменение уже применено командой или
                // пачка после сверки) — событие пропускается; заметка `to`,
                // которой нет в списке, добавляется.
                None => is_note_path(to) && self.insert(to, created_ms, touched),
            },
        }
    }

    /// Добавляет заметку, если её нет. `true` — добавлена.
    fn insert(
        &mut self,
        path: &VaultPath,
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
        touched: &mut Touched,
    ) -> bool {
        let key = path_key(path);
        if self.notes.contains_key(&key) {
            return false;
        }
        let Some(created) = created_ms(path) else {
            return false;
        };
        self.notes.insert(key.clone(), (path.clone(), created));
        touched.entry(key).or_insert(None);
        true
    }

    /// Убирает заметки по пути и под ним; `None` — таких не было.
    fn remove_under(&mut self, path: &VaultPath, touched: &mut Touched) -> Option<Vec<Note>> {
        let prefix = path_key(path);
        let keys: Vec<PathKey> = self
            .notes
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .map(|(key, _)| key.clone())
            .collect();
        if keys.is_empty() {
            return None;
        }
        Some(
            keys.into_iter()
                .filter_map(|key| {
                    let note = self.notes.remove(&key)?;
                    touched.entry(key).or_insert_with(|| Some(note.clone()));
                    Some(note)
                })
                .collect(),
        )
    }

    /// Переносит в ссылки и быстрый переход разницу пачки: заметки,
    /// которых после пачки нет или которые изменились (написание пути,
    /// дата создания), убираются с их мест в списке «до»; новые и
    /// изменившиеся добавляются на свои места в списке «после».
    fn update(&mut self, touched: Touched) {
        let mut removed: Vec<PathKey> = Vec::new();
        let mut added: Vec<PathKey> = Vec::new();
        for (key, old) in touched {
            let new = self.notes.get(&key);
            if old.as_ref() == new {
                continue;
            }
            if old.is_some() {
                removed.push(key.clone());
            }
            if new.is_some() {
                added.push(key);
            }
        }
        if removed.is_empty() && added.is_empty() {
            return;
        }
        // Один проход по списку «после» (ключи упорядочены): места
        // добавленных и число ключей «после», меньших каждого убранного.
        // Место убранного в списке «до» = меньших «после» − меньших
        // добавленных + меньших убранных.
        let mut added_at: Vec<usize> = Vec::with_capacity(added.len());
        let mut after_less: Vec<usize> = Vec::with_capacity(removed.len());
        let (mut a, mut r) = (0, 0);
        for (pos, key) in self.notes.keys().enumerate() {
            while r < removed.len() && removed[r] <= *key {
                after_less.push(pos);
                r += 1;
            }
            if a < added.len() && added[a] == *key {
                added_at.push(pos);
                a += 1;
            }
            if a == added.len() && r == removed.len() {
                break;
            }
        }
        after_less.resize(removed.len(), self.notes.len());
        let mut added_less = 0;
        let removed_at: Vec<usize> = removed
            .iter()
            .zip(after_less)
            .enumerate()
            .map(|(removed_less, (key, after_less))| {
                while added_less < added.len() && added[added_less] < *key {
                    added_less += 1;
                }
                after_less - added_less + removed_less
            })
            .collect();
        let added: Vec<(usize, NoteRef)> = added
            .iter()
            .zip(added_at)
            .filter_map(|(key, pos)| {
                let (path, created_ms) = self.notes.get(key)?;
                Some((
                    pos,
                    NoteRef {
                        path: path.clone(),
                        created_ms: *created_ms,
                    },
                ))
            })
            .collect();
        let paths: Vec<(usize, VaultPath)> = added
            .iter()
            .map(|(pos, note)| (*pos, note.path.clone()))
            .collect();
        self.links.splice(&removed_at, added);
        self.switcher.splice(&removed_at, paths, self.links.ranks());
    }

    fn rebuild(&mut self) {
        let notes: Vec<NoteRef> = self
            .notes
            .values()
            .map(|(path, created_ms)| NoteRef {
                path: path.clone(),
                created_ms: *created_ms,
            })
            .collect();
        self.switcher
            .replace_notes(notes.iter().map(|n| n.path.clone()).collect());
        self.links.replace_notes(notes);
    }
}

fn is_note_path(path: &VaultPath) -> bool {
    path.file_name().is_some_and(crate::names::is_note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(rel: &str) -> VaultPath {
        VaultPath::from_components(rel.split('/'))
    }

    fn note(rel: &str, created_ms: i64) -> NoteRef {
        NoteRef {
            path: vp(rel),
            created_ms,
        }
    }

    fn paths(index: &NoteList) -> Vec<VaultPath> {
        index.notes.values().map(|(p, _)| p.clone()).collect()
    }

    const NOW: &dyn Fn(&VaultPath) -> Option<i64> = &|_| Some(7);

    #[test]
    fn note_renamed_to_a_non_note_leaves_the_list() {
        // Решение оркестратора 90 (найдено в 2.12): `.md` → `.txt` — не заметка.
        let mut index = NoteList::new(vec![note("А/a.md", 1), note("А/b.md", 2)]);
        assert!(index.apply(
            &[TreeChange::Renamed {
                from: vp("А/a.md"),
                to: vp("А/a.txt"),
            }],
            NOW,
        ));
        assert_eq!(paths(&index), vec![vp("А/b.md")]);
        // Папка с заметками переименовывается как раньше.
        index.apply(
            &[TreeChange::Renamed {
                from: vp("А"),
                to: vp("Б"),
            }],
            NOW,
        );
        assert_eq!(paths(&index), vec![vp("Б/b.md")]);
    }

    #[test]
    fn applying_the_same_change_twice_is_harmless() {
        let mut index = NoteList::new(vec![note("А/a.md", 1)]);
        let created = TreeChange::Created {
            path: vp("А/a.md"),
            entry_kind: EntryKind::Note,
        };
        assert!(
            !index.apply(std::slice::from_ref(&created), NOW),
            "уже есть"
        );
        let renamed = TreeChange::Renamed {
            from: vp("А"),
            to: vp("Б"),
        };
        assert!(index.apply(std::slice::from_ref(&renamed), NOW));
        assert!(
            !index.apply(std::slice::from_ref(&renamed), NOW),
            "from уже нет"
        );
        assert_eq!(paths(&index), vec![vp("Б/a.md")]);
        let removed = TreeChange::Removed { path: vp("Б") };
        assert!(index.apply(std::slice::from_ref(&removed), NOW));
        assert!(!index.apply(std::slice::from_ref(&removed), NOW));
        assert!(paths(&index).is_empty());
    }

    #[test]
    fn created_date_is_kept_on_rename_and_paths_match_by_name_key() {
        let mut index = NoteList::new(vec![note("Проекты/Встреча.md", 1)]);
        index.apply(
            &[TreeChange::Renamed {
                from: vp("проекты/встреча.md"),
                to: vp("Архив/Встреча.md"),
            }],
            NOW,
        );
        assert_eq!(
            index.notes.values().next(),
            Some(&(vp("Архив/Встреча.md"), 1))
        );
        assert!(index.contains(&vp("АРХИВ/встреча.md")));
    }

    #[test]
    fn rename_with_unknown_from_adds_the_new_note_once() {
        let mut index = NoteList::default();
        let renamed = TreeChange::Renamed {
            from: vp("нет.md"),
            to: vp("есть.md"),
        };
        assert!(index.apply(std::slice::from_ref(&renamed), NOW));
        assert!(!index.apply(std::slice::from_ref(&renamed), NOW));
        assert_eq!(paths(&index), vec![vp("есть.md")]);
    }

    #[test]
    fn notes_under_folder_and_links_follow_changes() {
        let mut index = NoteList::new(vec![
            note("А/1.md", 1),
            note("А/Б/2.md", 2),
            note("АБ.md", 3),
        ]);
        assert_eq!(
            index.notes_under(&vp("А")),
            vec![vp("А/1.md"), vp("А/Б/2.md")]
        );
        index.apply(
            &[TreeChange::Created {
                path: vp("Новая.md"),
                entry_kind: EntryKind::Note,
            }],
            NOW,
        );
        assert_eq!(
            index
                .links()
                .resolve(&vp("АБ.md"), "Новая")
                .expect("разрешение"),
            Some(vp("Новая.md"))
        );
        assert_eq!(
            index.switcher().search("нов").expect("поиск")[0].path,
            vp("Новая.md")
        );
    }

    #[test]
    fn note_that_vanished_before_indexing_is_not_added() {
        let mut index = NoteList::default();
        let gone: &dyn Fn(&VaultPath) -> Option<i64> = &|_| None;
        assert!(!index.apply(
            &[TreeChange::Created {
                path: vp("a.md"),
                entry_kind: EntryKind::Note,
            }],
            gone
        ));
    }

    mod props {
        use super::*;
        use crate::commands::PathRename;
        use proptest::prelude::*;

        /// Малый алфавит — чтобы чаще совпадали имена, ключи и концы путей.
        const PARTS: [&str; 6] = ["А", "а", "Б", "Встреча", "ß", "ss"];

        fn rel() -> impl Strategy<Value = String> {
            prop::collection::vec(prop::sample::select(PARTS.to_vec()), 1..4)
                .prop_map(|parts| parts.join("/"))
        }

        /// Заметка (в том числе `.MD`), папка или не заметка.
        fn target() -> impl Strategy<Value = VaultPath> {
            (
                rel(),
                prop::sample::select(vec![".md", ".md", ".MD", "", ".txt"]),
            )
                .prop_map(|(rel, ext)| vp(&format!("{rel}{ext}")))
        }

        fn change() -> impl Strategy<Value = TreeChange> {
            prop_oneof![
                3 => target().prop_map(|path| TreeChange::Created {
                    entry_kind: if is_note_path(&path) {
                        EntryKind::Note
                    } else {
                        EntryKind::Attachment
                    },
                    path,
                }),
                2 => target().prop_map(|path| TreeChange::Removed { path }),
                4 => (target(), target()).prop_map(|(from, to)| TreeChange::Renamed { from, to }),
                1 => target().prop_map(|path| TreeChange::Modified { path }),
            ]
        }

        /// Дата создания по пути; у части путей файла «уже нет».
        fn created(path: &VaultPath) -> Option<i64> {
            let n: usize = path.components().iter().map(|c| c.chars().count()).sum();
            (!n.is_multiple_of(7)).then_some((n % 3) as i64)
        }

        /// Все наблюдаемые запросы списка, ссылок и быстрого перехода.
        fn observe(list: &NoteList, probes: &[VaultPath]) -> Vec<String> {
            let mut out = vec![format!("{:?}", paths(list))];
            let links = list.links();
            let from = vp("Откуда.md");
            for query in ["", "а", "встр", "б/в", "ss", "а а"] {
                out.push(format!(
                    "suggest {query:?}: {:?}",
                    links.suggest(&from, query)
                ));
                out.push(format!(
                    "search {query:?}: {:?}",
                    list.switcher().search(query)
                ));
            }
            let freed: Vec<VaultPath> = paths(list).into_iter().take(1).collect();
            for probe in probes {
                let text = probe.components().join("/");
                for target in [text.clone(), format!("/{text}")] {
                    out.push(format!(
                        "{target}: {:?} {:?}",
                        links.resolve(&from, &target),
                        links.candidates(&target)
                    ));
                }
                out.push(format!(
                    "{text}: {} {:?} {:?} {:?} {} {} {:?} {:?}",
                    list.contains(probe),
                    list.note(probe),
                    list.notes_under(probe),
                    links.shortest_unique_path(probe),
                    links.has_path(probe),
                    links.occupied(probe, &freed),
                    links.notes_at(probe),
                    links.spelled(probe),
                ));
                let after = links.after_renames(&[PathRename {
                    from: probe.clone(),
                    to: vp("Новое/Встреча.md"),
                }]);
                out.push(format!(
                    "{text} → Новое: {:?} {:?} {:?}",
                    after.moves(),
                    after.candidates("Встреча"),
                    after.shortest_unique_path(&vp("Новое/Встреча.md"))
                ));
            }
            out
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(256))]

            /// Задача 0.2-7.3: после пачек изменений (созданы, удалены,
            /// переименованы и перенесены заметки и папки) список, ссылки и
            /// быстрый переход отвечают так же, как собранные с нуля по тем
            /// же заметкам. Заметки, которой уже нет, не видно нигде.
            #[test]
            fn incremental_equals_rebuild(
                start in prop::collection::vec((target(), 0i64..3), 0..10),
                batches in prop::collection::vec(prop::collection::vec(change(), 0..10), 1..4),
            ) {
                let mut list = NoteList::new(
                    start.iter().map(|(path, at)| NoteRef { path: path.clone(), created_ms: *at }).collect(),
                );
                let mut probes: Vec<VaultPath> = start.iter().map(|(path, _)| path.clone()).collect();
                for batch in &batches {
                    for change in batch {
                        match change {
                            TreeChange::Created { path, .. }
                            | TreeChange::Removed { path }
                            | TreeChange::Modified { path } => probes.push(path.clone()),
                            TreeChange::Renamed { from, to } => {
                                probes.push(from.clone());
                                probes.push(to.clone());
                            }
                        }
                    }
                    list.apply(batch, &created);
                    let fresh = NoteList::new(
                        list.notes
                            .values()
                            .map(|(path, created_ms)| NoteRef { path: path.clone(), created_ms: *created_ms })
                            .collect(),
                    );
                    let (got, want) = (observe(&list, &probes), observe(&fresh, &probes));
                    for (got, want) in got.iter().zip(&want) {
                        prop_assert_eq!(got, want);
                    }
                    prop_assert_eq!(got.len(), want.len());
                }
            }
        }
    }
}
