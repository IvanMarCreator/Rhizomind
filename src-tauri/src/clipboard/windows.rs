//! Буфер Windows: текст, PNG и DIB. Для списков файлов проверяется только наличие формата.

use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

use super::{ClipboardText, classify};

/// `CF_UNICODETEXT` — текст UTF-16 с нулём в конце (стандартный формат Windows).
const CF_UNICODETEXT: u32 = 13;

pub fn read_paste(text_limit: u64) -> Result<super::PasteData, String> {
    use super::{MAX_IMAGE_BYTES, PasteData};
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::core::w;
    let _opened = open()?;
    // SAFETY: регистрируются только константные NUL-строки. Наличие формата
    // списка файлов проверяется без получения его данных или имён.
    unsafe {
        let files = [
            15,
            RegisterClipboardFormatW(w!("Shell IDList Array")),
            RegisterClipboardFormatW(w!("FileGroupDescriptorW")),
            RegisterClipboardFormatW(w!("FileNameW")),
        ];
        if files
            .iter()
            .any(|&id| id != 0 && IsClipboardFormatAvailable(id).is_ok())
        {
            return Ok(PasteData::ExternalPath);
        }
        let png = RegisterClipboardFormatW(w!("PNG"));
        for format in [png, 17, 8] {
            if format == 0 || IsClipboardFormatAvailable(format).is_err() {
                continue;
            }
            let handle = GetClipboardData(format).map_err(|_| "нет изображения".to_owned())?;
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            if size > MAX_IMAGE_BYTES {
                return Ok(PasteData::TooLarge);
            }
            let ptr = GlobalLock(global).cast::<u8>();
            if ptr.is_null() {
                return Err("нет данных изображения".into());
            }
            let data = std::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(global);
            drop(_opened);
            if format == png {
                return if data.starts_with(b"\x89PNG\r\n\x1a\n") {
                    Ok(PasteData::Png(data))
                } else {
                    Err("неверный PNG".into())
                };
            }
            return super::dib::to_png(&data);
        }
    }
    read_opened_text(text_limit).map(Into::into)
}

/// Закрывает буфер обмена при выходе из области видимости.
struct Opened;

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: буфер открыт этим потоком (Opened создаётся после OpenClipboard).
        let _ = unsafe { CloseClipboard() };
    }
}

/// Открывает буфер: его может держать другая программа — несколько попыток.
fn open() -> Result<Opened, String> {
    for _ in 0..10 {
        // SAFETY: окна-владельца нет; буфер закрывает Opened.
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(Opened);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err("буфер обмена занят".into())
}

pub fn read_text(limit_bytes: u64) -> Result<ClipboardText, String> {
    // SAFETY: функция только проверяет наличие формата.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
        return Ok(ClipboardText::Empty);
    }
    let _opened = open()?;
    read_opened_text(limit_bytes)
}

fn read_opened_text(limit_bytes: u64) -> Result<ClipboardText, String> {
    // SAFETY: буфер открыт; данные принадлежат буферу, мы их только читаем
    // под GlobalLock до GlobalUnlock и до закрытия буфера.
    unsafe {
        let Ok(handle) = GetClipboardData(CF_UNICODETEXT) else {
            return Ok(ClipboardText::Empty);
        };
        let global = HGLOBAL(handle.0);
        let units = GlobalSize(global) / 2;
        let data = GlobalLock(global).cast::<u16>();
        if data.is_null() {
            return Ok(ClipboardText::Empty);
        }
        let all = std::slice::from_raw_parts(data, units);
        let len = all.iter().position(|&unit| unit == 0).unwrap_or(units);
        // Каждая единица UTF-16 — не меньше байта UTF-8: длиннее предела в
        // единицах — длиннее и в байтах, текст не переводится.
        let result = if len as u64 > limit_bytes {
            ClipboardText::TooLarge
        } else {
            classify(String::from_utf16_lossy(&all[..len]), limit_bytes)
        };
        let _ = GlobalUnlock(global);
        Ok(result)
    }
}
