//! Интеграционный тест на Windows (план 0.1, задача 0.3): настоящее окно с
//! WebView2, созданное тем же кодом, что в приложении. Проверяет то, что видно
//! только у живого движка:
//! - `IsReputationCheckingRequired = false` (SmartScreen выключен);
//! - `AreBrowserAcceleratorKeysEnabled = false`, `IsZoomControlEnabled = false`
//!   (решение 94), и настоящие нажатия (SendInput) Ctrl+P, Ctrl+F, Ctrl+=, F5,
//!   F12 доходят до страницы, но печати, перезагрузки и масштаба нет;
//! - `AreDefaultContextMenusEnabled = false` (решение 97), и настоящий правый
//!   щелчок (SendInput) не открывает меню движка;
//! - `IsInPrivateModeEnabled = true` у профиля движка, окно показано после проверки;
//! - `IsCustomCrashReportingEnabled = true` (правка wry): WebView2 запускает
//!   процесс браузера с ключом `--edge-webview-disable-crash-reporting=1`
//!   (проверено в CI: без правки ключа нет), тест читает командную строку
//!   процесса по `BrowserProcessId`;
//! - процесс браузера получил временную папку данных и защищённые флаги;
//! - окно после показа на переднем плане (исправление КТ1);
//! - настоящее перетаскивание мышью (SendInput) внутри страницы доходит до
//!   `drop` (исправление КТ1: перехватчик Tauri ломал его);
//! - после выхода временной папки нет.
//!
//! Свой `main` (harness = false): окну нужен главный поток. На других ОС —
//! пропуск. Запускается в `cargo test` (`npm run verify`).

#[cfg(not(windows))]
fn main() {
    println!("webview2_runtime: только для Windows — пропуск");
}

#[cfg(windows)]
fn main() {
    windows_only::main();
}

#[cfg(windows)]
mod windows_only {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use rhizomind_lib::{Shell, context, create_window, handle_run_event, security, windows};
    use tauri::{Manager, RunEvent};

    type Outcome = Arc<Mutex<Vec<String>>>;

    /// Командная строка процесса (через CIM, как Process Explorer).
    fn command_line(pid: u32) -> String {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').CommandLine"),
            ])
            .output()
            .expect("powershell");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn check_live_webview(
        controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
        data_dir: &Path,
        browser_args: &str,
    ) -> Vec<String> {
        let mut failures = Vec::new();

        match security::windows::is_reputation_checking_required(controller) {
            Ok(false) => println!("ok: IsReputationCheckingRequired = false"),
            other => failures.push(format!("IsReputationCheckingRequired: {other:?}")),
        }

        // Сочетания движка (решение 94).
        match security::windows::are_browser_accelerator_keys_enabled(controller) {
            Ok(false) => println!("ok: AreBrowserAcceleratorKeysEnabled = false"),
            other => failures.push(format!("AreBrowserAcceleratorKeysEnabled: {other:?}")),
        }
        match security::windows::are_default_context_menus_enabled(controller) {
            Ok(false) => println!("ok: AreDefaultContextMenusEnabled = false"),
            other => failures.push(format!("AreDefaultContextMenusEnabled: {other:?}")),
        }
        match security::windows::is_zoom_control_enabled(controller) {
            Ok(false) => println!("ok: IsZoomControlEnabled = false"),
            other => failures.push(format!("IsZoomControlEnabled: {other:?}")),
        }

        // InPrivate подтверждён движком (ICoreWebView2_13::Profile): без него
        // приложение окно не показывает.
        match security::windows::is_in_private_mode_enabled(controller) {
            Ok(true) => println!("ok: IsInPrivateModeEnabled = true"),
            other => failures.push(format!("IsInPrivateModeEnabled: {other:?}")),
        }

        let pid = match security::windows::browser_process_id(controller) {
            Ok(pid) => pid,
            Err(e) => {
                failures.push(format!("BrowserProcessId: {e}"));
                return failures;
            }
        };
        let cmd = command_line(pid);
        println!("процесс браузера {pid}: {cmd}");

        if cmd.contains("--edge-webview-disable-crash-reporting=1") {
            println!(
                "ok: IsCustomCrashReportingEnabled = true (--edge-webview-disable-crash-reporting=1)"
            );
        } else {
            failures.push(
                "нет --edge-webview-disable-crash-reporting=1: правка wry не действует".into(),
            );
        }

        let expected_udf = format!("--user-data-dir=\"{}\\EBWebView\"", data_dir.display());
        if !cmd.contains(&expected_udf) {
            failures.push(format!(
                "папка данных не временная: ожидалось {expected_udf}"
            ));
        }
        // WebView2 пересобирает строку флагов (порядок может меняться),
        // поэтому каждый флаг ищется отдельно.
        for flag in browser_args
            .split(" --")
            .map(|f| f.trim_start_matches("--"))
        {
            if !cmd.contains(&format!("--{flag}")) {
                failures.push(format!("процесс браузера без флага --{flag}"));
            }
        }
        for forbidden in ["--remote-debugging-port", "--autoplay-policy"] {
            if cmd.contains(forbidden) {
                failures.push(format!("в командной строке {forbidden}"));
            }
        }
        failures
    }

    /// Выполняет скрипт в странице окна и возвращает результат (JSON) — из
    /// фонового потока: вызов идёт в главном потоке, ответ ждётся здесь.
    fn run_script(window: &tauri::WebviewWindow, script: &str) -> Result<String, String> {
        use webview2_com::ExecuteScriptCompletedHandler;
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
        let script = windows_core_hstring(script);
        window
            .with_webview(move |webview| {
                let tx_error = tx.clone();
                let handler =
                    ExecuteScriptCompletedHandler::create(Box::new(move |error, result| {
                        let _ = tx.send(error.map(|()| result).map_err(|e| e.to_string()));
                        Ok(())
                    }));
                // SAFETY: вызов COM в главном потоке (with_webview).
                let started = unsafe {
                    webview
                        .controller()
                        .CoreWebView2()
                        .and_then(|core| core.ExecuteScript(&script, &handler))
                };
                if let Err(error) = started {
                    let _ = tx_error.send(Err(error.to_string()));
                }
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "скрипт не ответил за 10 с".to_owned())?
    }

    fn windows_core_hstring(text: &str) -> ::windows::core::HSTRING {
        ::windows::core::HSTRING::from(text)
    }

    /// Окно приложения на переднем плане после показа (исправление КТ1, п. 3):
    /// Windows отдаёт передний план одному окну — последнему показанному.
    fn check_foreground(handle: &tauri::AppHandle, labels: &[String]) -> Vec<String> {
        use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        // SAFETY: функция только читает состояние рабочего стола.
        let foreground = unsafe { GetForegroundWindow() };
        let ours: Vec<_> = labels
            .iter()
            .filter_map(|label| handle.get_webview_window(label)?.hwnd().ok())
            .map(|hwnd| hwnd.0 as usize)
            .collect();
        if ours.contains(&(foreground.0 as usize)) {
            println!("ok: окно приложения на переднем плане после показа");
            Vec::new()
        } else {
            vec![format!(
                "на переднем плане чужое окно {:#x}, окна приложения {ours:x?}",
                foreground.0 as usize
            )]
        }
    }

    /// Настоящее перетаскивание мышью (SendInput) внутри страницы: элемент с
    /// `draggable` тянется на цель, страница получает `drop` (исправление КТ1,
    /// п. 1). События HTML5 из скрипта (как в tests/e2e/tree.test.ts) обходят
    /// цикл перетаскивания Windows (OLE DoDragDrop), а перехватчик
    /// перетаскивания Tauri ломал именно его: курсор «запрещено». Ломает его и
    /// `AllowExternalDrop = false` (проба: dragstart → dragend:none). Заодно
    /// видно, что запрет бросков извне в странице (src/shell/externalDrop.ts)
    /// своё перетаскивание пропускает.
    fn check_real_drag(window: &tauri::WebviewWindow) -> Vec<String> {
        use ::windows::Win32::Foundation::{HWND, POINT};
        use ::windows::Win32::Graphics::Gdi::ClientToScreen;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
            MOUSEINPUT, SendInput,
        };
        use ::windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };

        let _ = window.set_focus();
        std::thread::sleep(Duration::from_millis(500));
        let setup = r#"(() => {
            const box = (id, top) => {
              const el = document.createElement('div');
              el.id = id;
              el.style.cssText = `position:fixed;left:24px;top:${top}px;width:220px;height:64px;z-index:2147483647;background:#c84;`;
              document.body.appendChild(el);
              return el;
            };
            const source = box('rz-drag-source', 40);
            source.draggable = true;
            const target = box('rz-drag-target', 220);
            window.__rzDrag = [];
            source.addEventListener('dragstart', (e) => {
              e.dataTransfer.setData('application/x-rhizomind-test', '1');
              e.dataTransfer.effectAllowed = 'move';
              window.__rzDrag.push('dragstart');
            });
            target.addEventListener('dragover', (e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; });
            source.addEventListener('dragend', (e) => window.__rzDrag.push('dragend:' + e.dataTransfer.dropEffect));
            target.addEventListener('drop', (e) => { e.preventDefault(); window.__rzDrag.push('drop'); });
            const center = (el) => { const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; };
            const [sx, sy] = center(source);
            const [tx, ty] = center(target);
            return [sx, sy, tx, ty, window.devicePixelRatio].join(',');
          })()"#;
        let coords = match run_script(window, setup) {
            Ok(text) => text,
            Err(error) => {
                return vec![format!("перетаскивание: страница не подготовлена: {error}")];
            }
        };
        let numbers: Vec<f64> = coords
            .trim_matches('"')
            .split(',')
            .filter_map(|n| n.parse().ok())
            .collect();
        let [sx, sy, tx, ty, dpr] = numbers[..] else {
            return vec![format!("перетаскивание: координаты {coords}")];
        };
        let hwnd = match window.hwnd() {
            Ok(hwnd) => HWND(hwnd.0),
            Err(error) => return vec![format!("перетаскивание: нет HWND: {error}")],
        };
        let to_screen = |x: f64, y: f64| {
            let mut point = POINT {
                x: (x * dpr).round() as i32,
                y: (y * dpr).round() as i32,
            };
            // SAFETY: окно живо, point — локальная переменная.
            let _ = unsafe { ClientToScreen(hwnd, &mut point) };
            point
        };
        let (from, to) = (to_screen(sx, sy), to_screen(tx, ty));
        println!("перетаскивание мышью: {from:?} → {to:?} (dpr {dpr})");

        // SAFETY: GetSystemMetrics только читает настройки.
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
            )
        };
        let send = |x: i32, y: i32, flags: MOUSE_EVENT_FLAGS| {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: (x - left) * 65535 / (width - 1),
                        dy: (y - top) * 65535 / (height - 1),
                        mouseData: 0,
                        dwFlags: flags
                            | MOUSEEVENTF_MOVE
                            | MOUSEEVENTF_ABSOLUTE
                            | MOUSEEVENTF_VIRTUALDESK,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            // SAFETY: один корректно заполненный INPUT.
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) }
        };
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        let mut sent = send(from.x, from.y, MOUSE_EVENT_FLAGS(0));
        pause(300);
        sent += send(from.x, from.y, MOUSEEVENTF_LEFTDOWN);
        pause(300);
        const STEPS: i32 = 30;
        for step in 1..=STEPS {
            let x = from.x + (to.x - from.x) * step / STEPS;
            let y = from.y + (to.y - from.y) * step / STEPS;
            sent += send(x, y, MOUSE_EVENT_FLAGS(0));
            pause(40);
        }
        // Несколько мелких движений над целью: цикл перетаскивания Windows
        // опрашивает цель (DragOver) по движению мыши.
        const WIGGLE: i32 = 6;
        for step in 0..WIGGLE {
            let dx = if step % 2 == 0 { 2 } else { -2 };
            sent += send(to.x + dx, to.y, MOUSE_EVENT_FLAGS(0));
            pause(60);
        }
        pause(300);
        sent += send(to.x, to.y, MOUSEEVENTF_LEFTUP);
        pause(1000);
        if sent != (STEPS + WIGGLE) as u32 + 3 {
            return vec![format!(
                "перетаскивание: SendInput принял {sent} событий из {}",
                STEPS + WIGGLE + 3
            )];
        }
        match run_script(window, "(window.__rzDrag || []).join(',')") {
            Ok(events) if events.contains("drop") => {
                println!("ok: перетаскивание мышью внутри страницы работает ({events})");
                Vec::new()
            }
            Ok(events) => vec![format!(
                "перетаскивание мышью внутри страницы не дошло до drop: события {events}"
            )],
            Err(error) => vec![format!("перетаскивание: {error}")],
        }
    }

    /// Настоящие нажатия клавиш (SendInput): аккорд из модификаторов и клавиши.
    fn send_chord(modifiers: &[u16], key: u16) -> u32 {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
            MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY,
        };
        let input = |vk: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    // Код сканирования нужен странице: по нему движок
                    // заполняет `KeyboardEvent.code` (без него code пустой).
                    // SAFETY: функция только переводит код клавиши.
                    wScan: unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut inputs: Vec<INPUT> = modifiers.iter().map(|&m| input(m, false)).collect();
        inputs.push(input(key, false));
        inputs.push(input(key, true));
        inputs.extend(modifiers.iter().rev().map(|&m| input(m, true)));
        // SAFETY: массив корректно заполненных INPUT живёт до конца вызова.
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) }
    }

    /// Ускорители браузера у живого окна (решение 94): настоящие нажатия
    /// Ctrl+P, Ctrl+F, Ctrl+=, Ctrl+Shift+I, F12, F5 доходят до страницы
    /// (команды реестра горячих клавиш их получат), но печати нет (событие
    /// `beforeprint`), перезагрузки нет (метки страницы на месте), масштаб не
    /// изменился (`devicePixelRatio`). Запасной рубеж интерфейса
    /// (src/shell/engineShortcuts.ts) отключён: слушатель страницы
    /// останавливает нажатие раньше оболочки — проверяется настройка движка.
    /// Контроль детектора: с включёнными на время ускорителями Ctrl+P должен
    /// вызвать `beforeprint` (пока — только сообщение в журнал).
    /// Видимые окна верхнего уровня процесса `pid` (меню движка — такое окно
    /// процесса браузера WebView2).
    fn visible_windows_of(pid: u32) -> Vec<isize> {
        use ::windows::Win32::Foundation::{HWND, LPARAM};
        use ::windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
        };
        unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> ::windows::core::BOOL {
            // SAFETY: param — указатель на (pid, Vec), живой на время EnumWindows.
            let (pid, found) = unsafe { &mut *(param.0 as *mut (u32, Vec<isize>)) };
            let mut owner = 0u32;
            // SAFETY: функции только читают сведения об окне.
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut owner));
                if owner == *pid && IsWindowVisible(hwnd).as_bool() {
                    found.push(hwnd.0 as isize);
                }
            }
            true.into()
        }
        let mut state: (u32, Vec<isize>) = (pid, Vec::new());
        // SAFETY: state живёт до конца вызова, collect не сохраняет указатель.
        let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut state as *mut _ as isize)) };
        state.1
    }

    /// Настоящий правый щелчок (SendInput) в точке страницы (CSS-пиксели).
    fn right_click(window: &tauri::WebviewWindow, x: f64, y: f64, dpr: f64) -> u32 {
        use ::windows::Win32::Foundation::{HWND, POINT};
        use ::windows::Win32::Graphics::Gdi::ClientToScreen;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
            MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
            SendInput,
        };
        use ::windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };
        let Ok(hwnd) = window.hwnd() else { return 0 };
        let mut point = POINT {
            x: (x * dpr).round() as i32,
            y: (y * dpr).round() as i32,
        };
        // SAFETY: окно живо, point — локальная переменная.
        let _ = unsafe { ClientToScreen(HWND(hwnd.0), &mut point) };
        // SAFETY: GetSystemMetrics только читает настройки.
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
            )
        };
        let input = |flags: MOUSE_EVENT_FLAGS| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: (point.x - left) * 65535 / (width - 1),
                    dy: (point.y - top) * 65535 / (height - 1),
                    mouseData: 0,
                    dwFlags: flags
                        | MOUSEEVENTF_MOVE
                        | MOUSEEVENTF_ABSOLUTE
                        | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            input(MOUSE_EVENT_FLAGS(0)),
            input(MOUSEEVENTF_RIGHTDOWN),
            input(MOUSEEVENTF_RIGHTUP),
        ];
        // SAFETY: массив корректно заполненных INPUT живёт до конца вызова.
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) }
    }

    /// Меню движка у живого окна (решение 97): настоящий правый щелчок
    /// доходит до страницы (`contextmenu`), но нового окна у процесса
    /// браузера (меню движка) нет. Запасной рубеж интерфейса отключён:
    /// слушатель страницы останавливает событие раньше него. Контроль: с
    /// включённым на время меню движка правый щелчок открывает окно меню.
    fn check_engine_context_menu(window: &tauri::WebviewWindow) -> Vec<String> {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
        use ::windows::core::Interface;
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings;

        let setup = r#"(() => {
            window.__rzMenu = [];
            addEventListener('contextmenu', (e) => {
              window.__rzMenu.push('contextmenu');
              e.stopImmediatePropagation();
            }, true);
            const x = Math.round(innerWidth / 2), y = Math.round(innerHeight / 2);
            return [x, y, window.devicePixelRatio].join(',');
          })()"#;
        let coords = match run_script(window, setup) {
            Ok(text) => text,
            Err(error) => return vec![format!("меню движка: страница не подготовлена: {error}")],
        };
        let numbers: Vec<f64> = coords
            .trim_matches('"')
            .split(',')
            .filter_map(|n| n.parse().ok())
            .collect();
        let [x, y, dpr] = numbers[..] else {
            return vec![format!("меню движка: координаты {coords}")];
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = window.with_webview(move |webview| {
            let _ = tx.send(security::windows::browser_process_id(&webview.controller()).ok());
        });
        let Some(pid) = rx.recv_timeout(Duration::from_secs(10)).ok().flatten() else {
            return vec!["меню движка: нет номера процесса браузера".into()];
        };
        let toggle = |window: &tauri::WebviewWindow, enabled: bool| {
            let _ = window.with_webview(move |webview| {
                // SAFETY: вызовы COM в главном потоке (with_webview).
                let _ = unsafe {
                    webview
                        .controller()
                        .CoreWebView2()
                        .and_then(|core| core.Settings())
                        .and_then(|s| s.cast::<ICoreWebView2Settings>())
                        .and_then(|s| s.SetAreDefaultContextMenusEnabled(enabled))
                };
            });
            std::thread::sleep(Duration::from_millis(300));
        };

        let _ = window.set_focus();
        std::thread::sleep(Duration::from_millis(500));
        let before = visible_windows_of(pid);
        let sent = right_click(window, x, y, dpr);
        std::thread::sleep(Duration::from_millis(1500));
        let after = visible_windows_of(pid);
        let popups = after.iter().filter(|h| !before.contains(h)).count();
        let mut failures = Vec::new();
        if sent != 3 {
            failures.push(format!("меню движка: SendInput принял {sent} событий из 3"));
        }
        match run_script(window, "(window.__rzMenu || []).join(',')") {
            Ok(events) if events.contains("contextmenu") => {}
            other => failures.push(format!(
                "меню движка: правый щелчок не дошёл до страницы ({other:?})"
            )),
        }
        if popups > 0 {
            failures.push(format!(
                "меню движка: правый щелчок открыл окно ({popups} новых окон процесса браузера)"
            ));
            send_chord(&[], VK_ESCAPE.0);
        }
        if failures.is_empty() {
            println!("ok: правый щелчок доходит до страницы, меню движка нет");
        }

        // Контроль: с включённым меню движка окно меню появляется.
        toggle(window, true);
        let before = visible_windows_of(pid);
        right_click(window, x, y, dpr);
        std::thread::sleep(Duration::from_millis(1500));
        let control = visible_windows_of(pid)
            .iter()
            .filter(|h| !before.contains(h))
            .count();
        send_chord(&[], VK_ESCAPE.0);
        std::thread::sleep(Duration::from_millis(500));
        toggle(window, false);
        if control > 0 {
            println!(
                "ok: контроль: с включённым меню движка правый щелчок открывает окно меню — детектор работает"
            );
        } else {
            println!("предупреждение: контроль детектора меню не подтвердился (новых окон нет)");
        }
        failures
    }

    fn check_engine_keys(window: &tauri::WebviewWindow) -> Vec<String> {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_CONTROL, VK_ESCAPE, VK_F5, VK_F12, VK_OEM_PLUS, VK_SHIFT,
        };
        use ::windows::core::Interface;
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, ICoreWebView2Settings3,
        };

        let setup = r#"(() => {
            window.__rzKeys = [];
            addEventListener('beforeprint', () => window.__rzKeys.push('beforeprint'));
            addEventListener('keydown', (e) => {
              window.__rzKeys.push((e.ctrlKey ? 'Ctrl+' : '') + (e.shiftKey ? 'Shift+' : '') + e.code);
              e.stopImmediatePropagation();
            }, true);
            return String(window.devicePixelRatio);
          })()"#;
        let dpr_before = match run_script(window, setup) {
            Ok(text) => text,
            Err(error) => {
                return vec![format!(
                    "сочетания движка: страница не подготовлена: {error}"
                )];
            }
        };
        let focus = |window: &tauri::WebviewWindow| {
            let _ = window.set_focus();
            let _ = window.with_webview(|webview| {
                // SAFETY: вызов COM в главном потоке (with_webview).
                let _ = unsafe {
                    webview
                        .controller()
                        .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
                };
            });
            std::thread::sleep(Duration::from_millis(500));
        };
        focus(window);
        let pause = || std::thread::sleep(Duration::from_millis(800));
        let ctrl = VK_CONTROL.0;
        let shift = VK_SHIFT.0;
        let mut sent = 0;
        for (modifiers, key) in [
            (vec![ctrl], u16::from(b'P')),
            (vec![ctrl], u16::from(b'F')),
            (vec![ctrl], VK_OEM_PLUS.0),
            (vec![ctrl, shift], u16::from(b'I')),
            (vec![], VK_F12.0),
            (vec![], VK_F5.0),
        ] {
            sent += send_chord(&modifiers, key);
            pause();
        }
        std::thread::sleep(Duration::from_millis(1500));
        let mut failures = Vec::new();
        // Нажатие и отпускание каждой клавиши: 4 + 4 + 4 + 6 + 2 + 2.
        if sent != 22 {
            failures.push(format!(
                "сочетания движка: SendInput принял {sent} событий из 22"
            ));
        }
        // ExecuteScript отдаёт JSON значения скрипта: массив [метки, dpr].
        let script = "[window.__rzKeys || null, String(window.devicePixelRatio)]";
        match run_script(window, script) {
            Ok(result) => {
                println!("сочетания движка: страница получила {result}");
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let keys: Vec<String> = parsed[0]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if parsed[0].is_null() {
                    failures.push(format!(
                        "сочетания движка: страница перезагрузилась (F5/Ctrl+R): {result}"
                    ));
                }
                for code in [
                    "Ctrl+KeyP",
                    "Ctrl+KeyF",
                    "Ctrl+Equal",
                    "Ctrl+Shift+KeyI",
                    "F12",
                    "F5",
                ] {
                    if !keys.iter().any(|k| k == code) {
                        failures.push(format!(
                            "сочетания движка: нажатие {code} не дошло до страницы"
                        ));
                    }
                }
                if keys.iter().any(|k| k == "beforeprint") {
                    failures.push("сочетания движка: Ctrl+P открыл печать".into());
                }
                let dpr_after = parsed[1].as_str().unwrap_or_default();
                if dpr_after != dpr_before.trim_matches('"') {
                    failures.push(format!(
                        "сочетания движка: масштаб изменился: было {dpr_before}, стало {dpr_after}"
                    ));
                }
                if failures.is_empty() {
                    println!(
                        "ok: сочетания движка доходят до страницы, печати, перезагрузки и масштаба нет"
                    );
                }
            }
            Err(error) => failures.push(format!("сочетания движка: {error}")),
        }

        // Контроль детектора печати: ускорители включаются на время.
        let toggle = |window: &tauri::WebviewWindow, enabled: bool| {
            let _ = window.with_webview(move |webview| {
                // SAFETY: вызовы COM в главном потоке (with_webview).
                let _ = unsafe {
                    webview
                        .controller()
                        .CoreWebView2()
                        .and_then(|core| core.Settings())
                        .and_then(|s| s.cast::<ICoreWebView2Settings3>())
                        .and_then(|s| s.SetAreBrowserAcceleratorKeysEnabled(enabled))
                };
            });
            std::thread::sleep(Duration::from_millis(300));
        };
        toggle(window, true);
        focus(window);
        send_chord(&[ctrl], u16::from(b'P'));
        std::thread::sleep(Duration::from_secs(3));
        let control = run_script(window, "(window.__rzKeys || []).join(',')");
        send_chord(&[], VK_ESCAPE.0);
        std::thread::sleep(Duration::from_millis(800));
        toggle(window, false);
        match control {
            Ok(events) if events.contains("beforeprint") => {
                println!(
                    "ok: контроль: с ускорителями Ctrl+P открывает печать — детектор работает"
                );
            }
            other => {
                println!("предупреждение: контроль детектора печати не подтвердился ({other:?})")
            }
        }
        failures
    }

    pub fn main() {
        let shell = Shell::prepare().expect("временная папка движка");
        let data_dir: PathBuf = shell.data_dir().to_path_buf();
        let browser_args = shell.browser_args().to_owned();
        println!("папка данных: {}", data_dir.display());
        assert!(data_dir.starts_with(std::env::temp_dir()));

        let outcome: Outcome = Arc::new(Mutex::new(vec!["проверка не выполнена".into()]));
        let outcome_for_check = outcome.clone();
        let data_dir_for_check = data_dir.clone();

        let app = tauri::Builder::default()
            .manage(shell)
            .setup(move |app| {
                // Два окна, как два хранилища (задача 1.3): защитные настройки
                // — у каждого, процесс браузера и папка данных — общие.
                let labels = [windows::new_label(), windows::new_label()];
                for label in &labels {
                    create_window(
                        app.handle(),
                        label,
                        &rhizomind_lib::main_window_title(),
                        None,
                    )?;
                }
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    // Окна созданы; SmartScreen выключается сразу после создания.
                    std::thread::sleep(Duration::from_secs(5));
                    let results: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
                    let (done_tx, done_rx) = std::sync::mpsc::channel();
                    for label in &labels {
                        let window = handle.get_webview_window(label).expect("окно");
                        // Окно создаётся скрытым и показывается после проверки InPrivate.
                        let visible = window.is_visible().unwrap_or(false);
                        let results = results.clone();
                        let done_tx = done_tx.clone();
                        let data_dir = data_dir_for_check.clone();
                        let browser_args = browser_args.clone();
                        let label = label.clone();
                        let _ = window.with_webview(move |webview| {
                            let mut failures =
                                check_live_webview(&webview.controller(), &data_dir, &browser_args);
                            if visible {
                                println!("ok: окно {label} показано после проверки InPrivate");
                            } else {
                                failures.push(format!(
                                    "окно {label} не показано после проверки InPrivate"
                                ));
                            }
                            match security::windows::browser_process_id(&webview.controller()) {
                                Ok(pid) if pid != 0 => failures.push(format!("pid:{pid}")),
                                other => failures.push(format!(
                                    "окно {label}: нет номера процесса браузера ({other:?})"
                                )),
                            }
                            results.lock().unwrap().push(failures);
                            let _ = done_tx.send(());
                        });
                    }
                    for _ in &labels {
                        let _ = done_rx.recv_timeout(Duration::from_secs(30));
                    }
                    let mut failures = Vec::new();
                    let mut pids = std::collections::BTreeSet::new();
                    for result in results.lock().unwrap().iter() {
                        for line in result {
                            match line.strip_prefix("pid:") {
                                Some(pid) => {
                                    pids.insert(pid.to_owned());
                                }
                                None => failures.push(line.clone()),
                            }
                        }
                    }
                    if results.lock().unwrap().len() != labels.len() {
                        failures.push("проверены не все окна".into());
                    }
                    if pids.len() == 1 {
                        println!("ok: у окон один процесс браузера {pids:?}");
                    } else {
                        failures.push(format!("процессы браузера окон: {pids:?}"));
                    }
                    // Передний план и перетаскивание мышью — после проверок
                    // движка: окна показаны, страница загружена.
                    failures.extend(check_foreground(&handle, &labels));
                    match labels
                        .last()
                        .and_then(|label| handle.get_webview_window(label))
                    {
                        Some(window) => {
                            failures.extend(check_real_drag(&window));
                            failures.extend(check_engine_keys(&window));
                            failures.extend(check_engine_context_menu(&window));
                        }
                        None => failures.push("нет окна для перетаскивания".into()),
                    }
                    *outcome_for_check.lock().unwrap() = failures;
                    // Выход как у пользователя — закрытием окон: WebView
                    // уничтожается, процесс браузера завершается, затем
                    // приходит RunEvent::Exit. С `exit(0)` окна и процесс
                    // браузера ещё живы в момент Exit, папку удалить нельзя.
                    for label in &labels {
                        if let Some(window) = handle.get_webview_window(label) {
                            let _ = window.close();
                        }
                    }
                });
                Ok(())
            })
            .build(context())
            .expect("приложение");

        app.run(move |handle, event| {
            let is_exit = matches!(event, RunEvent::Exit);
            handle_run_event(handle, &event);
            if is_exit {
                let mut failures = outcome.lock().unwrap().clone();
                if data_dir.exists() {
                    failures.push(format!(
                        "после выхода осталась папка данных {}",
                        data_dir.display()
                    ));
                } else {
                    println!("ok: папка данных удалена после выхода");
                }
                if failures.is_empty() {
                    println!("webview2_runtime: ok");
                    std::process::exit(0);
                }
                for failure in &failures {
                    eprintln!("ОШИБКА: {failure}");
                }
                std::process::exit(1);
            }
        });
    }
}
