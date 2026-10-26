//! Windows: скрытое окно верхнего уровня для `WM_QUERYENDSESSION` и
//! `WM_ENDSESSION` (задача 0.11).
//!
//! Окно для сообщений (`HWND_MESSAGE`, как у одного экземпляра) эти
//! сообщения не получает: Windows рассылает их окнам верхнего уровня. Окно
//! не показывается (нет `WS_VISIBLE`), в панели задач его нет
//! (`WS_EX_TOOLWINDOW`). Создаётся и живёт в главном потоке до конца
//! процесса: сообщения ему разбирает цикл событий Tauri.

use std::io;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetWindowLongPtrW, MSG,
    PM_REMOVE, PeekMessageW, RegisterClassExW, SetWindowLongPtrW, TranslateMessage, WINDOW_STYLE,
    WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION, WNDCLASSEXW, WS_EX_TOOLWINDOW,
};
use windows::core::{HSTRING, PCWSTR};

/// Имя класса окна. Тестовая сборка — своё, как у одного экземпляра.
pub const CLASS: &str = if cfg!(feature = "webdriver") {
    "Rhizomind.SessionEnd.webdriver"
} else {
    "Rhizomind.SessionEnd"
};

/// Сообщение конца сеанса для обработчика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// `WM_QUERYENDSESSION`: записать ввод (обработчик возвращается, когда
    /// записано или вышел предел).
    QueryEndSession,
    /// `WM_ENDSESSION`: `true` — сеанс завершается, `false` — выход отменён.
    EndSession(bool),
}

type Handler = Box<dyn Fn(Message) + 'static>;

/// Создаёт скрытое окно конца сеанса в текущем (главном) потоке; `handler`
/// вызывается на каждое сообщение конца сеанса. Окно живёт до конца процесса.
pub fn listen(handler: Handler) -> io::Result<HWND> {
    let class = HSTRING::from(CLASS);
    // SAFETY: строки живут до конца вызовов; указатель на обработчик
    // кладётся в окно после создания и освобождается в WM_NCDESTROY.
    unsafe {
        let module = GetModuleHandleW(None).map_err(io::Error::other)?;
        let instance = HINSTANCE(module.0);
        let window_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&window_class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(io::Error::last_os_error());
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            &class,
            PCWSTR::null(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(io::Error::other)?;
        let handler: *mut Handler = Box::into_raw(Box::new(handler));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, handler as isize);
        Ok(hwnd)
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: `GWLP_USERDATA` — либо 0, либо указатель из `start`, живой до
    // WM_NCDESTROY; окно и обработчик — в одном (главном) потоке.
    unsafe {
        let handler = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Handler;
        match msg {
            WM_QUERYENDSESSION if !handler.is_null() => {
                // Причина — на время записи; не удалась — запись всё равно идёт.
                let reason = HSTRING::from(super::BLOCK_REASON);
                let blocked = ShutdownBlockReasonCreate(hwnd, &reason).is_ok();
                (*handler)(Message::QueryEndSession);
                if blocked {
                    let _ = ShutdownBlockReasonDestroy(hwnd);
                }
                // Выход пользователя программа не блокирует.
                LRESULT(1)
            }
            WM_ENDSESSION if !handler.is_null() => {
                (*handler)(Message::EndSession(wparam.0 != 0));
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let handler = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Handler;
                if !handler.is_null() {
                    drop(Box::from_raw(handler));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Разбирает всё, что сейчас есть в очереди сообщений потока (движок
/// WebView2, вызовы ядра из интерфейса, события Tauri), и возвращается.
pub fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: `msg` — локальная переменная; очередь текущего потока.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

    use super::*;

    /// Окно получает оба сообщения в своём потоке и отвечает на запрос «да».
    #[test]
    fn hidden_window_answers_query_and_reports_both_messages() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let hwnd = listen(Box::new(move |message| log.borrow_mut().push(message))).expect("окно");
        // SAFETY: окно этого потока; SendMessageW вызывает процедуру сразу.
        let (query, _) = unsafe {
            (
                SendMessageW(hwnd, WM_QUERYENDSESSION, None, Some(LPARAM(0))),
                SendMessageW(hwnd, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0))),
            )
        };
        assert_eq!(query, LRESULT(1), "выход не блокируется");
        assert_eq!(
            *seen.borrow(),
            vec![Message::QueryEndSession, Message::EndSession(true)]
        );
    }
}
