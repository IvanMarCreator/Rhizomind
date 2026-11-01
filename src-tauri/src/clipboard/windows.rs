//! Буфер обмена Windows: только формат `CF_UNICODETEXT`, только чтение.

use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

use super::{ClipboardText, classify};

/// `CF_UNICODETEXT` — текст UTF-16 с нулём в конце (стандартный формат Windows).
const CF_UNICODETEXT: u32 = 13;

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
