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
                    create_window(app.handle(), label, None)?;
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
                            let pid = security::windows::browser_process_id(&webview.controller())
                                .unwrap_or(0);
                            failures.push(format!("pid:{pid}"));
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
