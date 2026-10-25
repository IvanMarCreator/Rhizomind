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

use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Модель дерева одного хранилища.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TreeModel {
    root: PathBuf,
    nodes: BTreeMap<Key, Node>,
}

impl TreeModel {
    /// Обходит хранилище и строит модель (сверка при открытии).
    pub(crate) fn scan(root: &Path) -> io::Result<Self> {
        let mut model = Self {
            root: root.to_path_buf(),
            nodes: BTreeMap::new(),
        };
        fs::metadata(root)?;
        let mut nodes = BTreeMap::new();
        model.walk(&Key::new(), &mut nodes);
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
    pub(crate) fn rescan(&mut self, own: &OwnWrites) -> Vec<TreeChange> {
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
    ) -> Vec<TreeChange> {
        let targets = self.targets(paths);
        let (disk, model) = self.collect(&targets);
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
        changes
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
    fn collect(&self, targets: &BTreeSet<Key>) -> (BTreeMap<Key, Node>, BTreeMap<Key, Node>) {
        let mut disk = BTreeMap::new();
        let mut model = BTreeMap::new();
        let mut by_parent: BTreeMap<Key, Vec<&Key>> = BTreeMap::new();
        for target in targets {
            model.extend(self.range(target).map(|(k, n)| (k.clone(), n.clone())));
            match target.split_last() {
                None => self.walk(target, &mut disk),
                Some((_, parent)) => by_parent.entry(parent.to_vec()).or_default().push(target),
            }
        }
        for (parent, children) in by_parent {
            let listing = self.list(&parent);
            for target in children {
                let name = target.last().expect("не корень");
                if let Some(node) = listing.get(name) {
                    if node.is_folder() {
                        self.walk(target, &mut disk);
                    }
                    disk.insert(target.clone(), node.clone());
                }
            }
        }
        (disk, model)
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
            if own.is_own(
                &VaultPath::from_components(key.clone()),
                &self.abs(&key),
                new.size,
            ) {
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
            if let Some(from) = removed_index.find(&new, &removed) {
                let old = removed.remove(&from).expect("найден");
                created.remove(&to);
                if old.is_folder() {
                    absorb_content(&from, &to, &mut removed, &mut created, &mut modified);
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
        let mut applied: Vec<(Key, Key)> = Vec::new();
        let path = |key: Key| VaultPath::from_components(key);
        let mut changes = Vec::new();
        for (from, to) in renamed {
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

    /// Содержимое папки на диске: точные имена → элементы. Нет папки —
    /// пусто.
    fn list(&self, dir: &[String]) -> HashMap<String, Node> {
        let mut out = HashMap::new();
        let Ok(entries) = fs::read_dir(self.abs(dir)) else {
            return out;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue; // имя не UTF-8 в путях хранилища не выражается
            };
            if is_hidden(&name) {
                continue;
            }
            if let Some(node) = read_node(&entry, &name) {
                out.insert(name, node);
            }
        }
        out
    }

    /// Всё содержимое папки `dir` на диске, рекурсивно.
    fn walk(&self, dir: &Key, out: &mut BTreeMap<Key, Node>) {
        for (name, node) in self.list(dir) {
            let mut key = dir.clone();
            key.push(name);
            if node.is_folder() {
                self.walk(&key, out);
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

/// Элемент на диске; символические ссылки, точки соединения и прочее — нет.
fn read_node(entry: &fs::DirEntry, name: &str) -> Option<Node> {
    // `file_type` не переходит по ссылке; на Windows точка соединения тоже
    // считается ссылкой.
    let file_type = entry.file_type().ok()?;
    if file_type.is_symlink() {
        return None;
    }
    let id = file_id::get_file_id(entry.path()).ok();
    if file_type.is_dir() {
        return Some(Node {
            kind: EntryKind::Folder,
            size: 0,
            modified: None,
            id,
        });
    }
    if !file_type.is_file() {
        return None;
    }
    let meta = entry.metadata().ok()?;
    Some(Node {
        kind: if names::is_note(name) {
            EntryKind::Note
        } else {
            EntryKind::Attachment
        },
        size: meta.len(),
        modified: meta.modified().ok(),
        id,
    })
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

/// Идентификатор файла не переходит к новому файлу после удаления
/// прежнего. Так на NTFS (в 128-битном FileId есть счётчик повторного
/// использования); на Linux и macOS номер inode удалённого файла сразу
/// достаётся новому, поэтому там совпадение идентификатора подтверждается
/// размером и датой изменения (переименование их не меняет).
const FILE_ID_IS_UNIQUE: bool = cfg!(windows);

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
    /// исчезнувший с тем же идентификатором; если идентификатора нет —
    /// единственный исчезнувший файл с теми же размером и датой изменения.
    fn find(&self, new: &Node, gone: &BTreeMap<Key, Node>) -> Option<Key> {
        let same_kind = |old: &Node| old.kind == new.kind;
        if let Some(id) = new.id {
            let found = self.by_id.get(&id).into_iter().flatten().find(|key| {
                gone.get(*key).is_some_and(|old| {
                    same_kind(old)
                        && (FILE_ID_IS_UNIQUE || old.is_folder() || old.same_content(new))
                })
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
            .filter(|key| {
                gone.get(*key)
                    .is_some_and(|old| same_kind(old) && (old.id.is_none() || new.id.is_none()))
            });
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
    modified: &mut Vec<Key>,
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
        if old.kind != new.kind {
            continue;
        }
        if !old.is_folder() && !old.same_content(new) {
            modified.push(new_key.clone());
        }
        removed.remove(&old_key);
        created.remove(&new_key);
    }
}
