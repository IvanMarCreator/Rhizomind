//! События ядра → интерфейс (спецификация 0.1, С5.5).
//!
//! Кому: события отправляются **только окну своего хранилища**
//! (`Emitter::emit_to` с целью `EventTarget::WebviewWindow` по метке окна),
//! не всем окнам; интерфейс слушает события своего окна
//! (`getCurrentWebviewWindow().listen`, `src/api/events.ts`). Отправка —
//! модуль `src-tauri/src/events/` (задача 1.7); здесь — имена и данные.
//!
//! **События идемпотентны** (ревью 1.4): после полной сверки и при гонке
//! с собственными командами окна одно и то же изменение может прийти
//! дважды — в ответе команды и событием. Интерфейс применяет `vault:changed`
//! так: `created` уже известного элемента — не ошибка (элемент остаётся),
//! `renamed` с неизвестным `from` — пропускается, `removed` неизвестного —
//! пропускается. Ядро отправляет `note:renamed` и `note:deleted` только для
//! заметок, которые есть в его списке заметок, поэтому свои переименования
//! и удаления окна обычно не повторяются событием `note:*` (но могут —
//! если событие наблюдателя пришло раньше ответа команды).
//!
//! События `note:*` отправляются для **любой** заметки хранилища, а не
//! только для открытых: какие заметки открыты, знает интерфейс, он и
//! фильтрует.
//!
//! `note:renamed` при несохранённом вводе: интерфейс переключает путь
//! буфера на новый и пишет туда (`note_write`) с прежней ожидаемой версией.
//! `note:deleted` (в том числе при несохранённом вводе): интерфейс закрывает
//! заметку с уведомлением, несохранённый ввод отбрасывается — внешнее
//! действие считается действием пользователя (решение владельца, С3.6).

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
    /// Папка хранилища стала недоступна или снова доступна (расширение
    /// контракта v1 в задаче 1.7).
    pub const VAULT_AVAILABILITY: &str = "vault:availability";
    /// Пользователь закрывает окно хранилища: окно уже скрыто, интерфейс
    /// записывает ввод и отвечает `window_close_ready` или
    /// `window_close_cancel` (задача 0.7, решение владельца 53).
    pub const WINDOW_CLOSE_REQUESTED: &str = "window:close-requested";
}

/// Все события API v1 — для тестов снимков.
pub const EVENT_NAMES: [&str; 7] = [
    event::VAULT_CHANGED,
    event::NOTE_CHANGED,
    event::NOTE_RENAMED,
    event::NOTE_DELETED,
    event::NOTE_CONFLICT_SAVED,
    event::VAULT_AVAILABILITY,
    event::WINDOW_CLOSE_REQUESTED,
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

/// `note:deleted`: заметка удалена снаружи; интерфейс закрывает её с
/// уведомлением, несохранённый ввод отбрасывается.
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

/// `vault:availability`: папка хранилища пропала или недоступна (отключили
/// флешку или сетевой диск, папку переименовали снаружи) — или снова
/// доступна. Пока недоступна, наблюдатель не считает файлы удалёнными:
/// интерфейс не закрывает заметки и показывает, что хранилище недоступно;
/// запись в это время отклоняется ошибкой ядра. После `available: true`
/// приходит `vault:changed` с `rescan: true` (полная сверка с диском) —
/// интерфейс перечитывает дерево и открытые заметки.
///
/// Расширение контракта v1 (задача 1.7): новое событие, прежние команды и
/// события не менялись.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultAvailabilityPayload {
    /// `false` — папка недоступна; `true` — снова доступна.
    pub available: bool,
}

/// `window:close-requested`: пользователь закрыл окно хранилища (задача 0.7,
/// решение владельца 53). Окно уже скрыто; ядро ждёт, пока интерфейс запишет
/// несохранённый ввод всех заметок окна, и уничтожает окно по
/// `window_close_ready` (или через [`crate::windows::closing::CLOSE_TIMEOUT`]
/// без ответа). `window_close_cancel` — запись не удалась: окно снова
/// показывается и не закрывается.
///
/// То же событие уходит каждому окну хранилища при конце сеанса Windows
/// (задача 0.11): окно не скрыто и не закрывается, ядро только ждёт записи.
///
/// Расширение контракта v1 (задача 0.7): новое событие и две команды,
/// прежние команды и события не менялись. Поле `sessionEnd` — задача 0.11а
/// (решение 79).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WindowCloseRequestedPayload {
    /// `true` — запрос записи перед выходом из системы или выключением
    /// Windows (не закрытие окна): интерфейс только записывает ввод и
    /// отвечает, правило «повторное закрытие в течение минуты» не действует.
    pub session_end: bool,
}
