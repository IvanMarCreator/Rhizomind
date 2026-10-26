//! Минимальное окно wry с переключателями (аргументы командной строки):
//! `datadir` — папка данных во временной папке (как приложение); `incognito` —
//! InPrivate; `flags` — флаги Rhizomind; `protocol` — страница своей схемы
//! `tauri://localhost` (на Windows — http://tauri.localhost); `ipc`, `init`,
//! `bg`, `noext` — обработчик IPC, скрипт инициализации, фоновый цвет,
//! расширения выключены; `secs=N` — сколько держать окно.
use std::borrow::Cow;
use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

const RHIZOMIND_FLAGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
--no-proxy-server --disable-background-networking --disable-component-update \
--disable-domain-reliability --no-pings --host-resolver-rules=\"MAP * ~NOTFOUND\"";

fn main() -> wry::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |s: &str| args.iter().any(|a| a == s);
    let secs: u64 = args
        .iter()
        .find_map(|a| a.strip_prefix("secs=").and_then(|v| v.parse().ok()))
        .unwrap_or(8);

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(format!("t06 wry probe {}", args.join(" ")))
        .build(&event_loop)
        .expect("окно");
    let data_dir = std::env::temp_dir().join(format!("t06-wry-{}", std::process::id()));
    let mut context = WebContext::new(has("datadir").then(|| data_dir.clone()));
    let mut builder = WebViewBuilder::new_with_web_context(&mut context);
    if has("incognito") {
        builder = builder.with_incognito(true);
    }
    if has("flags") {
        builder = builder.with_additional_browser_args(RHIZOMIND_FLAGS);
    }
    if has("noext") {
        builder = builder.with_browser_extensions_enabled(false);
    }
    if has("bg") {
        builder = builder.with_background_color((30, 30, 30, 255));
    }
    if has("ipc") {
        builder = builder.with_ipc_handler(|_request| {});
    }
    if has("init") {
        builder = builder.with_initialization_script("window.__t06 = 1;");
    }
    builder = if has("protocol") {
        builder
            .with_custom_protocol("tauri".into(), |_id, _request| {
                http::Response::builder()
                    .header("Content-Type", "text/html")
                    .body(Cow::Borrowed(&b"<!doctype html><h1>t06 wry protocol</h1>"[..]))
                    .expect("ответ")
            })
            .with_url("tauri://localhost/")
    } else {
        builder.with_html("<!doctype html><h1>t06 wry</h1>")
    };
    let webview = builder.build(&window)?;

    let start = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        let _keep = &webview;
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200));
        if start.elapsed() >= Duration::from_secs(secs) {
            *control_flow = ControlFlow::Exit;
        }
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}
