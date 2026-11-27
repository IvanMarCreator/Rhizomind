//! Буфер обмена Linux через GTK (тот же GTK, на котором работает окно Tauri):
//! выбор `CLIPBOARD`: текст или байты PNG. Вызовы GTK — в главном потоке.

use std::sync::mpsc;
use std::time::Duration;

use super::{ClipboardText, classify};

pub fn read_paste<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    text_limit: u64,
) -> Result<super::PasteData, String> {
    use super::{MAX_IMAGE_BYTES, PasteData};
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let targets = clipboard.wait_for_targets().unwrap_or_default();
        let names: Vec<_> = targets.iter().map(|atom| atom.name()).collect();
        let result = if names.iter().any(|name| {
            matches!(
                name.as_str(),
                "text/uri-list" | "x-special/gnome-copied-files" | "application/x-kde-cutselection"
            )
        }) {
            // Не просим содержимое списка URI и не вызываем обработчик открытия файлов.
            PasteData::ExternalPath
        } else if let Some(target) = targets.iter().find(|atom| atom.name() == "image/png") {
            match clipboard.wait_for_contents(target) {
                Some(data) if data.length() > MAX_IMAGE_BYTES as i32 => PasteData::TooLarge,
                Some(data) if data.length() > 0 => {
                    let bytes = data.data();
                    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                        PasteData::Png(bytes)
                    } else {
                        PasteData::Empty
                    }
                }
                _ => PasteData::Empty,
            }
        } else {
            clipboard.wait_for_text().map_or(PasteData::Empty, |text| {
                classify(text.to_string(), text_limit).into()
            })
        };
        let _ = tx.send(result);
    })
    .map_err(|error| error.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "буфер обмена не ответил".to_owned())
}

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
