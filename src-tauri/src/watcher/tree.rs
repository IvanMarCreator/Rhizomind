//! Модель «дерево хранилища» — что наблюдатель знает о диске, и сверка её
//! с диском. Источник изменений для события `vault:changed`.
//!
//! Модель — все папки и файлы хранилища, кроме имён с точки в начале (в том
//! числе `.rhizomind/` и `.git/`) и символических ссылок и точек соединения
//! NTFS (за них контур хранилища не выходит). Для файла хранится размер,
//! дата изменения и идентификатор файла (`file-id`: FileId NTFS, inode).
//!
//! Сверка всегда сравнивает модель с диском, а события ОС только говорят,
//! где смотреть ([`TreeModel::reconcile`]). Поэтому после затихания событий
//! модель совпадает с диском, даже если события пришли не все или не в том
//! виде: новая папка обходится целиком, исчезнувшая — убирается со всем
//! содержимым, а Rescan сверяет всё хранилище ([`TreeModel::rescan`]).
//!
//! Переименование и перемещение распознаются среди исчезнувших и появившихся
//! путей одной пачки: по идентификатору файла, а если его нет — по размеру и
//! дате изменения (единственный подходящий файл). Папка переносится одним
//! изменением вместе с содержимым.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use file_id::FileId;

use super::own_writes::OwnWrites;
use crate::commands::{EntryKind, TreeChange, VaultPath};
use crate::names;

/// Путь внутри хранилища как ключ упорядоченной таблицы: содержимое папки
/// идёт в таблице сразу за ней.
type Key = Vec<String>;

/// Сколько раз за одну сверку искать старые пути перенесённых элементов.
const MAX_ORIGIN_ROUNDS: usize = 8;

/// Что модель знает об элементе.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    kind: EntryKind,
    /// Размер файла; у папки 0.
    size: u64,
    /// Дата изменения файла; у папки `None` (меняется вместе с содержимым).
    modified: Option<SystemTime>,
    /// Идентификатор файла или папки, если ОС его дала.
    id: Option<FileId>,
}

impl Node {
    fn is_folder(&self) -> bool {
        self.kind == EntryKind::Folder
    }

    /// Те же размер и дата — содержимое не менялось.
    fn same_content(&self, other: &Node) -> bool {
        self.size == other.size && self.modified == other.modified
    }
}

/// Итог сверки.
#[derive(Debug, Default)]
pub(crate) struct Reconciled {
    /// Изменения по порядку.
    pub(crate) changes: Vec<TreeChange>,
    /// Пути, которые не удалось прочитать (ошибка, но не «нет такого»):
    /// модель под ними не тронута, сверку надо повторить позже.
    pub(crate) retry: Vec<VaultPath>,
    /// Корня хранилища нет или он недоступен: изменений нет, модель не
    /// тронута.
    pub(crate) unavailable: bool,
    /// Исчезнувшие файлы, отложенные до окончательной проверки.
    pub(crate) held: Vec<VaultPath>,
}

/// Путь, который не удалось прочитать.
struct Unreadable {
    key: Key,
    /// Сам элемент на диске известен (не прочиталось только содержимое
    /// папки); иначе неизвестен и он.
    self_known: bool,
}

/// Модель дерева одного хранилища.
#[derive(Debug)]
pub(crate) struct TreeModel {
    root: PathBuf,
    nodes: BTreeMap<Key, Node>,
    #[cfg(test)]
    failing: HashSet<Key>,
    /// Тестовый шов: действие сразу после чтения списка папки.
    #[cfg(test)]
    hooks: Hooks,
}

/// Действия тестового шва по папкам.
#[cfg(test)]
#[derive(Default)]
struct Hooks(std::sync::Mutex<HashMap<Key, Box<dyn FnOnce() + Send>>>);

#[cfg(test)]
impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hooks")
    }
}

impl PartialEq for TreeModel {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.nodes == other.nodes
    }
}

impl TreeModel {
    /// Обходит хранилище и строит модель (сверка при открытии).
    pub(crate) fn scan(root: &Path) -> io::Result<Self> {
        let mut model = Self {
            root: root.to_path_buf(),
            nodes: BTreeMap::new(),
            #[cfg(test)]
            failing: HashSet::new(),
            #[cfg(test)]
            hooks: Hooks::default(),
        };
        if !fs::metadata(root)?.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "не папка"));
        }
        let mut nodes = BTreeMap::new();
        let mut unreadable = Vec::new();
        model.walk(&Key::new(), &mut nodes, &mut unreadable);
        if unreadable.iter().any(|u| u.key.is_empty()) {
            return Err(io::Error::other("корень хранилища не читается"));
        }
        // Непрочитанные папки при открытии остаются пустыми в модели;
        // их содержимое появится при следующей сверке этих путей.
        model.nodes = nodes;
        Ok(model)
    }

    /// Все элементы модели по порядку путей.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> Vec<(VaultPath, EntryKind)> {
        self.nodes
            .iter()
            .map(|(key, node)| (VaultPath::from_components(key.clone()), node.kind))
            .collect()
    }

    /// Полная сверка с диском (Rescan, массовые операции).
    /// Тестовый шов: чтение папки даёт ошибку доступа.
    #[cfg(test)]
    pub(crate) fn fail_listing(&mut self, path: &VaultPath) {
        self.failing.insert(path.components().to_vec());
    }

    /// Тестовый шов: `action` выполняется один раз сразу после чтения
    /// списка папки `dir` (например, перенос файла посреди обхода).
    #[cfg(test)]
    pub(crate) fn after_listing(&mut self, dir: VaultPath, action: Box<dyn FnOnce() + Send>) {
        self.hooks
            .0
            .lock()
            .expect("шов")
            .insert(dir.components().to_vec(), action);
    }

    /// Тестовый шов: ошибки чтения папок прошли.
    #[cfg(test)]
    pub(crate) fn clear_failures(&mut self) {
        self.failing.clear();
    }

    /// Сверка, в которой исчезнувший файл с идентификатором (его папка на
    /// месте) не выдаётся сразу: он остаётся в модели и возвращается в
    /// [`Reconciled::held`]. Если в следующей пачке он появится по новому
    /// пути, это будет перенос; окончательная проверка — обычной сверкой
    /// этих путей ([`TreeModel::reconcile`]).
    pub(crate) fn reconcile_holding(
        &mut self,
        paths: impl IntoIterator<Item = VaultPath>,
        own: &OwnWrites,
    ) -> Reconciled {
        self.reconcile_inner(paths, own, true)
    }

    /// Полная сверка с отложенными исчезновениями.
    pub(crate) fn rescan_holding(&mut self, own: &OwnWrites) -> Reconciled {
        self.reconcile_inner([VaultPath::root()], own, true)
    }

    #[cfg(test)]
    pub(crate) fn rescan(&mut self, own: &OwnWrites) -> Reconciled {
        self.reconcile([VaultPath::root()], own)
    }

    /// Сверяет с диском пути из событий ОС (и всё, что под ними) и
    /// возвращает изменения по порядку: переименования, исчезновения
    /// (содержимое раньше папки), появления (папка раньше содержимого),
    /// изменения содержимого. Своя запись ядра (журнал `own`) изменением не
    /// считается.
    pub(crate) fn reconcile(
        &mut self,
        paths: impl IntoIterator<Item = VaultPath>,
        own: &OwnWrites,
    ) -> Reconciled {
        self.reconcile_inner(paths, own, false)
    }

    fn reconcile_inner(
        &mut self,
        paths: impl IntoIterator<Item = VaultPath>,
        own: &OwnWrites,
        hold: bool,
    ) -> Reconciled {
        // Нет корня (флешку вынули, папку переименовали) или нет доступа к
        // нему — это не «удалено всё»: изменений нет, модель как была.
        if !fs::metadata(&self.root).is_ok_and(|m| m.is_dir()) {
            return Reconciled {
                unavailable: true,
                ..Reconciled::default()
            };
        }
        let mut targets = self.targets(paths);
        let (mut disk, mut model, mut unreadable) = self.collect_settled(&mut targets);
        // Исчезло что-то с идентификатором: возможно, его перенесли в папку,
        // которую обход уже прошёл (файла не оказалось ни там, ни там).
        // Второй взгляд: перенос к этому моменту закончен.
        let gone_with_id = model
            .iter()
            .any(|(key, node)| node.id.is_some() && !disk.contains_key(key));
        if gone_with_id {
            (disk, model, unreadable) = self.collect_settled(&mut targets);
        }
        if unreadable.iter().any(|u| u.key.is_empty()) {
            return Reconciled {
                unavailable: true,
                ..Reconciled::default()
            };
        }
        // Что не прочиталось, остаётся в модели как было.
        let mut retry = Vec::new();
        for u in &unreadable {
            let under: Vec<Key> = disk
                .range(u.key.clone()..)
                .take_while(|(k, _)| k.starts_with(&u.key))
                .filter(|(k, _)| !u.self_known || **k != u.key)
                .map(|(k, _)| k.clone())
                .collect();
            for key in under {
                disk.remove(&key);
            }
            disk.extend(
                self.range(&u.key)
                    .filter(|(k, _)| !u.self_known || **k != u.key)
                    .map(|(k, n)| (k.clone(), n.clone())),
            );
            retry.push(VaultPath::from_components(u.key.clone()));
        }
        let diff = diff(&disk, &model);
        let (changes, held) = self.resolve(diff, &disk, own, hold);
        // Неизвестный идентификатор при том же содержимом — прежний.
        for (key, node) in disk.iter_mut() {
            if node.id.is_none()
                && let Some(old) = self.nodes.get(key)
                && old.kind == node.kind
                && old.same_content(node)
            {
                node.id = old.id;
            }
        }
        // Отложенные остаются в модели как были.
        for key in &held {
            if let Some(node) = self.nodes.get(key) {
                disk.insert(key.clone(), node.clone());
            }
        }

        // Модель на этих путях становится равной диску.
        for target in &targets {
            let under: Vec<Key> = self.range(target).map(|(key, _)| key.clone()).collect();
            for key in under {
                self.nodes.remove(&key);
            }
        }
        self.nodes.extend(disk);
        Reconciled {
            changes,
            retry,
            unavailable: false,
            held: held.into_iter().map(VaultPath::from_components).collect(),
        }
    }

    /// Снимок диска на путях сверки, дополненный старыми путями переносов.
    fn collect_settled(
        &self,
        targets: &mut BTreeSet<Key>,
    ) -> (BTreeMap<Key, Node>, BTreeMap<Key, Node>, Vec<Unreadable>) {
        let (mut disk, mut model, mut unreadable) = self.collect(targets);
        // Появился элемент с идентификатором, который модель знает под другим
        // путём: возможно, это перенос, а событие о старом пути потерялось
        // (переполнение буфера) или придёт позже. Старый путь сверяется в
        // этой же пачке — тогда перенос распознаётся. Читаются только старые
        // пути, а уже прочитанное заново не обходится: иначе за время
        // повторного обхода находились новые перенесённые, и при пределе
        // кругов последние из них оставались без пары.
        for _ in 0..MAX_ORIGIN_ROUNDS {
            let origins = self.known_elsewhere(&disk, &model, targets);
            if origins.is_empty() {
                break;
            }
            let extra = self.targets(origins.into_iter().map(VaultPath::from_components));
            let (more_disk, more_model, more_unreadable) = self.collect(&extra);
            disk.extend(more_disk);
            model.extend(more_model);
            unreadable.extend(more_unreadable);
            targets.extend(extra);
        }
        self.drop_stale_duplicates(&mut disk);
        (disk, model, unreadable)
    }

    /// Пути модели вне путей сверки с идентификатором элемента, который на
    /// диске появился по новому пути.
    fn known_elsewhere(
        &self,
        disk: &BTreeMap<Key, Node>,
        model: &BTreeMap<Key, Node>,
        targets: &BTreeSet<Key>,
    ) -> Vec<Key> {
        let appeared: HashSet<FileId> = disk
            .iter()
            .filter(|(key, _)| !model.contains_key(*key))
            .filter_map(|(_, node)| node.id)
            .collect();
        if appeared.is_empty() {
            return Vec::new();
        }
        self.nodes
            .iter()
            .filter(|(key, node)| {
                node.id.is_some_and(|id| appeared.contains(&id))
                    && !(0..=key.len()).any(|n| targets.contains(&key[..n]))
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Пути для сверки: путь внутри папки, которой нет в модели, заменяется
    /// этой папкой (её надо обойти целиком); вложенные в другой путь
    /// отбрасываются.
    fn targets(&self, paths: impl IntoIterator<Item = VaultPath>) -> BTreeSet<Key> {
        let mut lifted = BTreeSet::new();
        for path in paths {
            let key = path.components();
            if key.iter().any(|c| is_hidden(c)) {
                continue;
            }
            let missing = (1..key.len()).find(|&n| !self.nodes.contains_key(&key[..n]));
            lifted.insert(key[..missing.unwrap_or(key.len())].to_vec());
        }
        // По порядку путей вложенные идут сразу за папкой.
        let mut targets: BTreeSet<Key> = BTreeSet::new();
        for key in lifted {
            if !targets.last().is_some_and(|kept| key.starts_with(kept)) {
                targets.insert(key);
            }
        }
        targets
    }

    /// Диск и модель на путях сверки. Имена на диске берутся из списка
    /// папки, а не по имени файла: на Windows поиск по имени не различает
    /// регистр, и переименование `a.md` → `A.md` иначе не видно.
    fn collect(
        &self,
        targets: &BTreeSet<Key>,
    ) -> (BTreeMap<Key, Node>, BTreeMap<Key, Node>, Vec<Unreadable>) {
        let mut disk = BTreeMap::new();
        let mut model = BTreeMap::new();
        let mut unreadable = Vec::new();
        let mut by_parent: BTreeMap<Key, Vec<&Key>> = BTreeMap::new();
        for target in targets {
            model.extend(self.range(target).map(|(k, n)| (k.clone(), n.clone())));
            match target.split_last() {
                None => self.walk(target, &mut disk, &mut unreadable),
                Some((_, parent)) => by_parent.entry(parent.to_vec()).or_default().push(target),
            }
        }
        for (parent, children) in by_parent {
            let Ok(listing) = self.list(&parent) else {
                unreadable.extend(children.into_iter().map(|key| Unreadable {
                    key: key.clone(),
                    self_known: false,
                }));
                continue;
            };
            for target in children {
                let name = target.last().expect("не корень");
                if let Some(node) = listing.get(name) {
                    if node.is_folder() {
                        self.walk(target, &mut disk, &mut unreadable);
                    }
                    disk.insert(target.clone(), node.clone());
                }
            }
        }
        self.drop_stale_duplicates(&mut disk);
        (disk, model, unreadable)
    }

    /// Обход не мгновенный: если элемент перенесли, пока шёл обход, он
    /// может оказаться в снимке дважды — в старой папке (прочитана раньше)
    /// и в новой. Пути с одинаковым идентификатором перепроверяются после
    /// обхода; исчезнувшие (со всем, что под ними) из снимка убираются, и
    /// перенос распознаётся как перенос. Жёсткие ссылки (оба пути есть)
    /// остаются.
    fn drop_stale_duplicates(&self, disk: &mut BTreeMap<Key, Node>) {
        let mut by_id: HashMap<FileId, Vec<&Key>> = HashMap::new();
        for (key, node) in disk.iter() {
            if let Some(id) = node.id {
                by_id.entry(id).or_default().push(key);
            }
        }
        let suspects: Vec<Key> = by_id
            .into_values()
            .filter(|keys| keys.len() > 1)
            .flatten()
            .cloned()
            .collect();
        // Список каждой папки — один раз на проверку.
        let mut listings: HashMap<Key, Option<HashMap<String, Node>>> = HashMap::new();
        for key in suspects {
            let Some((name, parent)) = key.split_last() else {
                continue;
            };
            let listing = listings
                .entry(parent.to_vec())
                .or_insert_with(|| self.list(parent).ok());
            // Папка не прочиталась — путь не трогаем.
            let still_there = listing.as_ref().is_none_or(|listing| {
                listing
                    .get(name)
                    .is_some_and(|node| node.id == disk.get(&key).and_then(|n| n.id))
            });
            if !still_there {
                let stale: Vec<Key> = disk
                    .range(key.clone()..)
                    .take_while(|(k, _)| k.starts_with(&key))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in stale {
                    disk.remove(&k);
                }
            }
        }
    }

    /// Изменения из разницы диска и модели: свои записи, переименования.
    fn resolve(
        &self,
        diff: Diff,
        disk: &BTreeMap<Key, Node>,
        own: &OwnWrites,
        hold: bool,
    ) -> (Vec<TreeChange>, Vec<Key>) {
        let Diff {
            mut removed,
            mut created,
            changed,
        } = diff;
        let mut renamed = Vec::new();
        let mut modified = Vec::new();
        // Файл заменён другим (новый идентификатор): прежний мог уехать на
        // новый путь этой же пачки — тогда это переименование.
        let mut displaced: BTreeMap<Key, Node> = BTreeMap::new();

        for (key, old) in changed {
            let new = &disk[&key];
            if own.is_own(&VaultPath::from_components(key.clone()), &self.abs(&key)) {
                continue;
            }
            match (old.id, new.id) {
                (Some(a), Some(b)) if a != b => {
                    displaced.insert(key, old);
                }
                _ => modified.push(key),
            }
        }

        let removed_index = Origins::index(&removed);
        let displaced_index = Origins::index(&displaced);
        let created_keys: Vec<Key> = created.keys().cloned().collect();
        for to in created_keys {
            let Some(new) = created.get(&to).cloned() else {
                continue; // уже ушёл вместе с папкой
            };
            if let Some(from) = removed_index
                .find(&new, &removed)
                .filter(|from| !new.is_folder() || folder_confirmed(from, &to, &removed, &created))
            {
                let old = removed.remove(&from).expect("найден");
                created.remove(&to);
                if old.is_folder() {
                    absorb_content(&from, &to, &mut removed, &mut created);
                } else if !old.same_content(&new) {
                    modified.push(to.clone());
                }
                own.forget(&VaultPath::from_components(from.clone()));
                renamed.push((from, to));
            } else if let Some(from) = displaced_index.find(&new, &displaced) {
                // Прежний файл уехал на `to`, а на его месте новый файл.
                let old = displaced.remove(&from).expect("найден");
                created.remove(&to);
                if !old.same_content(&new) {
                    modified.push(to.clone());
                }
                created.insert(from.clone(), disk[&from].clone());
                own.forget(&VaultPath::from_components(from.clone()));
                renamed.push((from, to));
            }
        }
        modified.extend(displaced.into_keys());

        // Исчезнувший без пары файл с идентификатором, чья папка не исчезла, —
        // возможно, перенос, о новом пути которого событие ещё не пришло.
        let mut held = Vec::new();
        if hold {
            held = removed
                .iter()
                .filter(|(key, node)| {
                    !node.is_folder()
                        && node.id.is_some()
                        && key
                            .split_last()
                            .is_some_and(|(_, parent)| !removed.contains_key(parent))
                        // Папка не уехала: иначе путь в модели повис бы.
                        && !renamed.iter().any(|(from, _)| key.starts_with(from))
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in &held {
                removed.remove(key);
            }
        }

        for key in removed.keys() {
            own.forget(&VaultPath::from_components(key.clone()));
        }

        // Изменения применяются по порядку, поэтому путь «откуда» и пути
        // исчезнувших пишутся с учётом уже применённых переименований:
        // файл, вынесенный из переименованной папки, или удалённый в ней,
        // к этому моменту лежит уже под новым именем папки. Папки — раньше
        // файлов и от верхних к вложенным: новые места уже есть.
        let is_folder = |key: &Key| disk.get(key).is_some_and(Node::is_folder);
        renamed.sort_by_key(|(from, to)| (!is_folder(to), to.len(), from.clone()));
        // Новая папка, в которую что-то переносится, появляется раньше
        // переноса.
        let mut applied: Vec<(Key, Key)> = Vec::new();
        let path = |key: Key| VaultPath::from_components(key);
        let mut changes = Vec::new();
        for (from, to) in renamed {
            for n in 1..to.len() {
                if let Some(node) = created.remove(&to[..n]) {
                    changes.push(TreeChange::Created {
                        path: path(to[..n].to_vec()),
                        entry_kind: node.kind,
                    });
                }
            }
            let from = through(&applied, from);
            changes.push(TreeChange::Renamed {
                from: path(from.clone()),
                to: path(to.clone()),
            });
            applied.push((from, to));
        }
        changes.extend(removed.into_keys().rev().map(|key| TreeChange::Removed {
            path: path(through(&applied, key)),
        }));
        changes.extend(created.into_iter().map(|(key, node)| TreeChange::Created {
            path: path(key),
            entry_kind: node.kind,
        }));
        modified.sort();
        modified.dedup();
        changes.extend(
            modified
                .into_iter()
                .map(|key| TreeChange::Modified { path: path(key) }),
        );
        (changes, held)
    }

    /// Элементы модели: путь и всё под ним.
    fn range<'a>(&'a self, key: &'a Key) -> impl Iterator<Item = (&'a Key, &'a Node)> + 'a {
        self.nodes
            .range(key.clone()..)
            .take_while(move |(k, _)| k.starts_with(key))
    }

    fn abs(&self, key: &[String]) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(key);
        path
    }

    /// Содержимое папки на диске: точные имена → элементы. Папки нет —
    /// пусто; другая ошибка (нет доступа, диск отключается) — ошибка: это
    /// не значит, что содержимого нет.
    fn list(&self, dir: &[String]) -> io::Result<HashMap<String, Node>> {
        #[cfg(test)]
        if self.failing.contains(dir) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let path = self.abs(dir);
        let listed = match listed_at_once(&path, dir.is_empty()) {
            Ok(Some(out)) => Ok(out),
            Ok(None) => not_a_link(&path, dir.is_empty()).and_then(|()| list_entry_by_entry(&path)),
            Err(err) => Err(err),
        };
        let out = match listed {
            Ok(out) => out,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
            Err(err) => return Err(err),
        };
        #[cfg(test)]
        if let Some(action) = self.hooks.0.lock().expect("шов").remove(dir) {
            action();
        }
        Ok(out)
    }

    /// Всё содержимое папки `dir` на диске, рекурсивно. Папки, которые не
    /// прочитались, — в `unreadable`.
    fn walk(&self, dir: &Key, out: &mut BTreeMap<Key, Node>, unreadable: &mut Vec<Unreadable>) {
        let Ok(listing) = self.list(dir) else {
            unreadable.push(Unreadable {
                key: dir.clone(),
                self_known: true,
            });
            return;
        };
        // По порядку имён: обход воспроизводим.
        let mut listing: Vec<_> = listing.into_iter().collect();
        listing.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, node) in listing {
            let mut key = dir.clone();
            key.push(name);
            if node.is_folder() {
                self.walk(&key, out, unreadable);
            }
            out.insert(key, node);
        }
    }
}

/// Где окажется путь после переименований `applied` (по порядку).
fn through(applied: &[(Key, Key)], mut key: Key) -> Key {
    for (from, to) in applied {
        if key.starts_with(from) {
            let mut moved = to.clone();
            moved.extend_from_slice(&key[from.len()..]);
            key = moved;
        }
    }
    key
}

/// Список папки одним чтением, без открытия её содержимого (Windows,
/// [`super::listing_windows`]): пока папка читается, её нельзя перенести, а
/// открытый файл внутри держал бы её и дольше. `None` — так прочитать
/// нельзя (другая ОС, файловая система без идентификаторов в списке).
#[cfg(windows)]
fn listed_at_once(path: &Path, is_root: bool) -> io::Result<Option<HashMap<String, Node>>> {
    use super::listing_windows::{self, Kind};
    // Корень хранилища — и по ссылке; папка внутри — только сама.
    let Some(entries) = listing_windows::list(path, is_root)? else {
        return Ok(None);
    };
    let mut out = HashMap::new();
    for entry in entries {
        if is_hidden(&entry.name) {
            continue;
        }
        let node = match entry.kind {
            Kind::Link => continue,
            Kind::Folder => Node {
                kind: EntryKind::Folder,
                size: 0,
                modified: None,
                id: entry.id,
            },
            Kind::File => Node {
                kind: if names::is_note(&entry.name) {
                    EntryKind::Note
                } else {
                    EntryKind::Attachment
                },
                size: entry.size,
                modified: entry.modified,
                id: entry.id,
            },
        };
        out.insert(entry.name, node);
    }
    Ok(Some(out))
}

#[cfg(not(windows))]
fn listed_at_once(_path: &Path, _is_root: bool) -> io::Result<Option<HashMap<String, Node>>> {
    Ok(None)
}

/// Папка внутри хранилища, ставшая символической ссылкой или точкой
/// соединения, не читается (за ней — не хранилище); корень — читается.
fn not_a_link(path: &Path, is_root: bool) -> io::Result<()> {
    if !is_root && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other(
            "папка стала ссылкой: за контур не читается",
        ));
    }
    Ok(())
}

/// Список папки по элементам: идентификатор каждого — отдельным открытием
/// (`file-id`). На Linux и macOS это `stat`, без дескриптора.
fn list_entry_by_entry(path: &Path) -> io::Result<HashMap<String, Node>> {
    let mut out = HashMap::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue; // имя не UTF-8 в путях хранилища не выражается
        };
        if is_hidden(&name) {
            continue;
        }
        if let Some(node) = read_node(&entry, &name)? {
            out.insert(name, node);
        }
    }
    Ok(out)
}

#[cfg(test)]
thread_local! {
    /// Тесты: сколько раз этот поток открывал элемент папки ради его
    /// идентификатора ([`read_node`]).
    pub(crate) static ENTRY_OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Имя с точки в начале: дерево его не показывает, слежение не учитывает.
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Элемент на диске; символические ссылки, точки соединения и прочее —
/// `None`, файл исчез между списком и чтением — тоже. Другая ошибка чтения
/// — ошибка.
fn read_node(entry: &fs::DirEntry, name: &str) -> io::Result<Option<Node>> {
    let gone = |err: io::Error| {
        if err.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(err)
        }
    };
    // `file_type` не переходит по ссылке; на Windows точка соединения тоже
    // считается ссылкой.
    let file_type = match entry.file_type() {
        Ok(t) => t,
        Err(err) => return gone(err),
    };
    if file_type.is_symlink() {
        return Ok(None);
    }
    // Файл исчез между списком папки и открытием (перенесли, удалили) —
    // его здесь уже нет; другая ошибка — идентификатор неизвестен.
    #[cfg(test)]
    ENTRY_OPENS.with(|opens| opens.set(opens.get() + 1));
    let id = match file_id::get_file_id(entry.path()) {
        Ok(id) => Some(id),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => None,
    };
    if file_type.is_dir() {
        return Ok(Some(Node {
            kind: EntryKind::Folder,
            size: 0,
            modified: None,
            id,
        }));
    }
    if !file_type.is_file() {
        return Ok(None);
    }
    let meta = match entry.metadata() {
        Ok(meta) => meta,
        Err(err) => return gone(err),
    };
    Ok(Some(Node {
        kind: if names::is_note(name) {
            EntryKind::Note
        } else {
            EntryKind::Attachment
        },
        size: meta.len(),
        modified: meta.modified().ok(),
        id,
    }))
}

/// Разница диска и модели на путях сверки.
struct Diff {
    /// Есть в модели, нет на диске (или другой вид элемента).
    removed: BTreeMap<Key, Node>,
    /// Есть на диске, нет в модели (или другой вид элемента).
    created: BTreeMap<Key, Node>,
    /// Файл есть и там и там, но другой размер, дата или идентификатор;
    /// значение — прежний элемент из модели.
    changed: Vec<(Key, Node)>,
}

fn diff(disk: &BTreeMap<Key, Node>, model: &BTreeMap<Key, Node>) -> Diff {
    let mut out = Diff {
        removed: BTreeMap::new(),
        created: BTreeMap::new(),
        changed: Vec::new(),
    };
    for (key, old) in model {
        match disk.get(key) {
            Some(new) if new.kind == old.kind => {
                // Идентификатор сравнивается, только если известен с обеих
                // сторон: неизвестный (файл ушёл между списком и открытием)
                // правкой не считается.
                let other_id = matches!((old.id, new.id), (Some(a), Some(b)) if a != b);
                if !old.is_folder() && (!old.same_content(new) || other_id) {
                    out.changed.push((key.clone(), old.clone()));
                }
            }
            _ => {
                out.removed.insert(key.clone(), old.clone());
            }
        }
    }
    for (key, new) in disk {
        if model.get(key).is_none_or(|old| old.kind != new.kind) {
            out.created.insert(key.clone(), new.clone());
        }
    }
    out
}

/// Указатель исчезнувших элементов для поиска переименований.
struct Origins {
    by_id: HashMap<FileId, Vec<Key>>,
    by_content: HashMap<(u64, SystemTime), Vec<Key>>,
}

impl Origins {
    fn index(gone: &BTreeMap<Key, Node>) -> Self {
        let mut index = Self {
            by_id: HashMap::new(),
            by_content: HashMap::new(),
        };
        for (key, node) in gone {
            if let Some(id) = node.id {
                index.by_id.entry(id).or_default().push(key.clone());
            }
            if let (false, Some(modified)) = (node.is_folder(), node.modified) {
                index
                    .by_content
                    .entry((node.size, modified))
                    .or_default()
                    .push(key.clone());
            }
        }
        index
    }

    /// Откуда приехал появившийся элемент: ещё не сопоставленный
    /// исчезнувший с тем же идентификатором (у файла — ещё и с теми же
    /// размером и датой: идентификатор удалённого файла может достаться
    /// новому — inode на Linux, FAT/exFAT); иначе — единственный исчезнувший
    /// файл с теми же размером и датой изменения, даже с другим
    /// идентификатором (на FAT он меняется при переносе).
    fn find(&self, new: &Node, gone: &BTreeMap<Key, Node>) -> Option<Key> {
        let same_kind = |old: &Node| old.kind == new.kind;
        if let Some(id) = new.id {
            let found = self.by_id.get(&id).into_iter().flatten().find(|key| {
                gone.get(*key)
                    .is_some_and(|old| same_kind(old) && (old.is_folder() || old.same_content(new)))
            });
            if let Some(key) = found {
                return Some(key.clone());
            }
        }
        let modified = new.modified.filter(|_| !new.is_folder())?;
        let mut candidates = self
            .by_content
            .get(&(new.size, modified))
            .into_iter()
            .flatten()
            .filter(|key| gone.get(*key).is_some_and(same_kind));
        match (candidates.next(), candidates.next()) {
            (Some(key), None) => Some(key.clone()),
            _ => None,
        }
    }
}

/// Папка `from` переехала в `to`: её содержимое с теми же относительными
/// путями — часть одного переименования, а не отдельные изменения.
fn absorb_content(
    from: &Key,
    to: &Key,
    removed: &mut BTreeMap<Key, Node>,
    created: &mut BTreeMap<Key, Node>,
) {
    let inner: Vec<Key> = removed
        .range(from.clone()..)
        .take_while(|(k, _)| k.starts_with(from))
        .map(|(k, _)| k.clone())
        .collect();
    for old_key in inner {
        let mut new_key = to.clone();
        new_key.extend_from_slice(&old_key[from.len()..]);
        let (Some(old), Some(new)) = (removed.get(&old_key), created.get(&new_key)) else {
            continue;
        };
        // Тот же файл — только с теми же размером и датой; иначе на этом
        // месте другой файл: исчез и появился.
        if old.kind != new.kind || (!old.is_folder() && !old.same_content(new)) {
            continue;
        }
        removed.remove(&old_key);
        created.remove(&new_key);
    }
}

/// Папка с тем же идентификатором — та же папка, если в ней нет файлов или
/// хотя бы один файл на том же относительном пути подтверждён размером и
/// датой: идентификатор папки тоже может достаться новой (FAT/exFAT).
fn folder_confirmed(
    from: &Key,
    to: &Key,
    removed: &BTreeMap<Key, Node>,
    created: &BTreeMap<Key, Node>,
) -> bool {
    let mut files = removed
        .range(from.clone()..)
        .take_while(|(k, _)| k.starts_with(from))
        .filter(|(_, n)| !n.is_folder())
        .peekable();
    if files.peek().is_none() {
        return true;
    }
    files.any(|(old_key, old)| {
        let mut new_key = to.clone();
        new_key.extend_from_slice(&old_key[from.len()..]);
        created
            .get(&new_key)
            .is_some_and(|new| new.kind == old.kind && old.same_content(new))
    })
}

#[cfg(test)]
mod tests {
    //! Распознавание переименований на синтетических элементах: совпадение
    //! идентификаторов, которого на NTFS не бывает (FAT/exFAT, inode Linux).

    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn file(size: u64, secs: u64, id: Option<u64>) -> Node {
        Node {
            kind: EntryKind::Note,
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
            id: id.map(|n| FileId::new_inode(1, n)),
        }
    }

    fn folder(id: u64) -> Node {
        Node {
            kind: EntryKind::Folder,
            size: 0,
            modified: None,
            id: Some(FileId::new_inode(1, id)),
        }
    }

    fn key(path: &str) -> Key {
        path.split('/').map(str::to_owned).collect()
    }

    fn model() -> TreeModel {
        TreeModel {
            root: PathBuf::from("нет-такой-папки"),
            nodes: BTreeMap::new(),
            #[cfg(test)]
            failing: HashSet::new(),
            #[cfg(test)]
            hooks: Hooks::default(),
        }
    }

    fn resolve(removed: &[(&str, Node)], created: &[(&str, Node)]) -> Vec<TreeChange> {
        let diff = Diff {
            removed: removed.iter().map(|(k, n)| (key(k), n.clone())).collect(),
            created: created.iter().map(|(k, n)| (key(k), n.clone())).collect(),
            changed: Vec::new(),
        };
        let disk = created.iter().map(|(k, n)| (key(k), n.clone())).collect();
        model().resolve(diff, &disk, &OwnWrites::new(), false).0
    }

    fn vp(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn same_file_id_with_other_size_or_date_is_not_a_rename() {
        let changes = resolve(
            &[("чужая.md", file(10, 1, Some(7)))],
            &[("новая.md", file(99, 5, Some(7)))],
        );
        assert_eq!(
            changes,
            vec![
                TreeChange::Removed {
                    path: vp("чужая.md")
                },
                TreeChange::Created {
                    path: vp("новая.md"),
                    entry_kind: EntryKind::Note
                },
            ]
        );
    }

    #[test]
    fn unique_same_size_and_date_is_a_rename_even_with_other_ids() {
        // FAT/exFAT: идентификатор меняется при переносе между папками.
        let changes = resolve(
            &[("а.md", file(10, 1, Some(7)))],
            &[("б.md", file(10, 1, Some(8)))],
        );
        assert_eq!(
            changes,
            vec![TreeChange::Renamed {
                from: vp("а.md"),
                to: vp("б.md")
            }]
        );
    }

    #[test]
    fn two_candidates_with_same_size_and_date_are_not_guessed() {
        let changes = resolve(
            &[
                ("а.md", file(10, 1, Some(7))),
                ("б.md", file(10, 1, Some(8))),
            ],
            &[("в.md", file(10, 1, Some(9)))],
        );
        assert!(
            !changes
                .iter()
                .any(|c| matches!(c, TreeChange::Renamed { .. }))
        );
    }

    #[test]
    fn folder_with_reused_id_and_foreign_content_is_not_a_rename() {
        let changes = resolve(
            &[("Старая", folder(5)), ("Старая/x.md", file(10, 1, Some(7)))],
            &[("Новая", folder(5)), ("Новая/x.md", file(20, 9, Some(8)))],
        );
        assert!(
            !changes
                .iter()
                .any(|c| matches!(c, TreeChange::Renamed { .. })),
            "{changes:?}"
        );
    }

    #[test]
    fn folder_rename_absorbs_only_confirmed_children() {
        let changes = resolve(
            &[
                ("Старая", folder(5)),
                ("Старая/своя.md", file(10, 1, Some(7))),
                ("Старая/x.md", file(10, 1, Some(17))),
            ],
            &[
                ("Новая", folder(5)),
                ("Новая/своя.md", file(10, 1, Some(7))),
                ("Новая/x.md", file(33, 9, Some(18))),
            ],
        );
        assert_eq!(
            changes,
            vec![
                TreeChange::Renamed {
                    from: vp("Старая"),
                    to: vp("Новая")
                },
                TreeChange::Removed {
                    path: vp("Новая/x.md")
                },
                TreeChange::Created {
                    path: vp("Новая/x.md"),
                    entry_kind: EntryKind::Note
                },
            ]
        );
    }
}

#[cfg(test)]
mod diff_tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn note(id: Option<u64>) -> Node {
        Node {
            kind: EntryKind::Note,
            size: 5,
            modified: Some(UNIX_EPOCH + Duration::from_secs(1)),
            id: id.map(|n| FileId::new_inode(1, n)),
        }
    }

    #[test]
    fn unknown_file_id_on_disk_is_not_a_change() {
        // Файл перенесли между чтением списка папки и открытием для
        // идентификатора (Windows): идентификатор неизвестен, а размер и
        // дата из списка — прежние. Это не правка файла.
        let key: Key = vec!["a.md".into()];
        let model = BTreeMap::from([(key.clone(), note(Some(7)))]);
        let disk = BTreeMap::from([(key, note(None))]);
        assert!(diff(&disk, &model).changed.is_empty());
    }
}

/// Задача 0.10: на Windows папка читается одним списком — её содержимое не
/// открывается, а данные элементов те же, что при чтении по элементам.
#[cfg(all(test, windows))]
mod windows_listing_tests {
    use super::*;

    fn sample() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("временная папка");
        let root = dir.path();
        for n in 0..20 {
            fs::write(root.join(format!("заметка {n}.md")), format!("# {n}\n")).expect("запись");
        }
        fs::write(root.join("картинка.png"), [0u8; 300]).expect("запись");
        fs::create_dir(root.join("Папка")).expect("папка");
        fs::write(root.join("Папка/вложенная.md"), "текст").expect("запись");
        fs::write(root.join(".скрытый.md"), "x").expect("запись");
        let outside = dir.path().join("Папка");
        junction::create(&outside, root.join("соединение")).expect("точка соединения");
        dir
    }

    #[test]
    fn listing_a_folder_opens_none_of_its_entries() {
        let dir = sample();
        ENTRY_OPENS.with(|opens| opens.set(0));
        let model = TreeModel::scan(dir.path()).expect("обход");
        assert_eq!(
            model.entries().len(),
            23,
            "20 заметок, картинка, папка и вложенная"
        );
        assert_eq!(
            ENTRY_OPENS.with(std::cell::Cell::get),
            0,
            "пока папка читается, её содержимое не открывается: иначе папку нельзя перенести"
        );
    }

    #[test]
    fn one_listing_gives_the_same_entries_as_reading_entry_by_entry() {
        let dir = sample();
        for folder in [dir.path().to_path_buf(), dir.path().join("Папка")] {
            let at_once = listed_at_once(&folder, folder == dir.path())
                .expect("список")
                .expect("NTFS отдаёт идентификаторы в списке");
            let by_entry = list_entry_by_entry(&folder).expect("список по элементам");
            assert_eq!(at_once, by_entry, "{}", folder.display());
        }
    }

    /// Задача 0.10б: папку подменили точкой соединения между чтением
    /// родителя и её самой — чтение не уходит за контур хранилища.
    #[test]
    fn folder_replaced_by_a_junction_is_not_followed() {
        let dir = sample();
        let outside = tempfile::tempdir().expect("папка вне хранилища");
        fs::write(outside.path().join("чужой.md"), "секрет").expect("запись");
        let model = TreeModel::scan(dir.path()).expect("обход");
        let folder = dir.path().join("Папка");
        fs::remove_dir_all(&folder).expect("убрать папку");
        junction::create(outside.path(), &folder).expect("точка соединения");
        let listed = model.list(&["Папка".to_owned()]);
        assert!(
            !listed
                .as_ref()
                .is_ok_and(|names| names.contains_key("чужой.md")),
            "содержимое за точкой соединения не читается: {listed:?}"
        );
        // Корень хранилища по ссылке (пользователь открыл ссылку) читается.
        let link_root = outside.path().join("ссылка-на-хранилище");
        junction::create(dir.path(), &link_root).expect("точка соединения на хранилище");
        let model = TreeModel::scan(&link_root).expect("хранилище по ссылке читается");
        assert!(!model.entries().is_empty());
    }

    #[test]
    fn missing_folder_lists_as_empty_and_file_is_not_a_folder() {
        let dir = sample();
        let mut model = TreeModel::scan(dir.path()).expect("обход");
        model.root = dir.path().join("нет такой");
        assert!(model.list(&[]).expect("пусто").is_empty());
        model.root = dir.path().join("картинка.png");
        assert!(model.list(&[]).is_err(), "файл — не папка");
    }
}
