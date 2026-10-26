//! Команды группы «Окно» (задача 0.7, решение владельца 53): ответ
//! интерфейса на событие `window:close-requested`. Действуют только на окно
//! вызова и только пока оно ждёт ответа ([`crate::windows::closing`]);
//! параметров нет.

use tauri::{Runtime, Window};

use super::error::CoreResult;
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
