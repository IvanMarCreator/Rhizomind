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
        let changes = self.resolve(diff, &disk, own);

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
    fn resolve(&self, diff: Diff, disk: &BTreeMap<Key, Node>, own: &OwnWrites) -> Vec<TreeChange> {
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
        changes
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
        let mut out = HashMap::new();
        let entries = match fs::read_dir(self.abs(dir)) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(err) => return Err(err),
        };
        for entry in entries {
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
    let id = file_id::get_file_id(entry.path()).ok();
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
                if !old.is_folder() && (!old.same_content(new) || old.id != new.id) {
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
        model().resolve(diff, &disk, &OwnWrites::new())
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
