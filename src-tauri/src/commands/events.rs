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
//! Переименование заметки в не-заметку (`a.md` → `a.txt`) приходит как
//! `note:deleted` (решение 90): для окна заметка ушла из заметок.
//! `note:deleted` (в том числе при несохранённом вводе): интерфейс закрывает
//! заметку с уведомлением, несохранённый ввод отбрасывается — внешнее
//! действие считается действием пользователя (решение владельца, С3.6).

use serde::{Deserialize, Serialize};

use super::types::{EntryKind, FileVersion, RelinkToken, VaultPath};

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
    /// Ход сборки индекса хранилища (0.2, С2-3.7): данные — те же, что у
    /// команды `index_status`; не чаще 4 раз в секунду.
    pub const INDEX_STATUS: &str = "index:status";
    /// Изменились обратные ссылки или теги (0.2): интерфейс перезапрашивает
    /// видимый вид; не чаще 4 раз в секунду.
    pub const INDEX_CHANGED: &str = "index:changed";
    /// Ядро собирается переписать ссылки: окно записывает несохранённый
    /// ввод всех своих заметок и отвечает `links_flush_done` (0.2, С2-5.4).
    pub const LINKS_FLUSH_REQUESTED: &str = "links:flush-requested";
    /// Нужен ответ пользователя об обновлении ссылок (внешние переименования,
    /// повторы имён; 0.2, решения 110, 111): ответ — `relink_answer`.
    pub const LINKS_PENDING: &str = "links:pending";
    /// Ссылки обновлены: сколько заметок переписано, где не удалось и
    /// почему (0.2, С2-3.5, решение 115).
    pub const LINKS_UPDATED: &str = "links:updated";
    /// Ход переписывания ссылок (перенос большой папки, 0.2, С2-8.3): не
    /// чаще `limits::MAX_EVENTS_PER_SEC` раз в секунду.
    pub const LINKS_PROGRESS: &str = "links:progress";
}

/// Все события API v1 — для тестов снимков.
pub const EVENT_NAMES: [&str; 13] = [
    event::VAULT_CHANGED,
    event::NOTE_CHANGED,
    event::NOTE_RENAMED,
    event::NOTE_DELETED,
    event::NOTE_CONFLICT_SAVED,
    event::VAULT_AVAILABILITY,
    event::WINDOW_CLOSE_REQUESTED,
    event::INDEX_STATUS,
    event::INDEX_CHANGED,
    event::LINKS_FLUSH_REQUESTED,
    event::LINKS_PENDING,
    event::LINKS_UPDATED,
    event::LINKS_PROGRESS,
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
    /// Нет поля — `false` (данные события до задачи 0.11а: `{}`).
    #[serde(default)]
    pub session_end: bool,
}

// ---- 0.2 «Связи»: индекс и обновление ссылок (С2-5.5) ----------------------
//
// Расширение контракта v1 (решение Р16): новые события, прежние не менялись.
// `index:status` несёт [`IndexStatus`](super::IndexStatus) — тот же ответ, что
// у команды `index_status`.

/// `index:changed`: изменились обратные ссылки или теги. Данных нет — окно
/// перезапрашивает то, что показывает (`backlinks_list`, `tags_list`,
/// `tag_notes`): так в событии нет ни путей, ни тегов.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexChangedPayload {}

/// `links:flush-requested`: окно записывает несохранённый ввод всех своих
/// заметок (как при закрытии окна, решение 53, без закрытия) и отвечает
/// `links_flush_done` с этим же знаком. Без ответа за 10 с — как ответ «не
/// записано»: ссылки не переписываются (С2-5.4, шаг 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksFlushRequestedPayload {
    /// Знак ожидания; возвращается в `links_flush_done`.
    pub token: RelinkToken,
}

/// Переименование «откуда → куда» (заметка или папка), путями хранилища.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathRename {
    /// Прежний путь.
    pub from: VaultPath,
    /// Новый путь.
    pub to: VaultPath,
}

/// О чём вопрос в `links:pending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    /// Подтверждённые переименования на диске (Проводник) в режиме «Всегда
    /// спрашивать» (решение 110).
    ExternalRename,
    /// Появились повторы имён: уточнить ссылки путём больше чем в 20
    /// заметках (решение 111).
    Duplicate,
}

/// `links:pending`: нужен ответ пользователя — уведомление внизу окна с
/// кнопками, висит до ответа (`relink_answer` с этим знаком). Одно событие
/// на пачку изменений.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksPendingPayload {
    /// Знак плана для `relink_answer`.
    pub token: RelinkToken,
    /// Внешние переименования или повторы имён.
    pub reason: PendingReason,
    /// Сколько заметок будет переписано.
    pub notes: u32,
    /// Сколько ссылок будет переписано.
    pub links: u32,
    /// Переименования пачки — не больше
    /// [`limits::MAX_PENDING_RENAMES`](super::limits::MAX_PENDING_RENAMES);
    /// у `duplicate` — пусто.
    pub renames: Vec<PathRename>,
    /// Сколько всего переименований в пачке (может быть больше `renames`).
    pub renames_total: u32,
}

/// Почему ссылки обновлялись (`links:updated`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinksUpdateReason {
    /// Переименование или перемещение в приложении (`entry_rename`,
    /// `entry_move` с `links: update`).
    Rename,
    /// Подтверждённое переименование на диске.
    ExternalRename,
    /// Уточнение путём при повторе имени.
    Duplicate,
    /// Доделка обновления, прерванного при прошлом запуске (журнал
    /// `link-update.json`).
    Resumed,
    /// Повтор пропущенных заметок по `retry_token` (решение 115).
    Retry,
}

/// Почему заметка не переписана (С2-3.5 «Где ссылки не могут быть
/// обновлены», Р20). Повторить (`retry_token`) можно только `busy` и `io`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelinkFailureReason {
    /// Больше максимального размера для редактирования (настройка).
    TooLarge,
    /// Не UTF-8.
    NotUtf8,
    /// Файл занят другой программой или запись запрещена (только чтение,
    /// нет прав).
    Busy,
    /// Другая ошибка ввода-вывода (нет места, носитель недоступен).
    Io,
    /// Изменилась, пока шло обновление, — повтор опасен (Р20): при доделке
    /// после сбоя заметка на диске не совпала ни с прочитанной, ни с
    /// записанной версией; или переименование не сопоставилось со списком
    /// заметок (тогда путь — «куда»). «Проверьте ссылки вручную», без
    /// «Повторить».
    Conflict,
}

impl RelinkFailureReason {
    /// Можно ли повторить (решение 115): занятые и недоступные — да, больше
    /// лимита и не UTF-8 — нет.
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Busy | Self::Io)
    }
}

/// Заметка, которую не удалось переписать.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkFailure {
    /// Заметка.
    pub path: VaultPath,
    /// Причина.
    pub reason: RelinkFailureReason,
}

/// Итог выполнения плана переписывания: ответ `entry_rename`/`entry_move`
/// (поле `relink`) и данные `links:updated`.
///
/// **Правило контракта:** уведомление пользователю («Ссылки обновлены…»,
/// «Не удалось обновить…») показывается **только по событию
/// `links:updated`**; поле `relink` в ответе переименования — для логики
/// окна (Ctrl+Z, выбор открытой заметки и т.п.), не для показа: иначе одно
/// обновление дало бы два уведомления.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkOutcome {
    /// Сколько заметок переписано.
    pub updated: u32,
    /// Заметки, которые переписать не удалось, с причиной.
    pub failed: Vec<RelinkFailure>,
    /// Знак для повтора (`relink_answer` с `apply: true`): есть, только если
    /// среди `failed` есть `busy` или `io` (решение 115).
    pub retry_token: Option<RelinkToken>,
}

/// `links:updated`: ссылки обновлены — единственный повод для уведомления
/// пользователю об итоге (см. [`RelinkOutcome`]). Интерфейс показывает «Ссылки
/// обновлены в N заметках» и, если `failed` не пуст, «Не удалось обновить
/// ссылки в N заметках» с кнопками «Показать» и (при `retry_token`)
/// «Повторить». Для переписанных заметок ядро отправляет и `note:changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksUpdatedPayload {
    /// Почему обновлялись ссылки.
    pub reason: LinksUpdateReason,
    /// Сколько заметок переписано.
    pub updated: u32,
    /// Заметки, которые переписать не удалось.
    pub failed: Vec<RelinkFailure>,
    /// Знак для «Повторить»; `null` — повторять нечего.
    pub retry_token: Option<RelinkToken>,
    /// Переименования, после которых обновлялись ссылки, «старое → новое»
    /// (0.2-4.6, ревью 0.2-5.1 М2): заголовок списка сбоев и уведомление
    /// «Заметка „Встреча“ переименована на диске…» (С2-3.5). Не больше
    /// [`limits::MAX_PENDING_RENAMES`](super::limits::MAX_PENDING_RENAMES);
    /// у `duplicate` и `resumed` — пусто.
    pub renames: Vec<PathRename>,
    /// Сколько всего переименований (может быть больше `renames`).
    pub renames_total: u32,
}

impl LinksUpdatedPayload {
    /// Данные события по итогу плана после переименований `renames`.
    pub fn new(reason: LinksUpdateReason, outcome: RelinkOutcome, renames: &[PathRename]) -> Self {
        Self {
            reason,
            updated: outcome.updated,
            failed: outcome.failed,
            retry_token: outcome.retry_token,
            renames: renames
                .iter()
                .take(super::limits::MAX_PENDING_RENAMES)
                .cloned()
                .collect(),
            renames_total: u32::try_from(renames.len()).unwrap_or(u32::MAX),
        }
    }
}

/// `links:progress`: ход переписывания ссылок (перенос папки на 1 000
/// заметок — секунды, «ход выполнения виден», С2-8.3). Не чаще
/// [`limits::MAX_EVENTS_PER_SEC`](super::limits::MAX_EVENTS_PER_SEC) раз в
/// секунду; конец — `links:updated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksProgressPayload {
    /// Знак плана, если план выдавался окну (`relink_preview`,
    /// `links:pending`); `null` — план без вопроса («Всегда обновлять»,
    /// уточнение до 20 заметок, доделка).
    pub token: Option<RelinkToken>,
    /// Сколько заметок плана уже обработано.
    pub done: u32,
    /// Сколько заметок в плане.
    pub total: u32,
}
