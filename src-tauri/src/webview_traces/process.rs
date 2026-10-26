//! Жив ли процесс и ожидание завершения процесса браузера с дочерними.

use std::time::{Duration, Instant};

/// Жив ли процесс с этим номером.
///
/// На ОС, где проверить нельзя без новых зависимостей (macOS), процесс
/// считается живым: уборка остатков его папку не тронет.
pub fn is_process_alive(pid: u32) -> bool {
    imp::is_process_alive(pid)
}

/// Завершает процесс `pid` (`TerminateProcess`) и ждёт его завершения не
/// дольше `timeout` (решение владельца 64: процесс браузера WebView2 при
/// выходе программы). `false` — процесс не удалось открыть или завершить.
/// На других ОС процесса браузера с номером нет — ничего не делает.
pub fn terminate(pid: u32, timeout: Duration) -> bool {
    imp::terminate(pid, timeout)
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
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    struct Handle(HANDLE);

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

    pub fn terminate(pid: u32, timeout: Duration) -> bool {
        // SAFETY: вызов WinAPI без указателей; ошибка — процесса нет или нет прав.
        let Ok(handle) =
            (unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) })
        else {
            return false;
        };
        let handle = Handle(handle);
        // SAFETY: дескриптор открыт с правом PROCESS_TERMINATE.
        if unsafe { TerminateProcess(handle.0, 1) }.is_err() {
            return false;
        }
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: дескриптор открыт с правом SYNCHRONIZE.
        let _ = unsafe { WaitForSingleObject(handle.0, ms) };
        true
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

    pub fn terminate(_pid: u32, _timeout: Duration) -> bool {
        false
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

    pub fn terminate(_pid: u32, _timeout: Duration) -> bool {
        false
    }

    pub fn wait_for_process_tree_exit(_pid: u32, _timeout: Duration) {
        let _ = Instant::now();
    }
}
