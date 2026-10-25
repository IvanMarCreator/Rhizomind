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
//!   `rescan: true` (даже без изменений: интерфейс перечитывает дерево).
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
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer_opt};

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

/// Получатель пачек изменений. Вызывается из потока наблюдателя.
pub type ChangeSink = Box<dyn Fn(VaultChangedPayload) + Send + Sync + 'static>;

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
    debouncer: Option<Debouncer<notify::RecommendedWatcher, RecommendedCache>>,
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
            RecommendedCache::new(),
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
            .spawn(move || run(model, journal, inbox, sink))
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

/// Поток сверки: владеет моделью, сверяет пачки и отдаёт изменения.
fn run(mut model: TreeModel, own: OwnWrites, inbox: Receiver<Message>, sink: ChangeSink) {
    while let Ok(message) = inbox.recv() {
        let mut batch = match message {
            Message::Stop => return,
            Message::Rescan => Batch {
                paths: Vec::new(),
                rescan: true,
            },
            Message::Events(batch) => batch,
        };
        // Пачки, идущие подряд, — одной сверкой: склейка выдаёт события по
        // каждому пути отдельно, и при переносе многих файлов в новую папку
        // папка приходит раньше, чем исчезновения старых путей. В одной
        // сверке это переименования, а не «появилось» и «исчезло».
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
        let changes = if batch.rescan {
            model.rescan(&own)
        } else {
            model.reconcile(batch.paths, &own)
        };
        if batch.rescan || !changes.is_empty() {
            sink(VaultChangedPayload {
                rescan: batch.rescan,
                changes,
            });
        }
    }
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
