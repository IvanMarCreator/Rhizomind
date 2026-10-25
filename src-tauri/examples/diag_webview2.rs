//! ВРЕМЕННАЯ диагностика (задача 0.3): что видно снаружи при
//! IsCustomCrashReportingEnabled = true/false и при WEBVIEW2_* в окружении.
//! Запуск: cargo run --example diag_webview2 -- <true|false> <папка данных>

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use std::sync::mpsc;
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::{
        Win32::{
            System::Com::*, System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::*,
        },
        core::*,
    };

    let mut args = std::env::args().skip(1);
    let crash: bool = args.next().unwrap().parse().unwrap();
    let udf = args.next().unwrap();
    let browser_args = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --no-proxy-server";
    println!("== crash_reporting={crash} udf={udf}");
    for (k, v) in std::env::vars() {
        if k.starts_with("WEBVIEW2") {
            println!("env {k}={v}");
        }
    }

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        let options = CoreWebView2EnvironmentOptions::default();
        options.set_additional_browser_arguments(browser_args.to_owned());
        options.set_is_custom_crash_reporting_enabled(crash);
        let (tx, rx) = mpsc::channel();
        let udf_h = HSTRING::from(udf.as_str());
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &udf_h,
            &ICoreWebView2EnvironmentOptions::from(options),
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |e, env| {
                tx.send(e.map(|_| env.unwrap())).unwrap();
                Ok(())
            })),
        )
        .unwrap();
        let env = wait_with_pump(rx).unwrap().unwrap();

        let hinstance = GetModuleHandleW(None).unwrap();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("diag"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            0,
            0,
            800,
            600,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        env.CreateCoreWebView2Controller(
            hwnd,
            &CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |e, c| {
                tx.send(e.map(|_| c.unwrap())).unwrap();
                Ok(())
            })),
        )
        .unwrap();
        let controller = wait_with_pump(rx).unwrap().unwrap();
        let webview = controller.CoreWebView2().unwrap();
        let settings = webview.Settings().unwrap();
        let s8: ICoreWebView2Settings8 = settings.cast().unwrap();
        let mut rep = BOOL::default();
        s8.IsReputationCheckingRequired(&mut rep).unwrap();
        println!("IsReputationCheckingRequired default = {}", rep.as_bool());
        let mut pid = 0u32;
        webview.BrowserProcessId(&mut pid).unwrap();
        println!("BrowserProcessId = {pid}");
        if let Ok(env11) = env.cast::<ICoreWebView2Environment11>() {
            let mut p = PWSTR::null();
            env11.FailureReportFolderPath(&mut p).unwrap();
            println!("FailureReportFolderPath = {}", p.to_string().unwrap());
        }

        pump_for(3000);
        dump(&udf, "before crash");

        let (tx, rx) = mpsc::channel();
        let r = webview.CallDevToolsProtocolMethod(
            w!("Browser.crash"),
            w!("{}"),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |e, s| {
                let _ = tx.send(format!("{e:?} {s}"));
                Ok(())
            })),
        );
        println!("Browser.crash call: {r:?}");
        pump_for(8000);
        println!("crash result: {:?}", rx.try_recv());
        dump(&udf, "after crash");
    }

    fn pump_for(ms: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        unsafe {
            let mut msg = MSG::default();
            while std::time::Instant::now() < deadline {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }

    fn dump(udf: &str, label: &str) {
        println!("---- {label}: processes");
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | ForEach-Object { \"$($_.ProcessId) parent=$($_.ParentProcessId) $($_.CommandLine)\" }",
            ])
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&out.stdout));
        println!("---- {label}: files under {udf}");
        for entry in walk(std::path::Path::new(udf)) {
            let len = std::fs::metadata(&entry).map(|m| m.len()).unwrap_or(0);
            let name = entry.display().to_string();
            println!("{name} ({len})");
            let lower = name.to_lowercase();
            if lower.contains("crashpad") && (lower.ends_with("settings.dat") || lower.ends_with("metadata")) {
                let bytes = std::fs::read(&entry).unwrap_or_default();
                println!("   hex: {}", bytes.iter().take(256).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""));
            }
        }
    }

    fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    v.extend(walk(&path));
                } else {
                    v.push(path);
                }
            }
        }
        v
    }
}
