//! Буфер обмена ОС по явному действию в окне хранилища (решения 97, 98).
//! Поля получают текст. Редактор также принимает изображение, переданное ОС
//! непосредственно, и отклоняет списки файлов по формату. Разрешение движка
//! ClipboardRead странице не выдаётся. Промежуточные данные находятся в памяти.

#[cfg(windows)]
mod dib;
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

/// Ни один вариант не содержит путь ОС.
pub enum PasteData {
    Png(Vec<u8>),
    Text(String),
    ExternalPath,
    TooLarge,
    Empty,
}

pub(crate) const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

pub fn read_paste<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    text_limit: u64,
) -> Result<PasteData, String> {
    #[cfg(windows)]
    {
        let _ = app;
        windows::read_paste(text_limit)
    }
    #[cfg(target_os = "linux")]
    {
        gtk::read_paste(app, text_limit)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (app, text_limit);
        Ok(PasteData::Empty)
    }
}

impl From<ClipboardText> for PasteData {
    fn from(text: ClipboardText) -> Self {
        match text {
            ClipboardText::Empty => Self::Empty,
            ClipboardText::Text(text) => Self::Text(text),
            ClipboardText::TooLarge => Self::TooLarge,
        }
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
