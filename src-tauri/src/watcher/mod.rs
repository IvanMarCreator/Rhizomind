//! Слежение за папкой хранилища (задача 1.4, ADR 0006: `notify`
//! 9.0.0-rc.6 + `notify-debouncer-full` 0.8.0-rc.2).
//!
//! Один наблюдатель на хранилище, рекурсивно на корень. Наружу выходят уже
//! склеенные пачки изменений дерева [`VaultChangedPayload`] — данные события
//! `vault:changed`; отправку окну делает слой команд (задача 1.7).
//!
//! Как устроено:
//! - при запуске наблюдатель обходит хранилище и строит модель «дерево
//!   хранилища» (`tree.rs`) — сверка при открытии;
//! - события ОС склеивает `notify-debouncer-full`; пути внутри `.rhizomind/`
//!   и с точкой в начале имени отбрасываются в своём обработчике (`notify`
//!   исключений не поддерживает);
//! - пути из событий сверяются с диском, и модель становится равной диску;
//!   разница — это изменения пачки. Переименования распознаются по
//!   идентификатору файла (`file-id`), запасной путь — размер и дата;
//! - свои записи ядра не внешние: журнал [`OwnWrites`] «путь → (хэш,
//!   размер)», куда хранилище пишет до замены файла;
//! - переполнение буфера ОС (событие Rescan), ошибка наблюдателя или вызов
//!   [`VaultWatcher::rescan`] — полная сверка с диском и пачка с
//!   `rescan: true` (даже без изменений: интерфейс перечитывает дерево);
//! - пропавший или недоступный корень — не «удалено всё»: изменений нет,
//!   сигнал [`WatcherEvent::Unavailable`], после возвращения —
//!   [`WatcherEvent::Available`] и полная сверка; папка, которая не
//!   прочиталась, остаётся в модели и сверяется снова через пару секунд;
//! - паника получателя не останавливает поток: сигнал
//!   [`WatcherEvent::DeliveryFailed`], следующая пачка — полная сверка.
//!
//! Кэш идентификаторов склейки (`FileIdMap`) не используется: он обходит
//! дерево по ссылкам и вышел бы через точку соединения за контур.
//!
//! Сетевого кода нет: только уведомления файловой системы ОС
//! (`ReadDirectoryChangesW`, inotify, FSEvents) и чтение файлов хранилища.
//! Запасной путь ADR 0006 (свой наблюдатель на `ReadDirectoryChangesW`)
//! меняет только реализацию этого модуля.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{EventKindMask, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};

mod own_writes;
mod tree;

#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tree_tests;
#[cfg(test)]
mod watcher_tests;

pub use own_writes::OwnWrites;

use crate::commands::{
    CoreError, CoreResult, ErrorCode, FileVersion, VaultChangedPayload, VaultPath,
};
use tree::TreeModel;

/// Что наблюдатель сообщает ядру.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherEvent {
    /// Пачка изменений дерева — данные события `vault:changed`.
    Changed(VaultChangedPayload),
    /// Корня хранилища нет или он недоступен.
    Unavailable,
    /// Корень снова доступен.
    Available,
    /// Получатель упал на предыдущем сообщении.
    DeliveryFailed,
}

/// Получатель сообщений наблюдателя. Вызывается из потока наблюдателя.
pub type ChangeSink = Box<dyn Fn(WatcherEvent) + Send + Sync + 'static>;

/// Сколько события ОС по одному пути склеиваются, прежде чем уйти в
/// сверку. Вместе со сверкой укладывается в «дерево в течение секунды»
/// (С3.6).
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Пачки склейки, между которыми меньше этого, сверяются вместе…
const GATHER_QUIET: Duration = Duration::from_millis(150);

/// …но ожидание следующих не дольше этого: дерево — в течение секунды.
const GATHER_MAX: Duration = Duration::from_millis(500);

/// Сообщение потоку сверки.
enum Message {
    /// Склеенные события ОС: пути для сверки или потребность в полной.
    Events(Batch),
    /// Полная сверка по запросу ядра.
    Rescan,
    /// Остановка.
    Stop,
}

/// Пачка событий ОС после склейки и фильтра.
#[derive(Default)]
struct Batch {
    paths: Vec<VaultPath>,
    rescan: bool,
}

/// Наблюдатель за одним хранилищем. Слежение останавливается при удалении
/// значения (`Drop`): после возврата из `drop` получатель больше не
/// вызывается.
pub struct VaultWatcher {
    own_writes: OwnWrites,
    messages: Sender<Message>,
    worker: Option<JoinHandle<()>>,
    debouncer: Option<Debouncer<notify::RecommendedWatcher, NoCache>>,
    #[cfg(test)]
    lose_os_events: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for VaultWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultWatcher").finish_non_exhaustive()
    }
}

impl VaultWatcher {
    /// Запускает слежение за корнем хранилища и сверку с диском при открытии
    /// со своим журналом своих записей ([`VaultWatcher::own_writes`]).
    pub fn start(root: &Path, sink: ChangeSink) -> CoreResult<Self> {
        Self::start_with(root, OwnWrites::new(), sink)
    }

    /// То же, с общим журналом своих записей: хранилище может получить
    /// журнал раньше, чем запущен наблюдатель.
    pub fn start_with(root: &Path, own_writes: OwnWrites, sink: ChangeSink) -> CoreResult<Self> {
        if !root.is_dir() {
            return Err(CoreError::new(ErrorCode::VaultUnavailable));
        }
        let (messages, inbox) = mpsc::channel();
        #[cfg(test)]
        let lose_os_events = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Сначала слежение, потом обход: изменения во время обхода придут
        // событиями и сверятся после него.
        let paths = EventPaths::new(root);
        let to_worker = messages.clone();
        #[cfg(test)]
        let lose = lose_os_events.clone();
        let config = notify::Config::default()
            // Обращения к файлам (открытие, чтение) — не изменения.
            .with_event_kinds(EventKindMask::CORE)
            // Слежение не выходит по символическим ссылкам за хранилище.
            .with_follow_symlinks(false);
        let mut debouncer = new_debouncer_opt::<_, notify::RecommendedWatcher, _>(
            DEBOUNCE,
            None,
            move |result: DebounceEventResult| {
                #[cfg(test)]
                if lose.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let batch = paths.batch(result);
                if batch.rescan || !batch.paths.is_empty() {
                    // Поток сверки остановлен — пачка никому не нужна.
                    let _ = to_worker.send(Message::Events(batch));
                }
            },
            // Без кэша идентификаторов: `FileIdMap` склейки обходит дерево по
            // ссылкам (`follow_links(true)`) и открывает каждый файл — через
            // точку соединения внутри хранилища он ушёл бы за его пределы.
            // Переносы распознаёт своя модель по `file-id`.
            NoCache,
            config,
        )
        .map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
        debouncer
            .watch(root, RecursiveMode::Recursive)
            .map_err(|_| CoreError::new(ErrorCode::IoFailed))?;

        let model =
            TreeModel::scan(root).map_err(|_| CoreError::new(ErrorCode::VaultUnavailable))?;
        let journal = own_writes.clone();
        let worker = std::thread::Builder::new()
            .name("rhizomind-watcher".into())
            .spawn(move || {
                Worker {
                    model,
                    own: journal,
                    sink,
                    unavailable: false,
                    retry: Vec::new(),
                    retry_at: None,
                    force_rescan: false,
                }
                .run(inbox);
            })
            .map_err(|_| CoreError::new(ErrorCode::IoFailed))?;

        Ok(Self {
            own_writes,
            messages,
            worker: Some(worker),
            debouncer: Some(debouncer),
            #[cfg(test)]
            lose_os_events,
        })
    }

    /// Журнал своих записей этого наблюдателя — для хранилища.
    pub fn own_writes(&self) -> OwnWrites {
        self.own_writes.clone()
    }

    /// Запоминает версию файла своей записи (хэш и размер; дата не нужна):
    /// событие о файле с этим содержимым не будет внешним изменением.
    /// Вызывается до замены файла.
    pub fn remember_own_write(&self, path: &VaultPath, version: &FileVersion) {
        self.own_writes.record(path, &version.hash, version.size);
    }

    /// Полная сверка с диском: пачка с `rescan: true`.
    pub fn rescan(&self) {
        let _ = self.messages.send(Message::Rescan);
    }

    /// Тестовый шов: события ОС теряются, как при переполнении буфера.
    #[cfg(test)]
    pub(crate) fn lose_os_events(&self, lose: bool) {
        self.lose_os_events
            .store(lose, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for VaultWatcher {
    fn drop(&mut self) {
        if let Some(debouncer) = self.debouncer.take() {
            debouncer.stop();
        }
        let _ = self.messages.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Через сколько повторить сверку путей, которые не прочитались.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// Как часто проверять, вернулся ли недоступный корень.
const UNAVAILABLE_POLL: Duration = Duration::from_secs(1);

/// Поток сверки: владеет моделью, сверяет пачки и отдаёт изменения.
struct Worker {
    model: TreeModel,
    own: OwnWrites,
    sink: ChangeSink,
    /// Корень недоступен: изменений нет, ждём его возвращения.
    unavailable: bool,
    /// Пути, которые не прочитались, и когда их сверить снова.
    retry: Vec<VaultPath>,
    retry_at: Option<Instant>,
    /// Получатель упал — следующая пачка будет полной сверкой.
    force_rescan: bool,
}

impl Worker {
    fn run(mut self, inbox: Receiver<Message>) {
        loop {
            let first = match self.wait(&inbox) {
                Waited::Message(message) => message,
                Waited::Timer => {
                    self.on_timer();
                    continue;
                }
                Waited::Stop => return,
            };
            let mut batch = match first {
                Message::Stop => return,
                Message::Rescan => Batch {
                    paths: Vec::new(),
                    rescan: true,
                },
                Message::Events(batch) => batch,
            };
            // Пачки, идущие подряд, — одной сверкой: склейка выдаёт события
            // по каждому пути отдельно, и при переносе многих файлов в новую
            // папку папка приходит раньше, чем исчезновения старых путей. В
            // одной сверке это переименования, а не «появилось» и «исчезло».
            let started = Instant::now();
            while started.elapsed() < GATHER_MAX {
                match inbox.recv_timeout(GATHER_QUIET) {
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Ok(Message::Rescan) => batch.rescan = true,
                    Ok(Message::Events(more)) => {
                        batch.rescan |= more.rescan;
                        batch.paths.extend(more.paths);
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            self.process(batch);
        }
    }

    /// Ждёт сообщения или срока повтора / проверки корня.
    fn wait(&self, inbox: &Receiver<Message>) -> Waited {
        let deadline = if self.unavailable {
            Some(Instant::now() + UNAVAILABLE_POLL)
        } else {
            self.retry_at
        };
        let result = match deadline {
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
        };
        match result {
            Ok(message) => Waited::Message(message),
            Err(RecvTimeoutError::Timeout) => Waited::Timer,
            Err(RecvTimeoutError::Disconnected) => Waited::Stop,
        }
    }

    fn on_timer(&mut self) {
        if self.unavailable {
            // Корень вернулся — полная сверка: пока его не было, события
            // не приходили.
            self.process(Batch {
                paths: Vec::new(),
                rescan: true,
            });
        } else if self.retry_at.is_some_and(|at| at <= Instant::now()) {
            let paths = std::mem::take(&mut self.retry);
            self.retry_at = None;
            self.process(Batch {
                paths,
                rescan: false,
            });
        }
    }

    fn process(&mut self, mut batch: Batch) {
        batch.rescan |= self.force_rescan;
        let result = if batch.rescan || self.unavailable {
            self.model.rescan(&self.own)
        } else {
            self.model.reconcile(batch.paths, &self.own)
        };
        if result.unavailable {
            if !self.unavailable {
                self.unavailable = true;
                self.deliver(WatcherEvent::Unavailable);
            }
            return;
        }
        let rescan = batch.rescan || self.unavailable;
        if self.unavailable {
            self.unavailable = false;
            self.deliver(WatcherEvent::Available);
        }
        if !result.retry.is_empty() {
            self.retry.extend(result.retry);
            self.retry_at
                .get_or_insert_with(|| Instant::now() + RETRY_AFTER);
        }
        if rescan || !result.changes.is_empty() {
            let delivered = self.deliver(WatcherEvent::Changed(VaultChangedPayload {
                rescan,
                changes: result.changes,
            }));
            self.force_rescan = !delivered;
        }
    }

    /// Отдаёт сообщение получателю. Паника получателя не останавливает
    /// поток: получатель узнаёт о сбое ([`WatcherEvent::DeliveryFailed`]),
    /// а следующая пачка будет полной сверкой.
    fn deliver(&mut self, event: WatcherEvent) -> bool {
        let sink = &self.sink;
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event))).is_ok() {
            return true;
        }
        self.force_rescan = true;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sink(WatcherEvent::DeliveryFailed)
        }));
        false
    }
}

enum Waited {
    Message(Message),
    Timer,
    Stop,
}

/// Перевод путей событий ОС в пути хранилища и фильтр служебных.
struct EventPaths {
    /// Корень, как его передали наблюдателю (так он приходит в событиях),
    /// и канонический (FSEvents на macOS отдаёт `/private/var/…`).
    roots: Vec<PathBuf>,
}

impl EventPaths {
    fn new(root: &Path) -> Self {
        let mut roots = vec![root.to_path_buf()];
        if let Ok(canonical) = root.canonicalize()
            && canonical != root
        {
            roots.push(canonical);
        }
        Self { roots }
    }

    fn batch(&self, result: DebounceEventResult) -> Batch {
        let mut batch = Batch::default();
        match result {
            // Ошибка наблюдателя: часть событий могла потеряться.
            Err(_) => batch.rescan = true,
            Ok(events) => {
                for event in events {
                    if event.need_rescan() {
                        batch.rescan = true;
                    }
                    batch
                        .paths
                        .extend(event.paths.iter().filter_map(|p| self.vault_path(p)));
                }
            }
        }
        batch
    }

    /// Путь хранилища или `None`: вне корня, имя не UTF-8, служебная папка
    /// `.rhizomind/` или другое имя с точки в начале. Событие о самом корне
    /// — полная сверка (пустой путь).
    fn vault_path(&self, path: &Path) -> Option<VaultPath> {
        let relative = self
            .roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())?;
        let mut components = Vec::new();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return None;
            };
            let name = name.to_str()?;
            if name.starts_with('.') {
                return None;
            }
            components.push(name.to_owned());
        }
        Some(VaultPath::from_components(components))
    }
}
