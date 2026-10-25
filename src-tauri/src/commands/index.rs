//! Список заметок хранилища — общий для разрешения ссылок (`links`) и
//! быстрого перехода (`switcher`) (ревью 0.4: «общий индекс заметок»).
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
//! `removed` неизвестного — пропускается.
//!
//! Ключ заметки — ключи имён компонентов пути (`names::key`): событие
//! наблюдателя приходит в написании с диска, команда — в написании
//! интерфейса.

use std::collections::BTreeMap;

use super::events::TreeChange;
use super::types::{EntryKind, VaultPath};
use crate::links::Links;
use crate::switcher::Switcher;
use crate::vault::NoteRef;

/// Путь по ключам имён компонентов.
type PathKey = Vec<String>;

fn path_key(path: &VaultPath) -> PathKey {
    path.components()
        .iter()
        .map(|c| crate::names::key(c).as_str().to_owned())
        .collect()
}

/// Заметки хранилища и построенные по ним ссылки и быстрый переход.
#[derive(Debug, Default)]
pub struct NoteIndex {
    /// Ключ пути → путь (как на диске или как прислала команда) и дата
    /// создания.
    notes: BTreeMap<PathKey, (VaultPath, i64)>,
    links: Links,
    switcher: Switcher,
}

impl NoteIndex {
    /// Индекс по полному списку заметок (`Vault::notes`).
    pub fn new(notes: Vec<NoteRef>) -> Self {
        let mut index = Self::default();
        index.replace(notes);
        index
    }

    /// Заменяет список целиком (открытие, полная сверка).
    pub fn replace(&mut self, notes: Vec<NoteRef>) {
        self.notes = notes
            .into_iter()
            .map(|note| (path_key(&note.path), (note.path, note.created_ms)))
            .collect();
        self.rebuild();
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

    /// Заметки по пути `path` и под ним (путь заметки или папки), в
    /// написании индекса.
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

    /// [`NoteIndex::apply`], но перед каждым изменением `before` видит
    /// список заметок до него (так выводятся события `note:*`). Ссылки и
    /// быстрый переход перестраиваются один раз в конце.
    pub fn apply_observed(
        &mut self,
        changes: &[TreeChange],
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
        before: &mut dyn FnMut(&NoteIndex, &TreeChange),
    ) -> bool {
        let mut changed = false;
        for change in changes {
            before(self, change);
            changed |= self.apply_one(change, created_ms);
        }
        if changed {
            self.rebuild();
        }
        changed
    }

    fn apply_one(
        &mut self,
        change: &TreeChange,
        created_ms: &dyn Fn(&VaultPath) -> Option<i64>,
    ) -> bool {
        match change {
            TreeChange::Created { path, entry_kind } => {
                *entry_kind == EntryKind::Note && self.insert(path, created_ms)
            }
            TreeChange::Modified { .. } => false,
            TreeChange::Removed { path } => self.remove_under(path).is_some(),
            TreeChange::Renamed { from, to } => match self.remove_under(from) {
                Some(moved) => {
                    let from_len = from.components().len();
                    for (old, created) in moved {
                        let new = VaultPath::from_components(
                            to.components()
                                .iter()
                                .chain(&old.components()[from_len..])
                                .cloned(),
                        );
                        self.notes.insert(path_key(&new), (new, created));
                    }
                    true
                }
                // `from` неизвестен (изменение уже применено командой или
                // пачка после сверки) — событие пропускается; заметка `to`,
                // которой нет в списке, добавляется.
                None => is_note_path(to) && self.insert(to, created_ms),
            },
        }
    }

    /// Добавляет заметку, если её нет. `true` — добавлена.
    fn insert(&mut self, path: &VaultPath, created_ms: &dyn Fn(&VaultPath) -> Option<i64>) -> bool {
        let key = path_key(path);
        if self.notes.contains_key(&key) {
            return false;
        }
        let Some(created) = created_ms(path) else {
            return false;
        };
        self.notes.insert(key, (path.clone(), created));
        true
    }

    /// Убирает заметки по пути и под ним; `None` — таких не было.
    fn remove_under(&mut self, path: &VaultPath) -> Option<Vec<(VaultPath, i64)>> {
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
            keys.iter()
                .filter_map(|key| self.notes.remove(key))
                .collect(),
        )
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

    fn paths(index: &NoteIndex) -> Vec<VaultPath> {
        index.notes.values().map(|(p, _)| p.clone()).collect()
    }

    const NOW: &dyn Fn(&VaultPath) -> Option<i64> = &|_| Some(7);

    #[test]
    fn applying_the_same_change_twice_is_harmless() {
        let mut index = NoteIndex::new(vec![note("А/a.md", 1)]);
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
        let mut index = NoteIndex::new(vec![note("Проекты/Встреча.md", 1)]);
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
        let mut index = NoteIndex::default();
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
        let mut index = NoteIndex::new(vec![
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
        let mut index = NoteIndex::default();
        let gone: &dyn Fn(&VaultPath) -> Option<i64> = &|_| None;
        assert!(!index.apply(
            &[TreeChange::Created {
                path: vp("a.md"),
                entry_kind: EntryKind::Note,
            }],
            gone
        ));
    }
}
