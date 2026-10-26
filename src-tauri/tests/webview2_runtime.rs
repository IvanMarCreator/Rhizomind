//! Интеграционный тест на Windows (план 0.1, задача 0.3): настоящее окно с
//! WebView2, созданное тем же кодом, что в приложении. Проверяет то, что видно
//! только у живого движка:
//! - `IsReputationCheckingRequired = false` (SmartScreen выключен);
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
                        Some(window) => failures.extend(check_real_drag(&window)),
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
