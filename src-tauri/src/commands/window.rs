//! Команды группы «Окно».
//!
//! - Ответ интерфейса на событие `window:close-requested` (задача 0.7,
//!   решение владельца 53): действуют только на окно вызова и только пока
//!   оно ждёт ответа ([`crate::windows::closing`]); параметров нет.
//! - Заметка, показанная в окне (задача 2.12, решение владельца 82):
//!   `window_shown_note`. Заголовок окна ставит **только ядро**: интерфейс
//!   сообщает лишь путь показанной заметки, ядро проверяет, что это заметка
//!   хранилища этого окна, и берёт имя из пути само
//!   ([`OpenVault::show_note`]). Произвольный текст в заголовок интерфейс
//!   записать не может: права `core:window:allow-set-title` у него нет.
//!   Переименование и удаление показанной заметки снаружи заголовок ведёт
//!   наблюдатель ([`OpenVault::follow_shown`]).

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::{CoreError, CoreResult, ErrorCode};
use super::events::TreeChange;
use super::index::NoteIndex;
use super::types::VaultPath;
use super::{Core, OpenVault, blocking, checked_path};
use crate::windows::closing::{self, Answer};

/// Ввод окна записан: окно уничтожается.
#[tauri::command]
pub async fn window_close_ready<R: Runtime>(window: Window<R>) -> CoreResult<()> {
    closing::answer(&window, Answer::Ready);
    Ok(())
}

/// Ввод записать не удалось: окно показывается снова и не закрывается.
#[tauri::command]
pub async fn window_close_cancel<R: Runtime>(window: Window<R>) -> CoreResult<()> {
    closing::answer(&window, Answer::Cancel);
    Ok(())
}

/// Запрос `window_shown_note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowShownNoteRequest {
    /// Заметка, показанная в окне; `null` — заметка не открыта.
    pub path: Option<VaultPath>,
}

/// Интерфейс сообщает, какая заметка показана в окне: ядро ставит заголовок
/// окна «<заметка> — <хранилище> — Rhizomind» (без заметки — «<хранилище> —
/// Rhizomind»). Только окно с хранилищем (окно управления — `no_vault_open`);
/// путь — заметка этого хранилища (`invalid_path`, `not_a_note`,
/// `not_found`). При отказе заголовок не меняется.
#[tauri::command]
pub async fn window_shown_note<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: WindowShownNoteRequest,
) -> CoreResult<()> {
    if let Some(path) = &request.path {
        checked_path(path, "path")?;
    }
    let open = core.require_vault(window.label())?;
    blocking(move || {
        open.show_note(request.path, &|title: &str| {
            // Окно могло закрыться — заголовок никому не нужен.
            let _ = window.set_title(title);
        })
    })
    .await
}

impl OpenVault {
    /// Запоминает заметку, показанную в окне, и ставит заголовок окна
    /// (`set_title`). `path` должен быть заметкой из списка заметок этого
    /// хранилища: иначе отказ, и ни показанная заметка, ни заголовок не
    /// меняются. Имя заметки в заголовке — из пути (имя файла без `.md`).
    pub fn show_note(&self, path: Option<VaultPath>, set_title: &dyn Fn(&str)) -> CoreResult<()> {
        let index = self.read_index();
        if let Some(path) = &path {
            if !path.file_name().is_some_and(crate::names::is_note) {
                return Err(CoreError::new(ErrorCode::NotANote));
            }
            if !index.contains(path) {
                return Err(CoreError::new(ErrorCode::NotFound));
            }
        }
        let mut shown = self.shown_note();
        *shown = path;
        set_title(&self.title_for(shown.as_ref()));
        Ok(())
    }

    /// Наблюдатель изменил список заметок (`index` — уже новый, замок держит
    /// вызывающий): показанная заметка следует за переименованием (своим или
    /// папки над ней); пропала — заголовок без заметки. Заметка, которую
    /// заменили на месте (резервная копия переименованием и новый файл с тем
    /// же именем), остаётся. Заголовок ставится (`set_title`), только если
    /// изменился.
    pub fn follow_shown(
        &self,
        index: &NoteIndex,
        changes: &[TreeChange],
        set_title: &dyn Fn(&str),
    ) {
        let mut shown = self.shown_note();
        let Some(before) = shown.clone() else {
            return;
        };
        let mut moved = before.clone();
        for change in changes {
            if let TreeChange::Renamed { from, to } = change
                && let Some(rest) = rest_under(&moved, from)
            {
                moved = VaultPath::from_components(to.components().iter().chain(rest).cloned());
            }
        }
        // Переименование в не-заметку (`.md` → `.md~`) — уже не заметка, даже
        // если список заметок ещё держит путь.
        let is_shown = |path: &VaultPath| {
            path.file_name().is_some_and(crate::names::is_note) && index.contains(path)
        };
        let after = if is_shown(&moved) {
            Some(moved)
        } else if is_shown(&before) {
            Some(before.clone())
        } else {
            None
        };
        let old_title = self.title_for(Some(&before));
        let new_title = self.title_for(after.as_ref());
        *shown = after;
        if new_title != old_title {
            set_title(&new_title);
        }
    }

    /// Заголовок окна для показанной заметки `shown`.
    fn title_for(&self, shown: Option<&VaultPath>) -> String {
        let note = shown
            .and_then(VaultPath::file_name)
            .and_then(crate::names::note_stem);
        crate::windows::window_title(&self.summary.name, note)
    }

    fn shown_note(&self) -> std::sync::MutexGuard<'_, Option<VaultPath>> {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Компоненты `path` после `prefix`, если `path` — это `prefix` или лежит
/// под ним (имена сравниваются по ключам, как в списке заметок).
fn rest_under<'a>(path: &'a VaultPath, prefix: &VaultPath) -> Option<&'a [String]> {
    let (path, prefix) = (path.components(), prefix.components());
    let inside = prefix.len() <= path.len()
        && prefix
            .iter()
            .zip(path)
            .all(|(a, b)| crate::names::key(a) == crate::names::key(b));
    inside.then(|| &path[prefix.len()..])
}
