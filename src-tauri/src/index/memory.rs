//! Индекс в памяти: записи заметок и производные карты (ссылки по ключу
//! имени цели, теги), которые пересчитываются только по изменённой заметке:
//! вклад старой записи вычитается, вклад новой — прибавляется. Итог не
//! зависит от порядка изменений — проверяет `proptest`
//! `incremental_equals_full_rebuild`.
//!
//! Состояние сборки ([`State`]) знает, какие заметки обновили или удалили
//! снаружи (своя запись, наблюдатель), пока шла сборка: прочитанное сборкой
//! их не перекрывает — внешнее изменение не старше прочитанного сборкой, а
//! если файл изменится ещё раз, наблюдатель пришлёт новое событие.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use super::{BacklinkSource, LinkSite, NoteEntry, TagCount};
use crate::commands::{IndexState, IndexStatus, VaultPath};
use crate::links::{Links, parse_target};
use crate::markdown::{WikiLink, tag_key};
use crate::names::{self, NameKey};

/// Номер заметки в индексе: производные карты хранят его вместо копий
/// пути (замер 0.2-7.1: копии путей — около половины памяти индекса).
/// Номер живёт, пока заметка в индексе; освободившийся номер достаётся
/// следующей новой заметке.
type NoteId = u32;

/// Набор номеров заметок — упорядоченный вектор без повторов: на порядок
/// меньше памяти, чем `HashSet`, а наборы малы (ссылки на одно имя, заметки
/// одного тега). Новые заметки получают растущие номера, поэтому вставка
/// почти всегда в конец.
type NoteSet = Vec<NoteId>;

fn set_insert(set: &mut NoteSet, id: NoteId) {
    if let Err(at) = set.binary_search(&id) {
        set.insert(at, id);
    }
}

fn set_remove(set: &mut NoteSet, id: NoteId) {
    if let Ok(at) = set.binary_search(&id) {
        set.remove(at);
    }
}

/// Заметка в индексе: путь (общий с картой путей) и запись.
#[derive(Debug)]
struct Slot {
    path: Arc<VaultPath>,
    entry: NoteEntry,
}

/// Индекс хранилища в памяти.
#[derive(Debug, Default)]
pub struct VaultIndex {
    /// Путь заметки → номер.
    ids: HashMap<Arc<VaultPath>, NoteId>,
    /// Заметки по номеру; `None` — номер свободен.
    slots: Vec<Option<Slot>>,
    /// Свободные номера.
    free: Vec<NoteId>,
    /// Ключ имени цели ссылки → заметки, в которых есть такие ссылки.
    by_target: HashMap<NameKey, NoteSet>,
    /// Ключ тега → заметки и написания.
    tags: HashMap<String, TagStat>,
    state: State,
    /// Растёт при каждом изменении содержимого.
    revision: u64,
    /// Номер последней начатой сборки.
    last_build: u64,
}

/// Строится или готов.
#[derive(Debug)]
enum State {
    Building(BuildState),
    Ready,
}

impl Default for State {
    fn default() -> Self {
        Self::Building(BuildState::default())
    }
}

/// Ход сборки.
#[derive(Debug, Default)]
struct BuildState {
    /// Номер сборки: более новая сборка останавливает старую.
    id: u64,
    done: u32,
    total: u32,
    /// Заметки, обновлённые или удалённые снаружи во время сборки.
    touched: HashSet<VaultPath>,
}

/// Тег: заметки с ним и написания.
#[derive(Debug, Default)]
struct TagStat {
    notes: NoteSet,
    /// Написание (NFC) → вхождения.
    spellings: HashMap<String, Spelling>,
}

/// Написание тега: вхождений во всём хранилище и в каких заметках.
#[derive(Debug, Default)]
struct Spelling {
    count: u32,
    notes: NoteSet,
}

/// Ключ имени цели ссылки; цель, которая не может быть путём, — `None`.
fn target_key(link: &WikiLink) -> Option<NameKey> {
    parse_target(&link.target)
        .ok()
        .map(|target| names::key(&target.name))
}

/// Ключ имени заметки без `.md`.
fn note_key(path: &VaultPath) -> Option<NameKey> {
    crate::switcher::link_path(path)?
        .last()
        .map(|name| names::key(name))
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// Путь строкой — порядок при равных датах (как у генератора, Р18).
fn path_text(path: &VaultPath) -> String {
    path.components().join("/")
}

fn sort_paths(paths: &mut [VaultPath]) {
    paths.sort_by(|a, b| a.components().cmp(b.components()));
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl VaultIndex {
    /// Пустой индекс, который ещё строится.
    pub fn new() -> Self {
        Self::default()
    }

    /// Готовый индекс из записей (файл индекса, задача 0.2-1.3; сборка с
    /// нуля в тестах).
    pub fn from_entries(entries: impl IntoIterator<Item = (VaultPath, NoteEntry)>) -> Self {
        let mut index = Self::new();
        for (path, entry) in entries {
            index.put(path, entry);
        }
        index.state = State::Ready;
        index.shrink();
        index
    }

    /// Записи всех заметок — снимок для файла индекса. Порядок не задан.
    pub fn entries(&self) -> impl Iterator<Item = (&VaultPath, &NoteEntry)> {
        self.slots
            .iter()
            .flatten()
            .map(|slot| (&*slot.path, &slot.entry))
    }

    /// Запись заметки `path`, если есть (сверка с диском, задача 0.2-1.3).
    pub fn entry(&self, path: &VaultPath) -> Option<&NoteEntry> {
        let id = *self.ids.get(path)?;
        self.slot(id).map(|slot| &slot.entry)
    }

    /// Строится или готов и ход сборки (команда `index_status`, событие
    /// `index:status`). У готового индекса `done = total` = число заметок.
    pub fn status(&self) -> IndexStatus {
        match &self.state {
            State::Building(build) => IndexStatus {
                state: IndexState::Building,
                done: build.done,
                total: build.total,
            },
            State::Ready => {
                let n = count(self.ids.len());
                IndexStatus {
                    state: IndexState::Ready,
                    done: n,
                    total: n,
                }
            }
        }
    }

    /// Номер изменения содержимого: меняется, только если запись заметки
    /// добавлена, стала другой или удалена (запись файла индекса «если
    /// изменился», событие `index:changed`).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Добавляет или заменяет запись заметки (своя запись, пачка
    /// наблюдателя). Чтение и разбор файла — до вызова, вне замка индекса
    /// ([`super::read_entry`]). Во время сборки эта запись главнее
    /// прочитанной сборкой.
    pub fn upsert(&mut self, path: VaultPath, entry: NoteEntry) {
        self.touch(&path);
        self.put(path, entry);
    }

    /// Убирает заметку (удалена, переименована в не-заметку); нет такой —
    /// ничего не меняется.
    pub fn remove(&mut self, path: &VaultPath) {
        self.touch(path);
        self.take(path);
    }

    /// Заметки, ссылающиеся на `note`, по ссылкам, которые `links`
    /// разрешает в `note` сейчас (правила С3.4); без самой `note`. Ссылки на
    /// заголовок и встраивания считаются. Порядок — по пути (порядок показа
    /// задаёт команда); ссылки заметки — по порядку в тексте.
    pub fn backlinks(&self, note: &VaultPath, links: &Links) -> Vec<BacklinkSource> {
        let Some(name) = note_key(note) else {
            return Vec::new();
        };
        let view = links.view();
        let mut out: Vec<BacklinkSource> = self
            .links_named(&name)
            .filter(|(from, link)| {
                *from != note && view.resolve(&link.target).as_ref() == Some(note)
            })
            .fold(Vec::new(), |mut out: Vec<BacklinkSource>, (from, link)| {
                let site = LinkSite {
                    line: link.line,
                    span: link.span.clone(),
                };
                // Ссылки одной заметки идут подряд (`links_named`).
                match out.last_mut().filter(|b| &b.from == from) {
                    Some(source) => source.sites.push(site),
                    None => out.push(BacklinkSource {
                        from: from.clone(),
                        sites: vec![site],
                    }),
                }
                out
            });
        out.sort_by(|a, b| a.from.components().cmp(b.from.components()));
        out
    }

    /// Все ссылки, у которых ключ имени цели — `name` (`[[Встреча]]`,
    /// `[[Папка/встреча#x]]`), с заметкой, где они стоят: кандидаты для
    /// обратных ссылок и плана переписывания (`relink`). Ссылки одной
    /// заметки идут подряд, по порядку в тексте; порядок заметок не задан.
    pub fn links_named<'a>(
        &'a self,
        name: &NameKey,
    ) -> impl Iterator<Item = (&'a VaultPath, &'a WikiLink)> + 'a {
        let name = name.clone();
        self.by_target
            .get(&name)
            .into_iter()
            .flatten()
            .filter_map(|&id| self.slot(id))
            .flat_map(move |slot| {
                let name = name.clone();
                let from = &*slot.path;
                slot.entry
                    .parsed
                    .links
                    .iter()
                    .filter(move |link| target_key(link).as_ref() == Some(&name))
                    .map(move |link| (from, link))
            })
    }

    /// Все теги хранилища, у которых есть заметки. Отображаемое написание —
    /// самое частое по вхождениям во всём хранилище; при равенстве — только
    /// среди равных по максимуму, из самой старой заметки (дата создания,
    /// затем путь), в ней — первое по порядку (решение 113, Р18). Порядок
    /// списка не задан (его задаёт интерфейс).
    pub fn tags(&self) -> Vec<TagCount> {
        self.tags
            .iter()
            .map(|(key, stat)| TagCount {
                display: self.display(stat),
                key: key.clone(),
                notes: count(stat.notes.len()),
            })
            .collect()
    }

    /// Заметки с тегом по ключу (`markdown::tag_key`), по пути; неизвестный
    /// ключ — пусто.
    pub fn tag_notes(&self, key: &str) -> Vec<VaultPath> {
        let mut notes: Vec<VaultPath> = self
            .tags
            .get(key)
            .map(|stat| self.paths(&stat.notes).cloned().collect())
            .unwrap_or_default();
        sort_paths(&mut notes);
        notes
    }

    /// Одинаковое ли содержимое (записи и производные карты) — для тестов.
    /// Номера заметок у двух индексов разные, поэтому карты сравниваются по
    /// путям.
    #[cfg(test)]
    pub(super) fn same_content(&self, other: &Self) -> bool {
        self.check_consistent();
        other.check_consistent();
        let entries = |index: &Self| -> HashMap<VaultPath, NoteEntry> {
            index
                .entries()
                .map(|(path, entry)| (path.clone(), entry.clone()))
                .collect()
        };
        let by_target = |index: &Self| -> HashMap<NameKey, HashSet<VaultPath>> {
            index
                .by_target
                .iter()
                .map(|(key, set)| (key.clone(), index.paths(set).cloned().collect()))
                .collect()
        };
        type Spellings = HashMap<String, (u32, HashSet<VaultPath>)>;
        let tags = |index: &Self| -> HashMap<String, (HashSet<VaultPath>, Spellings)> {
            index
                .tags
                .iter()
                .map(|(key, stat)| {
                    let spellings = stat
                        .spellings
                        .iter()
                        .map(|(name, s)| {
                            (
                                name.clone(),
                                (s.count, index.paths(&s.notes).cloned().collect()),
                            )
                        })
                        .collect();
                    (
                        key.clone(),
                        (index.paths(&stat.notes).cloned().collect(), spellings),
                    )
                })
                .collect()
        };
        entries(self) == entries(other)
            && by_target(self) == by_target(other)
            && tags(self) == tags(other)
    }

    /// Номера, пути и наборы согласованы (для тестов).
    #[cfg(test)]
    fn check_consistent(&self) {
        let live = self.slots.iter().flatten().count();
        assert_eq!(live, self.ids.len(), "заметок по номерам и по путям");
        assert_eq!(
            live + self.free.len(),
            self.slots.len(),
            "занятые и свободные номера"
        );
        for (path, &id) in &self.ids {
            assert_eq!(self.slot(id).map(|s| &s.path), Some(path), "номер пути");
        }
        let sets = self
            .by_target
            .values()
            .chain(self.tags.values().map(|t| &t.notes))
            .chain(
                self.tags
                    .values()
                    .flat_map(|t| t.spellings.values().map(|s| &s.notes)),
            );
        for set in sets {
            assert!(set.windows(2).all(|w| w[0] < w[1]), "набор упорядочен");
            assert!(
                set.iter().all(|&id| self.slot(id).is_some()),
                "живые номера"
            );
        }
    }

    // --- Сборка (модуль `build`) ----------------------------------------

    /// Начинает сборку: индекс «строится», ход с нуля. Возвращает номер
    /// сборки; предыдущая сборка с этого момента ничего не вставляет.
    pub(super) fn begin_build(&mut self) -> u64 {
        self.last_build += 1;
        self.state = State::Building(BuildState {
            id: self.last_build,
            ..BuildState::default()
        });
        self.last_build
    }

    /// Ход сборки `id`, если она ещё идёт.
    fn build_mut(&mut self, id: u64) -> Option<&mut BuildState> {
        match &mut self.state {
            State::Building(build) if build.id == id => Some(build),
            _ => None,
        }
    }

    /// Сколько заметок всего. `false` — сборку сменила другая.
    pub(super) fn set_total(&mut self, id: u64, total: usize) -> bool {
        self.build_mut(id).map(|b| b.total = count(total)).is_some()
    }

    /// Пачка, прочитанная сборкой `id`: `None` — заметку прочитать не
    /// удалось (удалена во время сборки). Заметки, изменённые снаружи во
    /// время сборки, не перекрываются. `false` — сборку сменила другая.
    pub(super) fn insert_built(
        &mut self,
        id: u64,
        batch: Vec<(VaultPath, Option<NoteEntry>)>,
    ) -> bool {
        let Some(build) = self.build_mut(id) else {
            return false;
        };
        build.done = build.done.saturating_add(count(batch.len()));
        let fresh: Vec<(VaultPath, NoteEntry)> = batch
            .into_iter()
            .filter_map(|(path, entry)| Some((path, entry?)))
            .filter(|(path, _)| !build.touched.contains(path))
            .collect();
        for (path, entry) in fresh {
            self.put(path, entry);
        }
        true
    }

    /// Конец сборки `id`: записи заметок не из списка `listed` убираются
    /// (кроме добавленных снаружи во время сборки), индекс готов. `false` —
    /// сборку сменила другая.
    pub(super) fn finish_build(&mut self, id: u64, listed: &HashSet<VaultPath>) -> bool {
        let Some(build) = self.build_mut(id) else {
            return false;
        };
        let touched = std::mem::take(&mut build.touched);
        let gone: Vec<VaultPath> = self
            .ids
            .keys()
            .filter(|path| !listed.contains(&***path) && !touched.contains(&***path))
            .map(|path| VaultPath::clone(path))
            .collect();
        for path in &gone {
            self.take(path);
        }
        self.state = State::Ready;
        self.shrink();
        true
    }

    // --- Пересчёт по одной заметке -------------------------------------

    fn slot(&self, id: NoteId) -> Option<&Slot> {
        self.slots.get(id as usize)?.as_ref()
    }

    /// Пути заметок набора.
    fn paths<'a>(&'a self, set: &'a NoteSet) -> impl Iterator<Item = &'a VaultPath> + 'a {
        set.iter()
            .filter_map(|&id| self.slot(id))
            .map(|slot| &*slot.path)
    }

    /// Отдаёт лишнюю ёмкость карт и наборов после загрузки или сборки.
    fn shrink(&mut self) {
        self.ids.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.free.shrink_to_fit();
        self.by_target.shrink_to_fit();
        for set in self.by_target.values_mut() {
            set.shrink_to_fit();
        }
        self.tags.shrink_to_fit();
        for stat in self.tags.values_mut() {
            stat.notes.shrink_to_fit();
            stat.spellings.shrink_to_fit();
            for spelling in stat.spellings.values_mut() {
                spelling.notes.shrink_to_fit();
            }
        }
    }

    fn touch(&mut self, path: &VaultPath) {
        if let State::Building(build) = &mut self.state {
            build.touched.insert(path.clone());
        }
    }

    /// Ставит запись: вклад старой вычитается, новой — прибавляется.
    fn put(&mut self, path: VaultPath, mut entry: NoteEntry) {
        if self.entry(&path) == Some(&entry) {
            return;
        }
        self.take(&path);
        let id = match self.free.pop() {
            Some(id) => id,
            None => {
                // Номеров `u32` хватает с запасом: заметок в хранилище на
                // порядки меньше.
                let id = NoteId::try_from(self.slots.len()).unwrap_or(NoteId::MAX);
                self.slots.push(None);
                id
            }
        };
        self.add_derived(id, &entry);
        // Разбор растит векторы удвоением — лишняя ёмкость живёт, пока
        // заметка в индексе.
        let parsed = &mut entry.parsed;
        parsed.headings.shrink_to_fit();
        parsed.tags.shrink_to_fit();
        parsed.links.shrink_to_fit();
        let path = Arc::new(path);
        self.ids.insert(Arc::clone(&path), id);
        self.slots[id as usize] = Some(Slot { path, entry });
        self.revision += 1;
    }

    /// Убирает запись и её вклад.
    fn take(&mut self, path: &VaultPath) {
        let Some(id) = self.ids.remove(path) else {
            return;
        };
        let Some(old) = self.slots.get_mut(id as usize).and_then(Option::take) else {
            return;
        };
        self.free.push(id);
        self.remove_derived(id, &old.entry);
        self.revision += 1;
    }

    fn add_derived(&mut self, id: NoteId, entry: &NoteEntry) {
        for name in entry.parsed.links.iter().filter_map(target_key) {
            set_insert(self.by_target.entry(name).or_default(), id);
        }
        for tag in &entry.parsed.tags {
            let stat = self.tags.entry(tag_key(&tag.name)).or_default();
            set_insert(&mut stat.notes, id);
            let spelling = stat.spellings.entry(nfc(&tag.name)).or_default();
            spelling.count += 1;
            set_insert(&mut spelling.notes, id);
        }
    }

    fn remove_derived(&mut self, id: NoteId, entry: &NoteEntry) {
        for name in entry.parsed.links.iter().filter_map(target_key) {
            if let Some(from) = self.by_target.get_mut(&name) {
                set_remove(from, id);
                if from.is_empty() {
                    self.by_target.remove(&name);
                }
            }
        }
        for tag in &entry.parsed.tags {
            let key = tag_key(&tag.name);
            let Some(stat) = self.tags.get_mut(&key) else {
                continue;
            };
            set_remove(&mut stat.notes, id);
            let written = nfc(&tag.name);
            if let Some(spelling) = stat.spellings.get_mut(&written) {
                spelling.count = spelling.count.saturating_sub(1);
                set_remove(&mut spelling.notes, id);
                if spelling.count == 0 {
                    stat.spellings.remove(&written);
                }
            }
            if stat.notes.is_empty() {
                self.tags.remove(&key);
            }
        }
    }

    /// Отображаемое написание тега (решение 113, Р18).
    fn display(&self, stat: &TagStat) -> String {
        let max = stat.spellings.values().map(|s| s.count).max().unwrap_or(0);
        let tied: Vec<&String> = stat
            .spellings
            .iter()
            .filter(|(_, s)| s.count == max)
            .map(|(name, _)| name)
            .collect();
        if let [only] = tied.as_slice() {
            return (*only).clone();
        }
        let oldest = tied
            .iter()
            .flat_map(|name| &stat.spellings[*name].notes)
            .filter_map(|&id| self.slot(id))
            .min_by_key(|slot| (slot.entry.created_ms, path_text(&slot.path)));
        oldest
            .and_then(|slot| {
                slot.entry
                    .parsed
                    .tags
                    .iter()
                    .map(|tag| nfc(&tag.name))
                    .find(|name| tied.contains(&name))
            })
            .or_else(|| tied.into_iter().min().cloned())
            .unwrap_or_default()
    }
}
