//! Команды обновления ссылок (0.2 «Связи», С2-3.5, С2-3.6, С2-5.4, С2-5.5).
//!
//! Интерфейс не может попросить ядро переписать произвольный файл: план
//! переписывания всегда считает ядро, интерфейс получает только число
//! заметок и ссылок и непрозрачный знак плана ([`RelinkToken`]) и может лишь
//! подтвердить или отклонить план (С2-6 «Граница»). Списка файлов в запросах
//! нет, лишнее поле отклоняется (`deny_unknown_fields`).
//!
//! Порядок для переименования в приложении (С2-5.4): `relink_preview` →
//! ответ пользователя → `entry_rename`/`entry_move` с `links: update` и
//! знаком → событие `links:flush-requested` → `links_flush_done` → файл
//! переименован → ссылки переписаны → ответ с `relink` и `links:updated`.
//!
//! Задача 0.2-0.1 — только контракт: параметры проверяются, `relink_preview`
//! отвечает «индекс строится», знаков ядро ещё не выдаёт, поэтому любой
//! знак — `invalid_argument` (`token`). Реализация — задачи 0.2-4.6, 0.2-6.1.

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::{CoreError, CoreResult};
use super::index_cmds::IndexReply;
use super::limits;
use super::types::{RelinkToken, VaultPath};
use super::{Core, check_entry_name, checked_path};

/// Запрос `relink_preview`: какое переименование или перемещение
/// пользователь собирается сделать. В JSON — поле `op`:
/// `{ "op": "rename", "path": [...], "new_name": "..." }` или
/// `{ "op": "move", "path": [...], "new_dir": [...] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelinkPreviewRequest {
    /// Переименование заметки или папки (параметры как у `entry_rename`).
    Rename {
        /// Что переименовать.
        path: VaultPath,
        /// Новое полное имя, как на диске (для заметки — с `.md`).
        new_name: String,
    },
    /// Перемещение заметки или папки (параметры как у `entry_move`).
    Move {
        /// Что переместить.
        path: VaultPath,
        /// Папка назначения (`[]` — корень).
        new_dir: VaultPath,
    },
}

/// Предпросмотр обновления ссылок (`relink_preview`, вариант `ready`).
/// `notes: 0` — переписывать нечего: интерфейс переименовывает сразу, без
/// окна и с `links: keep` (С2-3.5). Уточнение ссылок при повторе имени,
/// вызванное этим переименованием, в числа не входит и от ответа не зависит
/// (план 0.2, Т6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkPreview {
    /// Сколько заметок ссылается на переименуемое (будут переписаны).
    pub notes: u32,
    /// Сколько ссылок будет переписано.
    pub links: u32,
    /// Знак плана для `entry_rename`/`entry_move` с `links: update`.
    pub token: RelinkToken,
}

/// Запрос `relink_answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkAnswerRequest {
    /// Знак из `links:pending` (вопрос о внешних переименованиях или о
    /// повторах имён) или `retry_token` из `links:updated`/`relink`
    /// («Повторить», решение 115).
    pub token: RelinkToken,
    /// `true` — «Обновить ссылки», «Уточнить», «Повторить»; `false` — «Не
    /// обновлять», «Не трогать» (план отбрасывается).
    pub apply: bool,
}

/// Запрос `links_flush_done`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksFlushDoneRequest {
    /// Знак из `links:flush-requested`.
    pub token: RelinkToken,
    /// `true` — несохранённый ввод всех заметок окна записан; `false` —
    /// записать не удалось: ссылки не переписываются, как и без ответа за
    /// 10 с (инвариант 4).
    pub saved: bool,
}

/// Знак от интерфейса: правильного вида — иначе `invalid_argument` с полем
/// `field`. Ядро 0.2-0.1 знаков не выдаёт, поэтому и правильный знак ему
/// неизвестен — тоже `invalid_argument` (реализация — 0.2-4.6).
pub(super) fn unknown_token(token: &RelinkToken, field: &str) -> CoreResult<()> {
    limits::token(token, field)?;
    Err(CoreError::invalid_argument(field))
}

/// Проверка операции предпросмотра — те же правила, что у `entry_rename` и
/// `entry_move`.
fn check_preview(request: &RelinkPreviewRequest) -> CoreResult<()> {
    match request {
        RelinkPreviewRequest::Rename { path, new_name } => {
            checked_path(path, "path")?;
            check_entry_name(new_name, "new_name")?;
            let old_name = path
                .file_name()
                .ok_or_else(|| CoreError::invalid_argument("path"))?;
            match crate::names::check_rename_keeps_kind(old_name, new_name) {
                Some(problem) => Err(CoreError::invalid_name(problem)),
                None => Ok(()),
            }
        }
        RelinkPreviewRequest::Move { path, new_dir } => {
            checked_path(path, "path")?;
            if path.components().is_empty() {
                return Err(CoreError::invalid_argument("path"));
            }
            checked_path(new_dir, "new_dir")
        }
    }
}

/// Сколько заметок и ссылок перепишет переименование или перемещение, и
/// знак плана. Пока индекс строится — `index_building`: интерфейс показывает
/// окно «Подождите, индекс строится…» в любом режиме настройки (решение 114).
#[tauri::command]
pub async fn relink_preview<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: RelinkPreviewRequest,
) -> CoreResult<IndexReply<RelinkPreview>> {
    check_preview(&request)?;
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-4.6.
    Ok(IndexReply::building_unknown())
}

/// Ответ пользователя на `links:pending` или «Повторить». Итог приходит
/// событием `links:updated`.
#[tauri::command]
pub async fn relink_answer<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: RelinkAnswerRequest,
) -> CoreResult<()> {
    limits::token(&request.token, "token")?;
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-4.6, 0.2-6.1.
    unknown_token(&request.token, "token")
}

/// Окно записало (или не смогло записать) несохранённый ввод перед
/// переписыванием ссылок.
#[tauri::command]
pub async fn links_flush_done<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksFlushDoneRequest,
) -> CoreResult<()> {
    limits::token(&request.token, "token")?;
    core.require_vault(window.label())?;
    // Заглушка контракта (0.2-0.1) — 0.2-4.6.
    unknown_token(&request.token, "token")
}
