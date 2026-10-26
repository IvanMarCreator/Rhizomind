//! Жив ли процесс и ожидание завершения процесса браузера с дочерними.

use std::time::{Duration, Instant};

/// Жив ли процесс с этим номером.
///
/// На ОС, где проверить нельзя без новых зависимостей (macOS), процесс
/// считается живым: уборка остатков его папку не тронет.
pub fn is_process_alive(pid: u32) -> bool {
    imp::is_process_alive(pid)
}

/// Имя образа процесса браузера WebView2.
#[cfg_attr(not(windows), allow(dead_code))]
const BROWSER_IMAGE: &str = "msedgewebview2.exe";

/// Путь образа — процесс браузера WebView2 (`msedgewebview2.exe`, регистр
/// не важен). Нужен только на Windows (и в тестах).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_browser_image(path: &str) -> bool {
    path.rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(BROWSER_IMAGE))
}

/// Процесс браузера WebView2 этого запуска (ревью 0.7, B3). Дескриптор
/// открывается сразу, как только движок сообщил `BrowserProcessId` (при
/// создании окна), и держится до выхода: завершение (решение владельца 64)
/// и ожидание при выходе идут только через него, а не по номеру — номер
/// упавшего браузера мог достаться чужому процессу. Принимается только живой
/// процесс с образом `msedgewebview2.exe`. Не Windows — процесса браузера с
/// номером нет, `open` всегда `None`.
#[derive(Debug)]
pub struct BrowserProcess {
    pid: u32,
    handle: imp::Handle,
}

impl BrowserProcess {
    /// Открывает процесс `pid`, если он жив и это браузер WebView2.
    pub fn open(pid: u32) -> Option<Self> {
        imp::open_browser(pid).map(|handle| Self { pid, handle })
    }

    /// Номер процесса.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Процесс ещё работает.
    pub fn is_alive(&self) -> bool {
        imp::handle_alive(&self.handle)
    }

    /// Завершает процесс (`TerminateProcess`) и ждёт его завершения не
    /// дольше `timeout`. `false` — завершить не удалось.
    pub fn terminate(&self, timeout: Duration) -> bool {
        imp::terminate_handle(&self.handle, timeout)
    }

    /// Ждёт завершения процесса и его дочерних (по списку на момент вызова),
    /// не дольше `timeout`.
    pub fn wait_tree_exit(&self, timeout: Duration) {
        imp::wait_tree_exit_handle(&self.handle, self.pid, timeout);
    }
}

/// Ждёт, пока завершатся процесс `pid` и его дочерние (по списку на момент
/// вызова), но не дольше `timeout`.
pub fn wait_for_process_tree_exit(pid: u32, timeout: Duration) {
    imp::wait_for_process_tree_exit(pid, timeout);
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE, WAIT_TIMEOUT};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
        WaitForSingleObject,
    };

    #[derive(Debug)]
    pub struct Handle(HANDLE);

    // SAFETY: дескриптор процесса — значение ядра ОС, не привязан к потоку;
    // им пользуются под замком `WebviewDataDir` и закрывают один раз (Drop).
    unsafe impl Send for Handle {}
    // SAFETY: см. выше; вызовы WinAPI с дескриптором процесса потокобезопасны.
    unsafe impl Sync for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: дескриптор получен от OpenProcess/CreateToolhelp32Snapshot
            // и закрывается один раз.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn open(pid: u32) -> Option<Handle> {
        // SAFETY: вызов WinAPI без указателей; ошибка — процесса нет или нет прав.
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .ok()
        .map(Handle)
    }

    pub fn is_process_alive(pid: u32) -> bool {
        let Some(handle) = open(pid) else {
            return false;
        };
        let mut code = 0u32;
        // SAFETY: дескриптор открыт с правом запроса сведений; code — локальная переменная.
        let ok = unsafe { GetExitCodeProcess(handle.0, &mut code) }.is_ok();
        ok && code == STILL_ACTIVE.0 as u32
    }

    /// Номера процессов, чей родитель входит в дерево `root` (вместе с `root`).
    fn process_tree(root: u32) -> Vec<u32> {
        let mut pairs = Vec::new();
        // SAFETY: снимок процессов; структура инициализирована с dwSize.
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return vec![root];
            };
            let snapshot = Handle(snapshot);
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snapshot.0, &mut entry).is_ok() {
                loop {
                    pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                    if Process32NextW(snapshot.0, &mut entry).is_err() {
                        break;
                    }
                }
            }
        }
        let mut tree = vec![root];
        let mut i = 0;
        while i < tree.len() {
            let parent = tree[i];
            for &(pid, ppid) in &pairs {
                if ppid == parent && pid != parent && !tree.contains(&pid) {
                    tree.push(pid);
                }
            }
            i += 1;
        }
        tree
    }

    fn exit_code_is_active(handle: &Handle) -> bool {
        let mut code = 0u32;
        // SAFETY: дескриптор открыт с правом запроса сведений; code — локальная переменная.
        let ok = unsafe { GetExitCodeProcess(handle.0, &mut code) }.is_ok();
        ok && code == STILL_ACTIVE.0 as u32
    }

    pub fn handle_alive(handle: &Handle) -> bool {
        exit_code_is_active(handle)
    }

    pub fn open_browser(pid: u32) -> Option<Handle> {
        // SAFETY: вызов WinAPI без указателей; ошибка — процесса нет или нет прав.
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .ok()
        .map(Handle)?;
        if !exit_code_is_active(&handle) {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        // SAFETY: буфер длиной `length` единиц UTF-16; дескриптор с правом
        // PROCESS_QUERY_LIMITED_INFORMATION.
        unsafe {
            QueryFullProcessImageNameW(
                handle.0,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        }
        .ok()?;
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        super::is_browser_image(&path).then_some(handle)
    }

    pub fn terminate_handle(handle: &Handle, timeout: Duration) -> bool {
        // SAFETY: дескриптор открыт с правом PROCESS_TERMINATE.
        if unsafe { TerminateProcess(handle.0, 1) }.is_err() {
            return false;
        }
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: дескриптор открыт с правом SYNCHRONIZE.
        let _ = unsafe { WaitForSingleObject(handle.0, ms) };
        true
    }

    pub fn wait_tree_exit_handle(root: &Handle, pid: u32, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        // Дочерние — только пока сам браузер жив: иначе номер родителя мог
        // достаться другому процессу и «дети» оказались бы чужими.
        let children: Vec<Handle> = if exit_code_is_active(root) {
            process_tree(pid)
                .into_iter()
                .filter(|&child| child != pid)
                .filter_map(open)
                .collect()
        } else {
            Vec::new()
        };
        for handle in std::iter::once(root).chain(children.iter()) {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: дескриптор открыт с правом SYNCHRONIZE.
            if unsafe { WaitForSingleObject(handle.0, ms) } == WAIT_TIMEOUT {
                return;
            }
        }
    }

    pub fn wait_for_process_tree_exit(pid: u32, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        // Дескрипторы открываются до ожидания: номер завершившегося процесса
        // может достаться другому, а открытый дескриптор указывает на наш.
        let handles: Vec<Handle> = process_tree(pid).into_iter().filter_map(open).collect();
        for handle in handles {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: дескриптор открыт с правом SYNCHRONIZE.
            if unsafe { WaitForSingleObject(handle.0, ms) } == WAIT_TIMEOUT {
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    pub fn is_process_alive(pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    /// Процесса браузера с номером нет: дескриптора тоже.
    #[derive(Debug)]
    pub enum Handle {}

    pub fn open_browser(_pid: u32) -> Option<Handle> {
        None
    }

    pub fn handle_alive(handle: &Handle) -> bool {
        match *handle {}
    }

    pub fn terminate_handle(handle: &Handle, _timeout: Duration) -> bool {
        match *handle {}
    }

    pub fn wait_tree_exit_handle(handle: &Handle, _pid: u32, _timeout: Duration) {
        match *handle {}
    }

    pub fn wait_for_process_tree_exit(pid: u32, timeout: Duration) {
        // На Linux своего процесса браузера с номером нет (WebKitGTK); ждём только `pid`.
        let deadline = Instant::now() + timeout;
        while is_process_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::*;

    pub fn is_process_alive(_pid: u32) -> bool {
        true
    }

    /// Процесса браузера с номером нет: дескриптора тоже.
    #[derive(Debug)]
    pub enum Handle {}

    pub fn open_browser(_pid: u32) -> Option<Handle> {
        None
    }

    pub fn handle_alive(handle: &Handle) -> bool {
        match *handle {}
    }

    pub fn terminate_handle(handle: &Handle, _timeout: Duration) -> bool {
        match *handle {}
    }

    pub fn wait_tree_exit_handle(handle: &Handle, _pid: u32, _timeout: Duration) {
        match *handle {}
    }

    pub fn wait_for_process_tree_exit(_pid: u32, _timeout: Duration) {
        let _ = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ревью 0.7 (B3): дескриптор берётся только у процесса браузера WebView2.
    #[test]
    fn only_webview2_browser_image_is_accepted() {
        assert!(is_browser_image(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\141.0\msedgewebview2.exe"
        ));
        assert!(is_browser_image(r"C:\X\MSEdgeWebView2.EXE"));
        assert!(!is_browser_image(r"C:\Windows\System32\notepad.exe"));
        assert!(!is_browser_image(r"C:\X\msedgewebview2.exe.bak"));
        assert!(!is_browser_image(r"C:\X\not-msedgewebview2.exe"));
    }

    /// Чужой живой процесс (сам тест) не принимается за браузер: его нельзя
    /// ни завершить, ни ждать по запомненному номеру.
    #[test]
    fn foreign_process_is_not_taken_as_browser() {
        assert!(BrowserProcess::open(std::process::id()).is_none());
        assert!(BrowserProcess::open(u32::MAX - 3).is_none());
    }
}
