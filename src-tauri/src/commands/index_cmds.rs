//! Команды индекса хранилища (0.2 «Связи», С2-3.2, С2-3.3, С2-3.7, С2-5.5):
//! ход сборки, обратные ссылки, теги.
//!
//! Пока индекс строится, команды не ждут его и не отвечают ошибкой: ответ —
//! вариант `index_building` с ходом сборки ([`IndexReply`]), окно показывает
//! «Индекс строится… N %» и перезапрашивает по `index:status`. Так у
//! интерфейса нет гонки «состояние уже готово, а список ещё пуст».
//!
//! Задача 0.2-0.1 — только контракт: команды проверяют параметры и отвечают
//! «индекс строится». Индекс в работе окна — задача 0.2-1.4 (модуль
//! [`crate::index`]); формат ответов при этом не меняется.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::CoreResult;
use super::limits;
use super::types::VaultPath;
use super::{Core, checked_path};

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
/// — считаются (С2-3.2, Р4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backlinks {
    /// Сколько всего заметок ссылается (заголовок вида «N заметок»).
    pub notes_total: u32,
    /// Заметки в естественном порядке имён, затем путей — не больше
    /// [`limits::MAX_BACKLINK_NOTES`].
    pub notes: Vec<BacklinkNote>,
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

/// Состояние индекса хранилища окна.
#[tauri::command]
pub async fn index_status<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<IndexStatus> {
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1): индекса в окне ещё нет — 0.2-1.4.
    Ok(IndexStatus {
        state: IndexState::Building,
        done: 0,
        total: 0,
    })
}

/// Обратные ссылки заметки.
#[tauri::command]
pub async fn backlinks_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: BacklinksListRequest,
) -> CoreResult<IndexReply<Backlinks>> {
    checked_path(&request.path, "path")?;
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-1.4.
    Ok(IndexReply::building_unknown())
}

/// Все теги хранилища.
#[tauri::command]
pub async fn tags_list<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
) -> CoreResult<IndexReply<Tags>> {
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-1.4.
    Ok(IndexReply::building_unknown())
}

/// Заметки с тегом.
#[tauri::command]
pub async fn tag_notes<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: TagNotesRequest,
) -> CoreResult<IndexReply<TagNotes>> {
    limits::tag_key(&request.key)?;
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-1.4.
    Ok(IndexReply::building_unknown())
}
