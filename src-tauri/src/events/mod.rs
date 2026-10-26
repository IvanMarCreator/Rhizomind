//! События ядра → интерфейс: отправка окну своего хранилища (задача 1.7).
//!
//! Имена и данные событий — контракт API v1 (`commands::events`, снимки
//! `contracts/v1/event.*.snap`). Здесь — кто, кому и из чего их делает:
//! - **кому:** только окну, в котором открыто хранилище, — `emit_to` с
//!   целью `EventTarget::WebviewWindow { label }` ([`WindowEmitter`]); окна
//!   других хранилищ события не получают (тест
//!   `commands::core_tests::events_reach_only_the_window_of_their_vault`);
//! - **из чего:** из сообщений наблюдателя за папкой ([`VaultEvents`]):
//!   пачка изменений дерева → `vault:changed` и выведенные из неё `note:*`
//!   (`note:changed` для изменённой заметки, `note:renamed` и
//!   `note:deleted` для каждой заметки из списка заметок по пути и под ним),
//!   пропажа и возвращение папки → `vault:availability`;
//! - `note:conflict-saved` отправляет команда `note_write` (конфликт
//!   изменений решён при записи, а не по событию наблюдателя).
//!
//! Сбой отправки окну не роняет поток наблюдателя; если упадёт сам
//! получатель, наблюдатель сообщит `DeliveryFailed` и через пару секунд
//! сам пройдёт полную сверку (`watcher::DELIVERY_RETRY`).
//!
//! В событиях нет текста заметок: только пути внутри хранилища и версии
//! файлов (инвариант 3).

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, EventTarget, Runtime};

use crate::commands::{
    NoteChangedPayload, NoteDeletedPayload, NoteRenamedPayload, OpenVault, TreeChange,
    VaultAvailabilityPayload, VaultChangedPayload, VaultPath, event,
};
use crate::watcher::{ChangeSink, WatcherEvent};

/// Куда уходят события одного хранилища. Подменяется в тестах.
pub trait Emit: Send + Sync + 'static {
    /// Отправляет событие `event` с данными `payload` окну хранилища.
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Отправка событий одному окну приложения по его метке.
pub struct WindowEmitter<R: Runtime> {
    app: AppHandle<R>,
    label: String,
}

impl<R: Runtime> WindowEmitter<R> {
    /// Отправитель для окна с меткой `label`.
    pub fn new(app: AppHandle<R>, label: impl Into<String>) -> Self {
        Self {
            app,
            label: label.into(),
        }
    }
}

impl<R: Runtime> Emit for WindowEmitter<R> {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        // Окно могло закрыться — событие никому не нужно.
        let _ = self.app.emit_to(
            EventTarget::webview_window(self.label.clone()),
            event,
            payload,
        );
    }
}

/// Отправляет событие с данными контракта.
pub fn send<P: Serialize>(emit: &dyn Emit, event: &str, payload: &P) {
    if let Ok(payload) = serde_json::to_value(payload) {
        emit.emit(event, payload);
    }
}

/// Перевод сообщений наблюдателя одного хранилища в события окна и
/// обновление списка заметок.
pub struct VaultEvents {
    open: Arc<OpenVault>,
    emit: Arc<dyn Emit>,
}

impl VaultEvents {
    /// События хранилища `open` — в `emit`.
    pub fn new(open: Arc<OpenVault>, emit: Arc<dyn Emit>) -> Self {
        Self { open, emit }
    }

    /// Получатель для `VaultWatcher::start_with`.
    pub fn into_sink(self) -> ChangeSink {
        Box::new(move |message| self.handle(message))
    }

    /// Обрабатывает одно сообщение наблюдателя.
    pub fn handle(&self, message: WatcherEvent) {
        match message {
            WatcherEvent::Changed(payload) => self.changed(payload),
            WatcherEvent::Unavailable => self.availability(false),
            WatcherEvent::Available => self.availability(true),
            // Получатель упал на прошлом сообщении: наблюдатель сам пройдёт
            // полную сверку по таймеру, интерфейс получит `rescan: true`.
            WatcherEvent::DeliveryFailed => {}
        }
    }

    fn availability(&self, available: bool) {
        send(
            &*self.emit,
            event::VAULT_AVAILABILITY,
            &VaultAvailabilityPayload { available },
        );
    }

    fn changed(&self, payload: VaultChangedPayload) {
        let vault = &self.open.vault;
        // Всё, что читается с диска, — до замка списка заметок: дата
        // создания новых заметок и (после полной сверки) полный список.
        let created: std::collections::HashMap<VaultPath, i64> = payload
            .changes
            .iter()
            .filter_map(|change| match change {
                TreeChange::Created { path, .. } | TreeChange::Renamed { to: path, .. } => {
                    vault.note_created_ms(path).map(|ms| (path.clone(), ms))
                }
                _ => None,
            })
            .collect();
        let created_ms = |path: &VaultPath| created.get(path).copied();
        let all_notes = if payload.rescan {
            vault.notes().ok()
        } else {
            None
        };
        // `note:*` выводятся по списку заметок до каждого изменения: какие
        // заметки лежали по удалённому или переименованному пути.
        let mut notes: Vec<(&'static str, serde_json::Value)> = Vec::new();
        let mut modified: Vec<VaultPath> = Vec::new();
        {
            let mut index = self.open.write_index();
            index.apply_observed(
                &payload.changes,
                &created_ms,
                &mut |index, change| match change {
                    TreeChange::Modified { path } if index.contains(path) => {
                        modified.push(path.clone());
                    }
                    TreeChange::Removed { path } => {
                        for note in index.notes_under(path) {
                            push(
                                &mut notes,
                                event::NOTE_DELETED,
                                &NoteDeletedPayload { path: note },
                            );
                        }
                    }
                    TreeChange::Renamed { from, to } => {
                        let from_len = from.components().len();
                        for note in index.notes_under(from) {
                            let moved = VaultPath::from_components(
                                to.components()
                                    .iter()
                                    .chain(&note.components()[from_len..])
                                    .cloned(),
                            );
                            push(
                                &mut notes,
                                event::NOTE_RENAMED,
                                &NoteRenamedPayload {
                                    from: note,
                                    to: moved,
                                },
                            );
                        }
                    }
                    _ => {}
                },
            );
            if let Some(all) = all_notes {
                index.replace(all);
            }
        }
        // Версии изменённых заметок — с диска, вне замка списка заметок.
        for path in modified {
            if let Ok(version) = vault.file_version(&path) {
                push(
                    &mut notes,
                    event::NOTE_CHANGED,
                    &NoteChangedPayload { path, version },
                );
            }
        }
        send(&*self.emit, event::VAULT_CHANGED, &payload);
        for (name, payload) in notes {
            self.emit.emit(name, payload);
        }
    }
}

fn push<P: Serialize>(
    out: &mut Vec<(&'static str, serde_json::Value)>,
    name: &'static str,
    payload: &P,
) {
    if let Ok(value) = serde_json::to_value(payload) {
        out.push((name, value));
    }
}

#[cfg(test)]
mod tests;
