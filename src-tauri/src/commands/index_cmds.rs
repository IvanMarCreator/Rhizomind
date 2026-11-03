//! Команды индекса хранилища (0.2 «Связи», С2-3.2, С2-3.3, С2-3.7, С2-5.5):
//! ход сборки, обратные ссылки, теги.
//!
//! Пока индекс строится, команды не ждут его и не отвечают ошибкой: ответ —
//! вариант `index_building` с ходом сборки ([`IndexReply`]), окно показывает
//! «Индекс строится… N %» и перезапрашивает по `index:status`. Так у
//! интерфейса нет гонки «состояние уже готово, а список ещё пуст».
//!
//! Индекс в работе окна (задача 0.2-1.4) — [`WindowIndex`]: загрузка или
//! сборка при открытии хранилища ([`start_index`]), обновление после своих
//! команд и пачек наблюдателя, события `index:status` и `index:changed` не
//! чаще [`limits::MAX_EVENTS_PER_SEC`] раз в секунду, запись `index.json`
//! не чаще раза в 30 с.
//!
//! **Граница с интерфейсом (С2-6).** Команды работают только с хранилищем
//! своего окна; строки обратных ссылок читаются через `Vault::read_note`
//! (контур хранилища, только заметки из индекса этого хранилища) по
//! положениям из индекса, которые проверяются по тексту (`text.get(span)`):
//! подменённый `index.json` не роняет ядро и не даёт прочитать чужое.
//! Строки отдаются как есть — текстом; окно показывает их как текст, не как
//! HTML.

use std::ops::Range;
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::CoreResult;
use super::events::{IndexChangedPayload, TreeChange, event};
use super::limits;
use super::note_list::NoteList;
use super::types::VaultPath;
use super::{Core, OpenVault, blocking, checked_path};
use crate::events::{Emit, send};
use crate::index::store::{self, IndexStore, Load, WriteOutcome};
use crate::index::{BacklinkSource, LinkSite, NoteEntry, VaultIndex, read_entry};
use crate::vault::{NoteRef, Vault};

/// Состояние индекса хранилища.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    /// Индекс строится или сверяется с диском (открытие хранилища, файла
    /// индекса нет или он испорчен). Обратные ссылки, теги и переименование
    /// с обновлением ссылок ждут готовности (С2-3.7, решение 114).
    Building,
    /// Индекс готов и обновляется по изменениям.
    Ready,
}

/// Ответ `index_status` и данные события `index:status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexStatus {
    /// Строится или готов.
    pub state: IndexState,
    /// Сколько заметок уже обработано.
    pub done: u32,
    /// Сколько заметок всего; `0` — ещё неизвестно (идёт обход папки).
    pub total: u32,
}

/// Ход сборки в ответе команды, которой нужен готовый индекс.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexProgress {
    /// Сколько заметок уже обработано.
    pub done: u32,
    /// Сколько заметок всего; `0` — ещё неизвестно.
    pub total: u32,
}

/// Ответ команды, которой нужен готовый индекс (`backlinks_list`,
/// `tags_list`, `tag_notes`, `relink_preview`): данные или «индекс
/// строится». В JSON — поле `status` и поля варианта на одном уровне:
/// `{ "status": "ready", ...данные }` или
/// `{ "status": "index_building", "done": 10, "total": 200 }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IndexReply<T> {
    /// Индекс готов — данные.
    Ready(T),
    /// Индекс строится — ход сборки; окно ждёт `index:status` с `ready`.
    IndexBuilding(IndexProgress),
}

impl<T> IndexReply<T> {
    /// «Индекс строится», ход ещё неизвестен (заглушка 0.2-0.1).
    pub fn building_unknown() -> Self {
        Self::IndexBuilding(IndexProgress { done: 0, total: 0 })
    }
}

/// Запрос `backlinks_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklinksListRequest {
    /// Заметка, обратные ссылки на которую нужны.
    pub path: VaultPath,
}

/// Одна ссылка на заметку в тексте другой заметки — строка панели.
/// Строка читается из файла по положению из индекса (в индексе текста
/// заметок нет, Р13) и отдаётся как текст: интерфейс показывает её как
/// текст, не как HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklinkLine {
    /// Номер строки в заметке, с 1.
    pub line: u32,
    /// Начало ссылки (`[[` или `![[`) в строке, единиц UTF-16 от начала
    /// строки, — куда поставить курсор при щелчке.
    pub column_utf16: u32,
    /// Строка с ссылкой, обрезанная вокруг ссылки до
    /// [`limits::MAX_SNIPPET_CHARS`] символов.
    pub snippet: String,
    /// Начало ссылки в `snippet`, единиц UTF-16 (выделение).
    pub from_utf16: u32,
    /// Конец ссылки в `snippet` (не включая), единиц UTF-16.
    pub to_utf16: u32,
}

/// Заметка, которая ссылается на запрошенную.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklinkNote {
    /// Ссылающаяся заметка.
    pub from: VaultPath,
    /// Имя заметки без `.md` — для показа.
    pub name: String,
    /// Сколько всего ссылок на запрошенную заметку в этой заметке.
    pub links_total: u32,
    /// Ссылки по порядку в тексте — не больше [`limits::MAX_BACKLINK_LINES`].
    pub links: Vec<BacklinkLine>,
}

/// Обратные ссылки заметки (`backlinks_list`, вариант `ready`). Ссылка
/// заметки на себя не считается; встраивание `![[…]]` и ссылка на заголовок
/// — считаются (С2-3.2, Р4). Ответ ограничен ([`Backlinks::capped`]):
/// не больше [`limits::MAX_BACKLINK_NOTES`] заметок,
/// [`limits::MAX_BACKLINK_LINES`] строк на заметку и
/// [`limits::MAX_BACKLINK_LINES_TOTAL`] строк на ответ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backlinks {
    /// Сколько всего заметок ссылается (заголовок вида «N заметок»).
    pub notes_total: u32,
    /// `true` — показано не всё: часть заметок или строк отброшена пределами
    /// (сколько всего — `notes_total` и `links_total` у заметок).
    pub truncated: bool,
    /// Заметки в естественном порядке имён, затем путей.
    pub notes: Vec<BacklinkNote>,
}

impl Backlinks {
    /// Ответ по полному списку ссылающихся заметок (уже в порядке показа, у
    /// каждой — все строки и верный `links_total`): пределы заметок, строк
    /// на заметку и строк на ответ; что не поместилось — отбрасывается с
    /// признаком `truncated`. Заметка, которой не досталось строк из общего
    /// предела, остаётся в списке без строк (её имя всё равно видно).
    pub fn capped(mut notes: Vec<BacklinkNote>) -> Self {
        let notes_total = u32::try_from(notes.len()).unwrap_or(u32::MAX);
        let mut truncated = notes.len() > limits::MAX_BACKLINK_NOTES;
        notes.truncate(limits::MAX_BACKLINK_NOTES);
        let mut budget = limits::MAX_BACKLINK_LINES_TOTAL;
        for note in &mut notes {
            let keep = note.links.len().min(limits::MAX_BACKLINK_LINES).min(budget);
            truncated |= keep < note.links.len();
            note.links.truncate(keep);
            budget -= keep;
        }
        Self {
            notes_total,
            truncated,
            notes,
        }
    }
}

/// Тег хранилища и число заметок с ним.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagSummary {
    /// Написание для показа, без `#`: самое частое, при равенстве — из самой
    /// старой заметки (решение 113).
    pub tag: String,
    /// Ключ тега (NFC → приведение регистра → NFC) — для `tag_notes`.
    pub key: String,
    /// Сколько заметок с этим тегом.
    pub count: u32,
}

/// Все теги хранилища (`tags_list`, вариант `ready`). Список плоский
/// (решение 113); порядок задаёт интерфейс.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tags {
    /// Теги; тег без заметок в список не попадает.
    pub tags: Vec<TagSummary>,
}

/// Запрос `tag_notes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagNotesRequest {
    /// Ключ тега из `tags_list`.
    pub key: String,
}

/// Заметки с тегом (`tag_notes`, вариант `ready`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagNotes {
    /// Сколько всего заметок с тегом.
    pub notes_total: u32,
    /// Заметки — не больше [`limits::MAX_TAG_NOTES`]; порядок задаёт
    /// интерфейс. Неизвестный тег — пустой список, не ошибка.
    pub notes: Vec<VaultPath>,
}

// --- Индекс в работе окна (задача 0.2-1.4) ---------------------------------

/// Как часто поток событий индекса просыпается без сигнала: проверить срок
/// записи `index.json` и не пора ли остановиться.
const IDLE_WAKE: Duration = Duration::from_secs(1);

/// Через сколько повторить загрузку или сборку, если хранилище недоступно.
const PREPARE_RETRY: Duration = Duration::from_secs(5);

/// Частые события (`index:status`, `index:changed`) — не чаще этого.
fn event_interval() -> Duration {
    Duration::from_millis(1_000 / u64::from(limits::MAX_EVENTS_PER_SEC))
}

/// Индекс хранилища окна в работе: индекс в памяти ([`VaultIndex`]), когда
/// его писать в `index.json` ([`IndexStore`]) и какие события индекса ждут
/// отправки окну.
///
/// - **Запуск** — [`start_index`] при открытии хранилища: загрузка
///   `index.json` и сверка с диском или сборка с нуля — в фоновом потоке;
///   второй поток отправляет события не чаще [`limits::MAX_EVENTS_PER_SEC`]
///   раз в секунду и пишет `index.json` не чаще раза в 30 с (первая запись —
///   не раньше 30 с после открытия).
/// - **Обновление** — [`WindowIndex::update`] после своей команды
///   (`OpenVault::apply_own`, `note_write`) и после пачки наблюдателя:
///   чтение и разбор заметок — **вне замков** индекса и списка заметок;
///   обновления идут по одному (`updates`), поэтому более позднее чтение
///   файла всегда вставляется позже.
/// - **Остановка** — [`WindowIndex::stop`], когда окно отвязано от
///   хранилища; при закрытии окна индекс пишет `windows::closing`
///   ([`WindowIndex::write_now`]).
///
/// Порядок замков: список заметок → индекс; замок индекса никогда не ждёт
/// список заметок. Чтение файлов — без замков.
pub struct WindowIndex {
    index: RwLock<VaultIndex>,
    store: Mutex<IndexStore>,
    /// Обновления по заметкам — по одному (чтение файла и вставка).
    updates: Mutex<()>,
    signal: Mutex<Signal>,
    wake: Condvar,
}

/// Что ждёт потока событий индекса.
#[derive(Debug, Default)]
struct Signal {
    /// Отправить `index:status`.
    status: bool,
    /// Отправить `index:changed`.
    changed: bool,
    /// Окно отвязано: потоки индекса выходят.
    stopped: bool,
    /// Поток событий сейчас отправляет события (флаги уже сняты).
    sending: bool,
}

impl std::fmt::Debug for WindowIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowIndex")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl Default for WindowIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowIndex {
    /// Пустой индекс, который ещё строится (до [`start_index`]).
    pub fn new() -> Self {
        Self {
            index: RwLock::new(VaultIndex::new()),
            store: Mutex::new(IndexStore::new()),
            updates: Mutex::new(()),
            signal: Mutex::default(),
            wake: Condvar::new(),
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, VaultIndex> {
        self.index.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, VaultIndex> {
        self.index.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn signal(&self) -> MutexGuard<'_, Signal> {
        self.signal.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Строится или готов и ход сборки.
    pub fn status(&self) -> IndexStatus {
        self.read().status()
    }

    /// Окно отвязано от хранилища: потоки индекса выходят (сборка — после
    /// текущей пачки заметок), события больше не отправляются, срочной
    /// записи `index.json` нет.
    pub fn stop(&self) {
        self.signal().stopped = true;
        self.wake.notify_all();
    }

    fn stopped(&self) -> bool {
        self.signal().stopped
    }

    /// Ставит события в очередь потока событий.
    fn notify(&self, status: bool, changed: bool) {
        let mut signal = self.signal();
        signal.status |= status;
        signal.changed |= changed;
        drop(signal);
        self.wake.notify_all();
    }

    /// Пишет `index.json` сразу (закрытие окна после записи заметок), если
    /// индекс готов и изменился с прошлой записи. Ошибка — молча.
    pub fn write_now(&self, vault: &Vault) -> WriteOutcome {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_now(vault, &self.index)
    }

    fn write_if_due(&self, vault: &Vault, now: Instant) -> WriteOutcome {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_if_due(vault, &self.index, now)
    }

    /// Загрузка `index.json` и сверка с диском или сборка с нуля. Ход —
    /// событиями `index:status`.
    fn prepare(&self, vault: &Vault) -> CoreResult<()> {
        match store::load(vault) {
            Load::Loaded(loaded) => {
                let revision = loaded.revision();
                // Записи, вставленные до загрузки (наблюдатель), перекрываются
                // файлом — сверка ниже перечитает всё, что на диске иначе.
                *self.write() = *loaded;
                *self.store.lock().unwrap_or_else(PoisonError::into_inner) =
                    IndexStore::loaded(revision);
                self.notify(true, true);
                store::reconcile(vault, &self.index)?;
                let changed = self.read().revision() != revision;
                self.notify(false, changed);
                Ok(())
            }
            Load::Rebuild(_) => {
                crate::index::rebuild(vault, &self.index, &|_| self.notify(true, false))?;
                self.notify(true, true);
                Ok(())
            }
        }
    }

    /// Применяет изменения заметок ([`IndexUpdate`]): переносы и удаления —
    /// сразу, заметки для чтения читаются и разбираются **вне замков** и
    /// вставляются после. Изменились обратные ссылки или теги — окну
    /// `index:changed`. Своя запись и повтор того же изменения от
    /// наблюдателя дают одну и ту же запись — второго события нет.
    pub fn update(&self, vault: &Vault, update: IndexUpdate) {
        if update.ops.is_empty() && !update.rescan {
            return;
        }
        let _serial = self.updates.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = false;
        // Порядок изменений важен (`а → б`, затем новая `а`): переносы и
        // удаления — по порядку; чтение — в конце, по состоянию диска.
        let mut reads: Vec<VaultPath> = Vec::new();
        {
            let mut index = self.write();
            for op in update.ops {
                match op {
                    Op::Read(path) => {
                        if !reads.contains(&path) {
                            reads.push(path);
                        }
                    }
                    Op::Remove(path) => {
                        changed |= index.entry(&path).is_some();
                        index.remove(&path);
                    }
                    Op::Move(from, to) => {
                        let entry = index.entry(&from).cloned();
                        changed |= entry.is_some();
                        index.remove(&from);
                        match entry {
                            // Заметку перед переносом ещё и меняли — читать
                            // уже по новому пути.
                            Some(entry) if !reads.contains(&from) => index.upsert(to, entry),
                            _ => {
                                if !reads.contains(&to) {
                                    reads.push(to);
                                }
                            }
                        }
                    }
                }
            }
        }
        let read: Vec<(VaultPath, Option<NoteEntry>)> = reads
            .into_iter()
            .map(|path| {
                let entry = vault.note_created_ms(&path).and_then(|created_ms| {
                    let note = NoteRef {
                        path: path.clone(),
                        created_ms,
                    };
                    read_entry(vault, &note).ok()
                });
                (path, entry)
            })
            .collect();
        {
            let mut index = self.write();
            for (path, entry) in read {
                changed |= affects_views(index.entry(&path), entry.as_ref());
                match entry {
                    Some(entry) => index.upsert(path, entry),
                    None => index.remove(&path),
                }
            }
        }
        if update.rescan && self.status().state == IndexState::Ready {
            let revision = self.read().revision();
            // Хранилище недоступно — сверка позже, при следующей пачке.
            let _ = store::reconcile(vault, &self.index);
            changed |= self.read().revision() != revision;
        }
        if changed {
            self.notify(false, true);
        }
    }

    /// Ждёт, пока индекс готов и событий индекса не ждёт отправки и не
    /// отправляется (тесты). Слушатель событий на стороне Rust в Tauri
    /// регистрируется через `try_lock` и теряется, если в этот момент
    /// другой поток отправляет событие (tauri 2.12, `event/listener.rs`):
    /// тесты регистрируют слушателей окна только после этого.
    #[cfg(test)]
    pub fn wait_settled(&self) {
        let start = Instant::now();
        loop {
            let ready = self.status().state == IndexState::Ready;
            let signal = self.signal();
            if ready && !signal.status && !signal.changed && !signal.sending {
                return;
            }
            drop(signal);
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "индекс не успокоился: {:?}",
                self.status()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Заметки, ссылающиеся на `note` (тесты).
    #[cfg(test)]
    pub fn backlinks_of(&self, list: &NoteList, note: &VaultPath) -> Vec<VaultPath> {
        self.read()
            .backlinks(note, list.links())
            .into_iter()
            .map(|source| source.from)
            .collect()
    }
}

/// Изменилось ли то, что показывают виды «Обратные ссылки» и «Теги»:
/// заметка появилась или исчезла (меняется и разрешение ссылок), её ссылки
/// (с положениями — строки панели) или теги.
fn affects_views(old: Option<&NoteEntry>, new: Option<&NoteEntry>) -> bool {
    match (old, new) {
        (None, None) => false,
        (Some(old), Some(new)) => {
            old.parsed.links != new.parsed.links || old.parsed.tags != new.parsed.tags
        }
        _ => true,
    }
}

/// Одно изменение индекса.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    /// Прочитать заметку с диска (нет её — убрать из индекса).
    Read(VaultPath),
    /// Убрать заметку.
    Remove(VaultPath),
    /// Заметку переименовали или перенесли: текст тот же, запись
    /// переносится без чтения файла.
    Move(VaultPath, VaultPath),
}

/// Что изменить в индексе после пачки изменений дерева: выводится по
/// списку заметок **до** каждого изменения ([`IndexUpdate::before`], как
/// события `note:*`), применяется вне замков ([`WindowIndex::update`]).
#[derive(Debug, Default)]
pub struct IndexUpdate {
    ops: Vec<Op>,
    rescan: bool,
}

impl IndexUpdate {
    /// Изменение `change`; `list` — список заметок до него.
    pub fn before(&mut self, list: &NoteList, change: &TreeChange) {
        match change {
            TreeChange::Created { path, .. } => {
                if is_note_path(path) {
                    self.ops.push(Op::Read(path.clone()));
                }
            }
            TreeChange::Modified { path } => {
                if let Some(path) = list.note(path) {
                    self.ops.push(Op::Read(path.clone()));
                }
            }
            TreeChange::Removed { path } => {
                self.ops
                    .extend(list.notes_under(path).into_iter().map(Op::Remove));
            }
            TreeChange::Renamed { from, to } => {
                let moved = list.notes_under(from);
                if moved.is_empty() {
                    // `from` неизвестен (своя команда уже применила
                    // изменение) — перечитать `to`, если это заметка.
                    if is_note_path(to) {
                        self.ops.push(Op::Read(to.clone()));
                    }
                }
                let from_len = from.components().len();
                for note in moved {
                    let new = VaultPath::from_components(
                        to.components()
                            .iter()
                            .chain(&note.components()[from_len..])
                            .cloned(),
                    );
                    if is_note_path(&new) {
                        self.ops.push(Op::Move(note, new));
                    } else {
                        // `а.md` → `а.txt`: заметка ушла из заметок.
                        self.ops.push(Op::Remove(note));
                    }
                }
            }
        }
    }

    /// Полная сверка наблюдателя (`rescan`): индекс сверяется с диском
    /// целиком.
    pub fn rescan(&mut self) {
        self.rescan = true;
    }
}

fn is_note_path(path: &VaultPath) -> bool {
    path.file_name().is_some_and(crate::names::is_note)
}

/// Запускает индекс хранилища окна: загрузка или сборка в фоновом потоке и
/// поток событий и записи `index.json`. События — в `emit` (только окну
/// этого хранилища). Оба потока выходят после [`WindowIndex::stop`].
pub fn start_index(open: &Arc<OpenVault>, emit: Arc<dyn Emit>) {
    let preparing = Arc::clone(open);
    let _ = std::thread::Builder::new()
        .name("rhizomind-index-build".into())
        .spawn(move || prepare_loop(&preparing));
    let running = Arc::clone(open);
    let _ = std::thread::Builder::new()
        .name("rhizomind-index-events".into())
        .spawn(move || events_loop(&running, &*emit));
}

/// Загрузка или сборка; хранилище недоступно — повтор через
/// [`PREPARE_RETRY`], пока окно открыто.
fn prepare_loop(open: &OpenVault) {
    let index = &open.index;
    while !index.stopped() {
        if index.prepare(&open.vault).is_ok() {
            return;
        }
        let signal = index.signal();
        if signal.stopped {
            return;
        }
        let _ = index.wake.wait_timeout(signal, PREPARE_RETRY);
    }
}

/// Отправка событий индекса не чаще [`limits::MAX_EVENTS_PER_SEC`] раз в
/// секунду (последнее состояние доходит всегда) и запись `index.json` по
/// сроку.
fn events_loop(open: &OpenVault, emit: &dyn Emit) {
    let index = &open.index;
    let started = Instant::now();
    let mut status_gate = Throttle::new(event_interval());
    let mut changed_gate = Throttle::new(event_interval());
    loop {
        let mut signal = index.signal();
        if signal.stopped {
            return;
        }
        let now = Instant::now();
        let send_status = signal.status && status_gate.try_fire(now);
        let send_changed = signal.changed && changed_gate.try_fire(now);
        signal.status &= !send_status;
        signal.changed &= !send_changed;
        if send_status || send_changed {
            signal.sending = true;
            drop(signal);
            if send_status {
                send(emit, event::INDEX_STATUS, &index.status());
            }
            if send_changed {
                send(emit, event::INDEX_CHANGED, &IndexChangedPayload {});
            }
            index.signal().sending = false;
            continue;
        }
        let wait = [
            signal.status.then(|| status_gate.wait(now)),
            signal.changed.then(|| changed_gate.wait(now)),
        ]
        .into_iter()
        .flatten()
        .fold(IDLE_WAKE, Duration::min);
        let (signal, _) = index
            .wake
            .wait_timeout(signal, wait)
            .unwrap_or_else(PoisonError::into_inner);
        let stopped = signal.stopped;
        drop(signal);
        if stopped {
            return;
        }
        // Первая запись — не раньше срока после открытия: индекс, только
        // что прочитанный или собранный, не пишется сразу.
        let now = Instant::now();
        if now.saturating_duration_since(started) >= store::WRITE_INTERVAL {
            index.write_if_due(&open.vault, now);
        }
    }
}

/// Не чаще раза в `interval`: событие, которое пришло раньше, ждёт срока
/// (вызывающий хранит признак «ждёт» и спрашивает снова).
#[derive(Debug)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    /// Не чаще раза в `interval`.
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    /// Можно ли отправить сейчас; да — время отправки запоминается.
    pub fn try_fire(&mut self, now: Instant) -> bool {
        let ready = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= self.interval);
        if ready {
            self.last = Some(now);
        }
        ready
    }

    /// Сколько ждать до следующей возможной отправки.
    fn wait(&self, now: Instant) -> Duration {
        self.last.map_or(Duration::ZERO, |last| {
            (last + self.interval).saturating_duration_since(now)
        })
    }
}

// --- Строки обратных ссылок ------------------------------------------------

/// Длина в единицах UTF-16.
fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// Строка панели из строки заметки `line` и ссылки в ней (`link` — байты
/// строки, на границах символов): строка целиком, если в ней не больше
/// [`limits::MAX_SNIPPET_CHARS`] символов; иначе — отрезок этой длины
/// вокруг ссылки, места поровну слева и справа (не хватает с одной стороны —
/// отдаётся другой). Ссылка длиннее предела — её начало. Возвращает текст
/// и выделение ссылки в нём, единиц UTF-16.
pub fn snippet(line: &str, link: Range<usize>) -> (String, u32, u32) {
    let max = limits::MAX_SNIPPET_CHARS;
    let (before, rest) = line.split_at(link.start);
    let (link_text, after) = rest.split_at(link.end - link.start);
    let before: Vec<char> = before.chars().collect();
    let link_chars: Vec<char> = link_text.chars().collect();
    let after: Vec<char> = after.chars().collect();
    let (left, shown_link, right) = if before.len() + link_chars.len() + after.len() <= max {
        (before.len(), link_chars.len(), after.len())
    } else if link_chars.len() >= max {
        (0, max, 0)
    } else {
        let room = max - link_chars.len();
        let mut left = room / 2;
        let mut right = room - left;
        if before.len() < left {
            right += left - before.len();
            left = before.len();
        }
        if after.len() < right {
            left = (left + right - after.len()).min(before.len());
            right = after.len();
        }
        (left, link_chars.len(), right)
    };
    let head: String = before[before.len() - left..].iter().collect();
    let body: String = link_chars[..shown_link].iter().collect();
    let tail: String = after[..right].iter().collect();
    let from = utf16_len(&head);
    let to = from.saturating_add(utf16_len(&body));
    (head + &body + &tail, from, to)
}

/// Строки панели по положениям ссылок в тексте заметки (`read_note`: без
/// BOM, переносы `\n`). Положения приходят из индекса — и из `index.json`,
/// который мог быть подменён или устарел (файл изменили, наблюдатель ещё не
/// сообщил): каждое берётся через `text.get(span)` (за концом текста и
/// внутри символа — `None`) и должно быть ссылкой `[[…]]` или `![[…]]` в
/// одной строке; иначе строка пропускается. Номер строки считается по
/// тексту, порядок — по тексту.
pub fn backlink_lines(text: &str, sites: &[LinkSite]) -> Vec<BacklinkLine> {
    let mut spans: Vec<Range<usize>> = sites
        .iter()
        .map(|site| site.span.clone())
        .filter(|span| {
            text.get(span.clone()).is_some_and(|link| {
                (link.starts_with("[[") || link.starts_with("![["))
                    && link.len() >= 4
                    && link.ends_with("]]")
                    && !link.contains('\n')
            })
        })
        .collect();
    spans.sort_by_key(|span| span.start);
    spans.dedup();
    let mut line_no: u32 = 1;
    let mut counted = 0;
    spans
        .into_iter()
        .map(|span| {
            let line_start = text[..span.start].rfind('\n').map_or(0, |i| i + 1);
            let line_end = text[span.end..]
                .find('\n')
                .map_or(text.len(), |i| span.end + i);
            let newlines = text[counted..line_start].matches('\n').count();
            line_no = line_no.saturating_add(u32::try_from(newlines).unwrap_or(u32::MAX));
            counted = line_start;
            let line = &text[line_start..line_end];
            let link = span.start - line_start..span.end - line_start;
            let column_utf16 = utf16_len(&line[..link.start]);
            let (snippet, from_utf16, to_utf16) = snippet(line, link);
            BacklinkLine {
                line: line_no,
                column_utf16,
                snippet,
                from_utf16,
                to_utf16,
            }
        })
        .collect()
}

/// Ответ `backlinks_list`: ссылающиеся заметки — под замками списка заметок
/// и индекса; строки — из файлов, **вне замков**; пределы ответа.
fn backlinks_reply(open: &OpenVault, path: &VaultPath) -> IndexReply<Backlinks> {
    let sources = {
        let list = open.read_notes();
        let index = open.index.read();
        let status = index.status();
        if status.state == IndexState::Building {
            return IndexReply::IndexBuilding(IndexProgress {
                done: status.done,
                total: status.total,
            });
        }
        // Путь в написании списка: ссылки разрешаются в него.
        match list.note(path) {
            Some(note) => index.backlinks(note, list.links()),
            None => Vec::new(),
        }
    };
    let mut sources: Vec<(String, BacklinkSource)> = sources
        .into_iter()
        .map(|source| {
            let name = source
                .from
                .file_name()
                .and_then(crate::names::note_stem)
                .unwrap_or_default()
                .to_owned();
            (name, source)
        })
        .collect();
    // Естественный порядок имён (как дерево файлов), затем путей.
    sources.sort_by_cached_key(|(name, source)| {
        (
            crate::names::sort_key(name),
            crate::names::KeyedPath::new(source.from.components()),
        )
    });

    // Пределы ответа — одна функция [`Backlinks::capped`]. Файлы читаются
    // только для того, что может попасть в ответ: заметки после первой
    // 1 000 — без чтения (только для числа и признака), строк у заметки —
    // на одну больше предела, всего — на одну больше общего предела (так
    // `capped` видит, что отброшено, и ставит `truncated`).
    let mut lines_left = limits::MAX_BACKLINK_LINES_TOTAL + 1;
    let notes = sources
        .into_iter()
        .enumerate()
        .map(|(i, (name, source))| {
            let total = source.sites.len();
            let want = if i < limits::MAX_BACKLINK_NOTES {
                total.min(limits::MAX_BACKLINK_LINES + 1).min(lines_left)
            } else {
                0
            };
            let links = if want == 0 {
                Vec::new()
            } else {
                // Заметку удалили или её не прочитать — имя без строк.
                let text = open
                    .vault
                    .read_note(&source.from)
                    .map(|read| read.text)
                    .unwrap_or_default();
                let mut lines = backlink_lines(&text, &source.sites);
                lines.truncate(want);
                lines
            };
            // В ответе у заметки останется не больше предела строк.
            lines_left -= links.len().min(limits::MAX_BACKLINK_LINES);
            BacklinkNote {
                from: source.from,
                name,
                links_total: u32::try_from(total).unwrap_or(u32::MAX),
                links,
            }
        })
        .collect();
    IndexReply::Ready(Backlinks::capped(notes))
}

/// Ответ `tags_list`.
fn tags_reply(open: &OpenVault) -> IndexReply<Tags> {
    let index = open.index.read();
    let status = index.status();
    if status.state == IndexState::Building {
        return IndexReply::IndexBuilding(IndexProgress {
            done: status.done,
            total: status.total,
        });
    }
    IndexReply::Ready(Tags {
        tags: index
            .tags()
            .into_iter()
            .map(|tag| TagSummary {
                tag: tag.display,
                key: tag.key,
                count: tag.notes,
            })
            .collect(),
    })
}

/// Ответ `tag_notes`.
fn tag_notes_reply(open: &OpenVault, key: &str) -> IndexReply<TagNotes> {
    let index = open.index.read();
    let status = index.status();
    if status.state == IndexState::Building {
        return IndexReply::IndexBuilding(IndexProgress {
            done: status.done,
            total: status.total,
        });
    }
    let mut notes = index.tag_notes(&crate::markdown::tag_key(key));
    let notes_total = u32::try_from(notes.len()).unwrap_or(u32::MAX);
    notes.truncate(limits::MAX_TAG_NOTES);
    IndexReply::Ready(TagNotes { notes_total, notes })
}

/// Состояние индекса хранилища окна.
#[tauri::command]
pub async fn index_status<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<IndexStatus> {
    let open = core.require_vault(window.label())?;
    Ok(open.index.status())
}

/// Обратные ссылки заметки. Только заметки хранилища окна: строки читаются
/// через `Vault::read_note` (контур хранилища) и отдаются как текст.
#[tauri::command]
pub async fn backlinks_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: BacklinksListRequest,
) -> CoreResult<IndexReply<Backlinks>> {
    checked_path(&request.path, "path")?;
    let open = core.require_vault(window.label())?;
    blocking(move || Ok(backlinks_reply(&open, &request.path))).await
}

/// Все теги хранилища.
#[tauri::command]
pub async fn tags_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<IndexReply<Tags>> {
    let open = core.require_vault(window.label())?;
    blocking(move || Ok(tags_reply(&open))).await
}

/// Заметки с тегом.
#[tauri::command]
pub async fn tag_notes<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: TagNotesRequest,
) -> CoreResult<IndexReply<TagNotes>> {
    limits::tag_key(&request.key)?;
    let open = core.require_vault(window.label())?;
    blocking(move || Ok(tag_notes_reply(&open, &request.key))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(name: &str, links: usize) -> BacklinkNote {
        BacklinkNote {
            from: VaultPath::from_components([format!("{name}.md")]),
            name: name.into(),
            links_total: links as u32,
            links: (0..links)
                .map(|i| BacklinkLine {
                    line: i as u32 + 1,
                    column_utf16: 0,
                    snippet: "[[Узел]]".into(),
                    from_utf16: 0,
                    to_utf16: 8,
                })
                .collect(),
        }
    }

    fn shown_lines(backlinks: &Backlinks) -> usize {
        backlinks.notes.iter().map(|n| n.links.len()).sum()
    }

    #[test]
    fn small_answer_is_not_truncated() {
        let backlinks = Backlinks::capped(vec![note("а", 3), note("б", 1)]);
        assert_eq!(backlinks.notes_total, 2);
        assert!(!backlinks.truncated);
        assert_eq!(shown_lines(&backlinks), 4);
    }

    #[test]
    fn lines_per_note_and_per_answer_are_capped_with_flag() {
        // 100 заметок по 100 ссылок = 10 000 строк > 5 000.
        let notes = (0..100).map(|i| note(&format!("з{i}"), 100)).collect();
        let backlinks = Backlinks::capped(notes);
        assert!(backlinks.truncated);
        assert_eq!(backlinks.notes_total, 100);
        assert_eq!(shown_lines(&backlinks), limits::MAX_BACKLINK_LINES_TOTAL);
        assert!(backlinks.notes.iter().all(|n| n.links_total == 100));

        let one = Backlinks::capped(vec![note("много", 150)]);
        assert!(one.truncated);
        assert_eq!(one.notes[0].links.len(), limits::MAX_BACKLINK_LINES);
        assert_eq!(one.notes[0].links_total, 150);
    }

    #[test]
    fn notes_are_capped_at_1000_with_flag() {
        let notes = (0..1_001).map(|i| note(&format!("з{i}"), 1)).collect();
        let backlinks = Backlinks::capped(notes);
        assert!(backlinks.truncated);
        assert_eq!(backlinks.notes_total, 1_001);
        assert_eq!(backlinks.notes.len(), limits::MAX_BACKLINK_NOTES);
    }

    // --- 0.2-1.4: строки обратных ссылок и частота событий ----------------

    use crate::index::LinkSite;
    use std::time::{Duration, Instant};

    fn site(span: std::ops::Range<usize>) -> LinkSite {
        LinkSite { line: 1, span }
    }

    fn utf16(text: &str, from: u32, to: u32) -> String {
        let units: Vec<u16> = text.encode_utf16().collect();
        String::from_utf16(&units[from as usize..to as usize]).expect("UTF-16")
    }

    #[test]
    fn snippet_keeps_short_line_whole() {
        let (text, from, to) = snippet("до [[Узел]] после", 5..17);
        assert_eq!(text, "до [[Узел]] после");
        assert_eq!(utf16(&text, from, to), "[[Узел]]");
    }

    #[test]
    fn snippet_cut_around_link_to_limit() {
        let line = format!("{}[[Узел]]{}", "а".repeat(300), "б".repeat(10));
        let start = "а".repeat(300).len();
        let (text, from, to) = snippet(&line, start..start + "[[Узел]]".len());
        assert_eq!(text.chars().count(), limits::MAX_SNIPPET_CHARS);
        assert_eq!(utf16(&text, from, to), "[[Узел]]");
        // Справа текста мало — остаток места отдан левой части.
        assert!(text.ends_with(&"б".repeat(10)));
    }

    #[test]
    fn lines_from_forged_positions_do_not_panic() {
        let text = "Привет [[Узел]]\nещё 😀 [[Узел]]";
        let real = text.find("[[").expect("ссылка");
        let second = text.rfind("[[").expect("ссылка");
        let sites = [
            site(1..3),                                  // внутри «П»
            site(text.len() - 1..text.len() + 10),       // за концом
            site(usize::MAX - 1..usize::MAX),            // огромное
            site(std::ops::Range { start: 10, end: 5 }), // конец раньше начала
            site(0..6),                                  // не ссылка
            site(real..real + "[[Узел]]".len()),         // настоящая
            site(second..second + "[[Узел]]".len()),     // настоящая, строка 2
            site(second + 1..second + "[[Узел]]".len()), // со сдвигом
        ];
        let lines = backlink_lines(text, &sites);
        let got: Vec<(u32, String, u32)> = lines
            .iter()
            .map(|l| {
                (
                    l.line,
                    utf16(&l.snippet, l.from_utf16, l.to_utf16),
                    l.column_utf16,
                )
            })
            .collect();
        assert_eq!(
            got,
            [(1, "[[Узел]]".to_owned(), 7), (2, "[[Узел]]".to_owned(), 7)]
        );
    }

    #[test]
    fn throttle_fires_at_most_four_times_per_second() {
        let mut gate = Throttle::new(Duration::from_millis(250));
        let start = Instant::now();
        let fired = (0..100)
            .filter(|i| gate.try_fire(start + Duration::from_millis(10 * i)))
            .count();
        // 0, 250, 500, 750 мс за первую секунду.
        assert_eq!(fired, 4);
    }
}
