//! Защищённые настройки браузерного движка (ADR 0007, ADR 0009).
//!
//! Здесь собрано то, что можно проверить без окна: строка флагов WebView2,
//! какие переменные окружения удаляются до создания окна, какие адреса может
//! открыть главное окно, а также разбор порта отладки для тестовой сборки
//! (функция cargo `webdriver`). Применяет эти настройки `crate::create_main_window`.

use std::ffi::{OsStr, OsString};


/// Флаги, которые wry 0.57 передаёт WebView2 по умолчанию (`src/webview2/mod.rs`
/// в wry). Свои `additional_browser_args` **заменяют** этот набор, поэтому он
/// повторяется здесь целиком.
/// - `msWebOOUI`, `msPdfOOUI` — всплывающее «мини-меню» Edge над выделением;
/// - `msSmartScreenProtection` — SmartScreen (проверка адресов на серверах Microsoft).
pub const WRY_DEFAULT_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Флаги движка для каждого запуска, по одному с пояснением.
///
/// Документирован для WebView2 только `--no-proxy-server`. Остальные — флаги
/// Chromium: для WebView2 они не документированы, и **действие каждого
/// подтверждается сетевым аудитом** перед выпуском (план 0.1, задача 3.3);
/// неподтверждённый флаг в `docs/SECURITY.md` гарантией не называется.
///
/// Флаг `--autoplay-policy` (wry добавляет его при `autoplay`) сюда не входит:
/// автозапуск звука и видео интерфейсу не нужен.
const PROTECTED_BROWSER_ARGS: &[&str] = &[
    // Набор wry по умолчанию (см. WRY_DEFAULT_BROWSER_ARGS).
    WRY_DEFAULT_BROWSER_ARGS,
    // Без прокси и без автопоиска прокси (WPAD). Документирован для WebView2.
    "--no-proxy-server",
    // Фоновые сетевые службы Chromium (проверки, предзагрузки, обновления
    // вариаций). Действие подтверждается сетевым аудитом.
    "--disable-background-networking",
    // Обновление компонентов Chromium (словари, списки и т. п.). Действие
    // подтверждается сетевым аудитом.
    "--disable-component-update",
    // Отчёты Domain Reliability о сетевых ошибках. Действие подтверждается
    // сетевым аудитом.
    "--disable-domain-reliability",
    // Пинги гиперссылок (`<a ping>`). Действие подтверждается сетевым аудитом.
    "--no-pings",
    // Любое имя хоста разрешается в «не найдено»: движок не может обратиться к
    // DNS. Адреса интерфейса (`http://tauri.localhost`, `http://ipc.localhost`)
    // перехватывает wry внутри процесса до разрешения имени. Действие
    // подтверждается сетевым аудитом.
    "--host-resolver-rules=\"MAP * ~NOTFOUND\"",
];

/// Строка `additional_browser_args` для выпускной сборки.
pub fn browser_args() -> String {
    PROTECTED_BROWSER_ARGS.join(" ")
}

/// Строка флагов для тестовой сборки (функция `webdriver`): защищённый набор и
/// порт отладки для msedgedriver поверх него. Без порта — защищённый набор.
pub fn browser_args_with_debug_port(port: Option<u16>) -> String {
    match port {
        Some(port) => format!("{} --remote-debugging-port={port}", browser_args()),
        None => browser_args(),
    }
}

/// Порт отладки из строки флагов msedgedriver (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`).
///
/// Берётся только `--remote-debugging-port=<цифры>`; остальные флаги драйвера
/// (`--user-data-dir` и др.) отбрасываются, чтобы тестовая сборка работала с
/// теми же временной папкой и флагами, что и выпускная.
pub fn webdriver_debug_port(driver_args: Option<&str>) -> Option<u16> {
    driver_args?
        .split_whitespace()
        .filter_map(|arg| arg.strip_prefix("--remote-debugging-port="))
        .find(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u16>().ok())
        .filter(|port| *port != 0)
}

/// Удалять ли переменную окружения до создания окна.
///
/// - `WEBVIEW2_*` — по документации Microsoft загрузчик WebView2 сам читает
///   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, `WEBVIEW2_USER_DATA_FOLDER`,
///   `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`, `WEBVIEW2_RELEASE_CHANNEL_PREFERENCE`
///   и др.: они могли бы подменить флаги, папку данных и сам движок.
/// - `TAURI_WEBVIEW_AUTOMATION` — на Linux включает управление WebKitGTK
///   извне (WebDriver). Сохраняется только в тестовой сборке, где этим
///   управляет tauri-driver.
pub fn is_webview_override_var(name: &OsStr, keep_automation: bool) -> bool {
    let Some(name) = name.to_str() else {
        // Имя не в Unicode — не наша переменная: загрузчик WebView2 и Tauri
        // таких не читают.
        return false;
    };
    let upper = name.to_ascii_uppercase();
    upper.starts_with("WEBVIEW2_") || (!keep_automation && upper == "TAURI_WEBVIEW_AUTOMATION")
}

/// Имена переменных, которые нужно удалить, из перечня окружения.
pub fn webview_override_vars(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    keep_automation: bool,
) -> Vec<OsString> {
    vars.into_iter()
        .map(|(name, _)| name)
        .filter(|name| is_webview_override_var(name, keep_automation))
        .collect()
}

/// Удаляет из окружения процесса переменные, которые могли бы переопределить
/// настройки движка. Возвращает имена удалённых переменных (без значений).
///
/// Вызывается первым делом в `crate::run`, до создания потоков и окна.
pub fn remove_webview_overrides(keep_automation: bool) -> Vec<OsString> {
    let names = webview_override_vars(std::env::vars_os(), keep_automation);
    for name in &names {
        // SAFETY: вызывается в начале `run()` из `main`, пока в процессе один
        // поток и окружение никто не читает параллельно.
        unsafe { std::env::remove_var(name) };
    }
    names
}

/// Может ли главное окно перейти по адресу.
///
/// Разрешён только адрес своего интерфейса: на Windows Tauri отдаёт его как
/// `http://tauri.localhost` (запросы перехватывает wry внутри процесса), на
/// macOS и Linux — `tauri://localhost`. Сервер разработки (`npm run dev`) —
/// только в сборке разработки (`dev_url`). Всё остальное — внешние сайты,
/// `file:`, `data:`, `javascript:` — отклоняется: внешняя ссылка открывается
/// только своей командой ядра с подтверждением (ADR 0009).
pub fn is_allowed_navigation(url: &tauri::Url, dev_url: Option<&tauri::Url>) -> bool {
    let own_app = match url.scheme() {
        "http" => url.host_str() == Some("tauri.localhost") && url.port().is_none(),
        "tauri" => url.host_str() == Some("localhost") && url.port().is_none(),
        _ => false,
    };
    let dev_server = dev_url.is_some_and(|dev| {
        url.scheme() == dev.scheme()
            && url.host_str() == dev.host_str()
            && url.port_or_known_default() == dev.port_or_known_default()
    });
    own_app && url.username().is_empty() && url.password().is_none() || dev_server
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> tauri::Url {
        s.parse().expect("адрес")
    }

    #[test]
    fn browser_args_are_wry_defaults_plus_no_proxy_and_background_network_flags() {
        assert_eq!(
            browser_args(),
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
             --no-proxy-server --disable-background-networking --disable-component-update \
             --disable-domain-reliability --no-pings \
             --host-resolver-rules=\"MAP * ~NOTFOUND\""
        );
    }

    #[test]
    fn browser_args_have_no_autoplay_policy_and_no_debug_port() {
        let args = browser_args();
        assert!(!args.contains("--autoplay-policy"), "{args}");
        assert!(!args.contains("--remote-debugging"), "{args}");
        // Повторный --disable-features отменил бы набор wry: побеждает последний.
        assert_eq!(args.matches("--disable-features").count(), 1, "{args}");
    }

    #[test]
    fn debug_port_is_appended_to_protected_set() {
        assert_eq!(
            browser_args_with_debug_port(Some(9222)),
            format!("{} --remote-debugging-port=9222", browser_args())
        );
        assert_eq!(browser_args_with_debug_port(None), browser_args());
    }

    #[test]
    fn webdriver_takes_only_numeric_debug_port() {
        assert_eq!(
            webdriver_debug_port(Some("--remote-debugging-port=9222 --user-data-dir=C:\\t")),
            Some(9222)
        );
        assert_eq!(
            webdriver_debug_port(Some("--user-data-dir=C:\\t   --remote-debugging-port=51234")),
            Some(51234)
        );
    }

    #[test]
    fn webdriver_rejects_everything_else() {
        for args in [
            None,
            Some(""),
            Some("  "),
            Some("--user-data-dir=C:\\t"),
            Some("--remote-debugging-port="),
            Some("--remote-debugging-port=0"),
            Some("--remote-debugging-port=-1"),
            Some("--remote-debugging-port=9222;--x"),
            Some("--remote-debugging-port=99999"),
            Some("--remote-debugging-pipe"),
            Some("--remote-debugging-address=0.0.0.0"),
        ] {
            assert_eq!(webdriver_debug_port(args), None, "{args:?}");
        }
    }

    #[test]
    fn webview2_and_automation_vars_are_removed() {
        let vars = [
            ("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=9333"),
            ("WEBVIEW2_USER_DATA_FOLDER", "C:\\x"),
            ("webview2_browser_executable_folder", "C:\\y"),
            ("WEBVIEW2_RELEASE_CHANNEL_PREFERENCE", "1"),
            ("TAURI_WEBVIEW_AUTOMATION", "true"),
            ("PATH", "C:\\Windows"),
            ("TEMP", "C:\\Temp"),
            ("MY_WEBVIEW2_NOTE", "x"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));

        assert_eq!(
            webview_override_vars(vars.clone(), false),
            [
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "WEBVIEW2_USER_DATA_FOLDER",
                "webview2_browser_executable_folder",
                "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
                "TAURI_WEBVIEW_AUTOMATION",
            ]
            .map(OsString::from)
        );
        assert!(
            !webview_override_vars(vars, true).contains(&OsString::from("TAURI_WEBVIEW_AUTOMATION"))
        );
    }

    #[test]
    fn remove_webview_overrides_clears_process_environment() {
        // Своё имя, чтобы не мешать другим тестам процесса.
        let name = "WEBVIEW2_RHIZOMIND_TEST_ONLY";
        // SAFETY: имя уникально для этого теста; другие тесты его не читают.
        unsafe { std::env::set_var(name, "--remote-debugging-port=9333") };
        let removed = remove_webview_overrides(true);
        assert!(removed.contains(&OsString::from(name)));
        assert!(std::env::var_os(name).is_none());
    }

    #[test]
    fn navigation_only_to_own_interface() {
        assert!(is_allowed_navigation(&url("http://tauri.localhost/"), None));
        assert!(is_allowed_navigation(&url("http://tauri.localhost/index.html#x"), None));
        assert!(is_allowed_navigation(&url("tauri://localhost/"), None));
    }

    #[test]
    fn navigation_to_anything_else_is_rejected() {
        for denied in [
            "https://tauri.localhost/",
            "http://tauri.localhost:8080/",
            "http://user@tauri.localhost/",
            "http://ipc.localhost/",
            "http://asset.localhost/x",
            "http://localhost:5173/",
            "http://example.com/",
            "https://example.com/",
            "http://tauri.localhost.example.com/",
            "file:///C:/Windows/win.ini",
            "data:text/html,<p>x</p>",
            "javascript:alert(1)",
            "about:blank",
            "tauri://evil/",
        ] {
            assert!(!is_allowed_navigation(&url(denied), None), "{denied}");
        }
    }

    #[test]
    fn dev_server_allowed_only_when_given() {
        let dev = url("http://localhost:5173");
        assert!(is_allowed_navigation(&url("http://localhost:5173/src/x"), Some(&dev)));
        assert!(!is_allowed_navigation(&url("http://localhost:5174/"), Some(&dev)));
        assert!(!is_allowed_navigation(&url("http://example.com/"), Some(&dev)));
    }

    #[test]
    fn clipboard_access_and_devtools_are_never_enabled_in_sources() {
        // Доступ страницы к буферу обмена и инструменты разработчика не
        // включаются нигде в коде ядра (ADR 0007, 0009).
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let forbidden = ["enable_clipboard_access", ".devtools(true)", "open_devtools"];
        let mut stack = vec![src];
        let mut checked = 0;
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("папка исходников") {
                let path = entry.expect("элемент папки").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).expect("исходник");
                    // Комментарии и сам список запрещённых имён не считаются.
                    let body: String = text
                        .lines()
                        .filter(|line| !line.trim_start().starts_with("//"))
                        .filter(|line| !line.contains("let forbidden = ["))
                        .collect::<Vec<_>>()
                        .join("\n");
                    for name in forbidden {
                        assert!(!body.contains(name), "{} вызывает {name}", path.display());
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked >= 3, "проверено файлов: {checked}");
    }
}
