//! Чтение текста из системного буфера обмена для пункта «Вставить» своего
//! меню правки текста (решения 97, 98; команда `clipboard_read_text`).
//!
//! Только текст и только по запросу команды; ядро ничего не пишет и не
//! хранит. Разрешение движка ClipboardRead странице не выдаётся — страница
//! буфер сама не читает (`docs/SECURITY.md`).
//!
//! - Windows — формат `CF_UNICODETEXT` (`windows.rs`): длина проверяется до
//!   перевода в UTF-8, текст длиннее предела не читается целиком.
//! - Linux — буфер GTK (`CLIPBOARD`) в главном потоке (`gtk.rs`).
//! - Другие ОС — буфер пуст.

#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod gtk;

/// Что нашлось в буфере обмена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardText {
    /// Текст не длиннее предела (байты UTF-8).
    Text(String),
    /// Текста нет (пусто, картинка, файлы).
    Empty,
    /// Текст длиннее предела: не вставляется.
    TooLarge,
}

/// Разбор прочитанного текста: пустой — `Empty`, длиннее `limit_bytes` байт
/// UTF-8 — `TooLarge`.
pub fn classify(text: String, limit_bytes: u64) -> ClipboardText {
    if text.is_empty() {
        ClipboardText::Empty
    } else if text.len() as u64 > limit_bytes {
        ClipboardText::TooLarge
    } else {
        ClipboardText::Text(text)
    }
}

/// Текст системного буфера обмена, не длиннее `limit_bytes` байт UTF-8.
pub fn read_text<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    limit_bytes: u64,
) -> Result<ClipboardText, String> {
    #[cfg(windows)]
    {
        let _ = app;
        windows::read_text(limit_bytes)
    }
    #[cfg(target_os = "linux")]
    {
        gtk::read_text(app, limit_bytes)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (app, limit_bytes);
        Ok(ClipboardText::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_empty_long_text_is_too_large() {
        assert_eq!(classify(String::new(), 10), ClipboardText::Empty);
        assert_eq!(classify("абв".into(), 6), ClipboardText::Text("абв".into()));
        // Предел — в байтах UTF-8: «абв» — 6 байт.
        assert_eq!(classify("абв".into(), 5), ClipboardText::TooLarge);
    }
}
