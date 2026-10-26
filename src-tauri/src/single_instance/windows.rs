//! Windows: именованный мьютекс в пространстве имён сессии и окно «только
//! для сообщений» первого экземпляра.
//!
//! - Мьютекс `Local\<имя>` — объект ядра, не файл: на диск и в реестр ничего
//!   не пишется; ОС удаляет его с последним дескриптором (и при падении).
//!   Дескриптор не наследуется дочерними процессами (атрибуты по умолчанию).
//! - Окно для сообщений (`HWND_MESSAGE`) не видно, его нет в панели задач и в
//!   `EnumWindows`. Принимает только `WM_COPYDATA` с меткой [`PROTOCOL`];
//!   любое другое содержимое игнорируется. Действие сигнала одно — вывести
//!   окно вперёд, данные из сообщения сейчас не читаются.
//! - Второй экземпляр отдаёт первому право на передний план
//!   (`AllowSetForegroundWindow`): Windows разрешает выводить окно вперёд
//!   только процессу, который пользователь только что запустил.

use std::ffi::c_void;
use std::io;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_CLASS_ALREADY_EXISTS,
    GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW,
    GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, RegisterClassExW,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_COPYDATA, WM_NCDESTROY, WNDCLASSEXW,
};
use windows::core::{HSTRING, PCWSTR};

/// Метка сообщения «повторный запуск», версия 1 (`RZ` + 1). Полезной
/// нагрузки пока нет; путь хранилища из аргументов — будущая версия.
pub const PROTOCOL: usize = 0x525A_0001;

/// Сколько второй экземпляр ждёт ответа первого на одно сообщение.
const SEND_TIMEOUT_MS: u32 = 3000;

type Callback = Box<dyn Fn() + Send + Sync + 'static>;

/// Мьютекс первого экземпляра: закрывается вместе с владельцем.
#[derive(Debug)]
pub struct MutexGuard(isize);

impl Drop for MutexGuard {
    fn drop(&mut self) {
        // SAFETY: дескриптор получен от CreateMutexW и закрывается один раз.
        unsafe {
            let _ = CloseHandle(HANDLE(self.0 as *mut c_void));
        }
    }
}

/// Занимает мьютекс `Local\<name>`. `Ok(None)` — он уже есть (в том числе
/// создан процессом с другими правами — например, запущенным от
/// администратора).
pub fn try_acquire_named(name: &str) -> io::Result<Option<MutexGuard>> {
    let full = HSTRING::from(format!("Local\\{name}"));
    // SAFETY: имя живёт до конца вызова; атрибуты по умолчанию.
    match unsafe { CreateMutexW(None, false, &full) } {
        Ok(handle) => {
            // SAFETY: сразу после CreateMutexW, других вызовов WinAPI между ними нет.
            let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            let guard = MutexGuard(handle.0 as isize);
            if existed {
                drop(guard);
                Ok(None)
            } else {
                Ok(Some(guard))
            }
        }
        Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => Ok(None),
        Err(error) => Err(io::Error::other(error)),
    }
}

/// Будит первый экземпляр: находит его окно для сообщений класса `class`,
/// отдаёт право на передний план и шлёт `WM_COPYDATA`. `false` — окна нет
/// или оно не ответило.
pub fn notify_named(class: &str) -> bool {
    let class = HSTRING::from(class);
    // SAFETY: строки живут до конца вызовов; `cds` и `result` — локальные
    // переменные, SendMessageTimeoutW копирует данные до возврата.
    unsafe {
        let Ok(hwnd) = FindWindowExW(Some(HWND_MESSAGE), None, &class, PCWSTR::null()) else {
            return false;
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            let _ = AllowSetForegroundWindow(pid);
        }
        let cds = COPYDATASTRUCT {
            dwData: PROTOCOL,
            cbData: 0,
            lpData: std::ptr::null_mut(),
        };
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            Some(&mut result),
        );
        sent.0 != 0 && result == 1
    }
}

/// Окно для сообщений первого экземпляра. Сообщения ему разбирает очередь
/// потока, создавшего окно (в программе — цикл событий Tauri в главном
/// потоке); уничтожать — в том же потоке.
#[derive(Debug)]
pub struct Listener(isize);

impl Listener {
    /// Создаёт окно класса `class`; `on_signal` вызывается на каждый сигнал.
    pub fn start_named(class: &str, on_signal: Callback) -> io::Result<Self> {
        let class = HSTRING::from(class);
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
            if RegisterClassExW(&window_class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS
            {
                return Err(io::Error::last_os_error());
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                &class,
                PCWSTR::null(),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
            .map_err(io::Error::other)?;
            let callback: *mut Callback = Box::into_raw(Box::new(on_signal));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, callback as isize);
            Ok(Self(hwnd.0 as isize))
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // SAFETY: окно создано этим объектом; обработчик освобождается в
        // WM_NCDESTROY. Из чужого потока DestroyWindow откажет — окно умрёт
        // вместе с процессом.
        unsafe {
            let _ = DestroyWindow(HWND(self.0 as *mut c_void));
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: `GWLP_USERDATA` — либо 0, либо указатель из `start_named`,
    // живой до WM_NCDESTROY; `lparam` у WM_COPYDATA — COPYDATASTRUCT
    // отправителя, действительный на время обработки.
    unsafe {
        match msg {
            WM_COPYDATA => {
                let data = lparam.0 as *const COPYDATASTRUCT;
                let callback = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Callback;
                if data.is_null() || callback.is_null() || (*data).dwData != PROTOCOL {
                    return LRESULT(0);
                }
                (*callback)();
                LRESULT(1)
            }
            WM_NCDESTROY => {
                let callback = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Callback;
                if !callback.is_null() {
                    drop(Box::from_raw(callback));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Тесты: разбирает очередь сообщений потока, пока `stop` не вернёт `true`
/// или не выйдет `limit`.
#[cfg(test)]
pub fn pump_messages_until(limit: std::time::Duration, stop: impl Fn() -> bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let deadline = std::time::Instant::now() + limit;
    while !stop() && std::time::Instant::now() < deadline {
        let mut msg = MSG::default();
        // SAFETY: `msg` — локальная переменная; очередь текущего потока.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
