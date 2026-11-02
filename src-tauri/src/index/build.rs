//! Сборка индекса с нуля и чтение одной заметки (С2-5.2, С2-3.7).
//!
//! Сборка блокирующая: окно (задача 0.2-1.4) запускает её в своём фоновом
//! потоке. Внутри — рабочие потоки, не больше числа ядер: каждый берёт
//! следующую заметку списка, читает её через [`Vault::read_note`] (контур
//! хранилища, как у редактора) и разбирает — **без замка индекса**; замок на
//! запись берётся только на вставку пачки из [`BATCH`] заметок. Ход сборки
//! сообщается после каждой пачки, тоже вне замка; как часто показывать его
//! окну, решает вызывающий. Сети и записи на диск здесь нет.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{PoisonError, RwLock};

use super::{NoteEntry, VaultIndex};
use crate::commands::{CoreResult, IndexStatus, VaultPath};
use crate::markdown;
use crate::vault::{NoteRef, Vault};

/// Заметок в пачке: сколько рабочий поток читает до вставки под замком.
const BATCH: usize = 64;

/// Читает и разбирает заметку — вне замка индекса (сборка, своя запись,
/// пачка наблюдателя). Положения в записи — в тексте, каким его отдаёт
/// [`Vault::read_note`]: без BOM, переносы `\n`. Заметка больше предела
/// чтения ядра не читается целиком — запись без заголовков, тегов и ссылок.
/// Ошибки — как у `read_note` (`not_found`, контур и т.п.).
pub fn read_entry(vault: &Vault, note: &NoteRef) -> CoreResult<NoteEntry> {
    let read = vault.read_note(&note.path)?;
    Ok(NoteEntry {
        size: read.version.size,
        modified_ms: read.version.modified_ms,
        created_ms: note.created_ms,
        parsed: markdown::parse(&read.text),
    })
}

/// Собирает индекс хранилища с нуля: обходит папку, читает и разбирает все
/// заметки параллельно и вставляет их в `index` пачками; в конце убирает
/// записи заметок, которых больше нет, и объявляет индекс готовым.
///
/// - Пока идёт сборка, `index` можно читать и менять (`upsert`/`remove`
///   наблюдателя): изменённое снаружи сборка не перекрывает.
/// - Новая сборка того же индекса останавливает старую: та дочитывает
///   текущие заметки и выходит без ошибки.
/// - `on_progress` вызывается вне замка: «строится» с ходом
///   `{done, total}` (сначала `total = 0` — идёт обход папки) и последним —
///   «готов».
/// - Хранилище недоступно (обход папки не удался) — ошибка, индекс
///   остаётся «строится».
pub fn rebuild(
    vault: &Vault,
    index: &RwLock<VaultIndex>,
    on_progress: &(dyn Fn(IndexStatus) + Sync),
) -> CoreResult<()> {
    let write = || index.write().unwrap_or_else(PoisonError::into_inner);
    let (id, status) = {
        let mut index = write();
        (index.begin_build(), index.status())
    };
    on_progress(status);

    let notes = vault.notes()?;
    let status = {
        let mut index = write();
        if !index.set_total(id, notes.len()) {
            return Ok(());
        }
        index.status()
    };
    on_progress(status);

    let next = AtomicUsize::new(0);
    let stopped = AtomicBool::new(false);
    let worker = || {
        let mut batch: Vec<(VaultPath, Option<NoteEntry>)> = Vec::with_capacity(BATCH);
        loop {
            let note = notes.get(next.fetch_add(1, Ordering::Relaxed));
            if let Some(note) = note {
                batch.push((note.path.clone(), read_entry(vault, note).ok()));
            }
            let flush = batch.len() == BATCH || (note.is_none() && !batch.is_empty());
            if flush {
                let status = {
                    let mut index = write();
                    let current = index.insert_built(id, std::mem::take(&mut batch));
                    current.then(|| index.status())
                };
                match status {
                    Some(status) => on_progress(status),
                    None => stopped.store(true, Ordering::Relaxed),
                }
            }
            if note.is_none() || stopped.load(Ordering::Relaxed) {
                return;
            }
        }
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(notes.len())
        .max(1);
    std::thread::scope(|scope| {
        // Нехватка ресурсов ОС на поток — не паника: работает меньше потоков
        // (как минимум текущий).
        for _ in 1..threads {
            let _ = std::thread::Builder::new()
                .name("index-build".into())
                .spawn_scoped(scope, worker);
        }
        worker();
    });
    if stopped.load(Ordering::Relaxed) {
        return Ok(());
    }

    let listed: HashSet<VaultPath> = notes.into_iter().map(|n| n.path).collect();
    let status = {
        let mut index = write();
        if !index.finish_build(id, &listed) {
            return Ok(());
        }
        index.status()
    };
    on_progress(status);
    Ok(())
}
