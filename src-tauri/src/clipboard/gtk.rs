//! Буфер обмена Linux через GTK (тот же GTK, на котором работает окно Tauri):
//! выбор `CLIPBOARD`, только текст. Вызовы GTK — в главном потоке.

use std::sync::mpsc;
use std::time::Duration;

use super::{ClipboardText, classify};

pub fn read_text<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    limit_bytes: u64,
) -> Result<ClipboardText, String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let text = clipboard.wait_for_text().map(|text| text.to_string());
        let _ = tx.send(text);
    })
    .map_err(|error| error.to_string())?;
    let text = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "буфер обмена не ответил".to_owned())?;
    Ok(text.map_or(ClipboardText::Empty, |text| classify(text, limit_bytes)))
}
