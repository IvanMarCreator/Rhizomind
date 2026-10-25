//! Свои записи ядра для наблюдателя за папкой (ADR 0006, уточнение
//! 25.10.2026): хэш и размер новой версии регистрируются **до** замены
//! файла, чтобы событие о замене не стало внешним изменением. Дату изменения
//! до замены не узнать, поэтому сверка — по пути, хэшу и размеру.
//!
//! Наблюдатель (задача 1.4) получает записи двумя путями: через
//! получателя ([`OwnWrites::set_hook`], например `VaultWatcher::remember_own_write`
//! после склейки) или запросом [`OwnWrites::contains`] по событию.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use super::{OwnWrite, OwnWriteHook};
use crate::commands::VaultPath;

/// Сколько помнить свою запись: события ОС приходят за доли секунды, склейка
/// — около секунды; запас — на нагруженный диск.
const KEEP: Duration = Duration::from_secs(120);
/// Не больше стольких записей в журнале.
const CAPACITY: usize = 1024;

/// Журнал своих записей и их получатель.
#[derive(Default)]
pub struct OwnWrites {
    hook: RwLock<Option<OwnWriteHook>>,
    log: Mutex<VecDeque<(OwnWrite, Instant)>>,
}

impl std::fmt::Debug for OwnWrites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnWrites").finish_non_exhaustive()
    }
}

impl OwnWrites {
    /// Получатель своих записей; заменяет прежнего.
    pub fn set_hook(&self, hook: OwnWriteHook) {
        *self.hook.write().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Регистрирует запись — вызывается до замены файла на диске.
    pub fn register(&self, path: &VaultPath, hash: &str, size: u64) {
        let write = OwnWrite {
            path: path.clone(),
            hash: hash.to_owned(),
            size,
        };
        {
            let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            while log
                .front()
                .is_some_and(|(_, at)| now.duration_since(*at) > KEEP || log.len() >= CAPACITY)
            {
                log.pop_front();
            }
            log.push_back((write.clone(), now));
        }
        if let Some(hook) = self
            .hook
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            hook(&write);
        }
    }

    /// Была ли недавно своя запись с таким путём, хэшем и размером.
    pub fn contains(&self, path: &VaultPath, hash: &str, size: u64) -> bool {
        let now = Instant::now();
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|(w, at)| {
                now.duration_since(*at) <= KEEP
                    && w.size == size
                    && w.hash == hash
                    && &w.path == path
            })
    }
}
