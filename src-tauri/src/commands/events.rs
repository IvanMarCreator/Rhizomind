//! События ядра → интерфейс (спецификация 0.1, С5.5).
//!
//! Кому: события отправляются **только окну своего хранилища**
//! (`Emitter::emit_to` по метке окна), не всем окнам; интерфейс слушает
//! события своего окна (`getCurrentWebviewWindow().listen`,
//! `src/api/events.ts`). Отправку и тест «чужое окно не получает» делает
//! задача 1.7 (`src-tauri/src/events/`); здесь — имена и данные.
//!
//! События `note:*` отправляются для **любой** заметки хранилища, а не
//! только для открытых: какие заметки открыты, знает интерфейс, он и
//! фильтрует.
//!
//! `note:renamed` при несохранённом вводе: интерфейс переключает путь
//! буфера на новый и пишет туда (`note_write`) с прежней ожидаемой версией.
//! `note:deleted` при несохранённом вводе: `note_write` по прежнему пути
//! вернёт `not_found`; интерфейс держит текст в памяти. Окончательное
//! поведение — решение владельца, ещё не принято.

use serde::{Deserialize, Serialize};

use super::types::{EntryKind, FileVersion, VaultPath};

/// Имена событий.
pub mod event {
    /// Изменилось дерево файлов хранилища.
    pub const VAULT_CHANGED: &str = "vault:changed";
    /// Открытая заметка изменилась снаружи.
    pub const NOTE_CHANGED: &str = "note:changed";
    /// Заметка переименована или перемещена снаружи.
    pub const NOTE_RENAMED: &str = "note:renamed";
    /// Заметка удалена снаружи.
    pub const NOTE_DELETED: &str = "note:deleted";
    /// При записи сохранены обе версии заметки (конфликт изменений).
    pub const NOTE_CONFLICT_SAVED: &str = "note:conflict-saved";
}

/// Все события API v1 — для тестов снимков.
pub const EVENT_NAMES: [&str; 5] = [
    event::VAULT_CHANGED,
    event::NOTE_CHANGED,
    event::NOTE_RENAMED,
    event::NOTE_DELETED,
    event::NOTE_CONFLICT_SAVED,
];

/// Одно изменение дерева.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TreeChange {
    /// Появился файл или папка.
    Created {
        /// Путь.
        path: VaultPath,
        /// Вид элемента.
        entry_kind: EntryKind,
    },
    /// Изменилось содержимое файла.
    Modified {
        /// Путь.
        path: VaultPath,
    },
    /// Файл или папка исчезли.
    Removed {
        /// Путь.
        path: VaultPath,
    },
    /// Переименование или перемещение.
    Renamed {
        /// Прежний путь.
        from: VaultPath,
        /// Новый путь.
        to: VaultPath,
    },
}

/// `vault:changed`: пачка изменений дерева после склейки событий.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultChangedPayload {
    /// `true` — была полная сверка с диском (переполнение буфера ОС,
    /// массовые операции): интерфейс перечитывает дерево `tree_list`, а
    /// `changes` может быть пустым.
    pub rescan: bool,
    /// Изменения по порядку.
    pub changes: Vec<TreeChange>,
}

/// `note:changed`: заметка изменилась снаружи; интерфейс перечитывает её
/// `note_read`, если у него нет несохранённых правок.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteChangedPayload {
    /// Путь заметки.
    pub path: VaultPath,
    /// Новая версия файла на диске.
    pub version: FileVersion,
}

/// `note:renamed`: заметка переименована или перемещена снаружи.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteRenamedPayload {
    /// Прежний путь.
    pub from: VaultPath,
    /// Новый путь.
    pub to: VaultPath,
}

/// `note:deleted`: заметка удалена снаружи; интерфейс закрывает её с уведомлением.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDeletedPayload {
    /// Путь удалённой заметки.
    pub path: VaultPath,
}

/// `note:conflict-saved`: сохранены обе версии заметки — текст из редактора
/// в заметке, версия с диска рядом копией (С3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteConflictSavedPayload {
    /// Заметка с текстом из редактора.
    pub path: VaultPath,
    /// Копия с версией с диска: «Имя (внешняя версия ГГГГ-ММ-ДД ЧЧ-ММ).md».
    pub copy_path: VaultPath,
}
