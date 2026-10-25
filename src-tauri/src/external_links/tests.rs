//! Тесты внешних ссылок на шве `open_checked`: «показать окно подтверждения»
//! и «передать адрес ОС» подменяются, настоящих окон и браузера нет.

use std::cell::RefCell;
use std::io;

use super::*;
use crate::commands::ErrorCode;

/// Запись того, что произошло на шве.
#[derive(Default)]
struct Seam {
    asked: RefCell<Vec<Confirmation>>,
    launched: RefCell<Vec<String>>,
}

impl Seam {
    fn run(&self, window: &str, url: &str, answer: bool) -> CoreResult<OpenExternalOutcome> {
        open_checked(
            window,
            url,
            |confirmation| {
                self.asked.borrow_mut().push(confirmation.clone());
                answer
            },
            |target| {
                self.launched.borrow_mut().push(target.to_owned());
                Ok(())
            },
        )
    }
}

fn code_of(result: CoreResult<OpenExternalOutcome>) -> ErrorCode {
    result.expect_err("ожидался отказ").code
}

#[test]
fn without_confirmation_address_does_not_reach_os() {
    let seam = Seam::default();
    let outcome = seam.run("w-cancel", "https://example.org/a", false);
    assert_eq!(outcome, Ok(OpenExternalOutcome::Cancelled));
    assert_eq!(seam.asked.borrow().len(), 1, "окно подтверждения показано");
    assert!(seam.launched.borrow().is_empty(), "адрес не ушёл в ОС");
}

#[test]
fn after_confirmation_address_goes_to_os_once() {
    let seam = Seam::default();
    let outcome = seam.run("w-open", "https://example.org/a?b=1#c", true);
    assert_eq!(outcome, Ok(OpenExternalOutcome::Opened));
    assert_eq!(*seam.launched.borrow(), ["https://example.org/a?b=1#c"]);
}

#[test]
fn other_schemes_are_rejected_without_dialog() {
    for url in [
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "ftp://example.org/",
        "vbscript:msgbox",
        "ms-settings:privacy",
        "search-ms:query=x",
        "\\\\server\\share\\x.exe",
        "C:\\Windows\\notepad.exe",
        "example.org",
        "//example.org/a",
        "",
    ] {
        let seam = Seam::default();
        assert_eq!(
            code_of(seam.run("w-schemes", url, true)),
            ErrorCode::SchemeNotAllowed,
            "{url}"
        );
        assert!(seam.asked.borrow().is_empty(), "{url}: окна не было");
        assert!(seam.launched.borrow().is_empty(), "{url}: в ОС не ушло");
        assert_eq!(
            check_url(url).unwrap_err().code,
            ErrorCode::SchemeNotAllowed
        );
    }
}

#[test]
fn allowed_schemes_pass_in_any_case() {
    for url in [
        "http://example.org/",
        "https://example.org/",
        "HTTPS://Example.ORG/",
        "mailto:ivan@example.org",
    ] {
        assert_eq!(check_url(url), Ok(()), "{url}");
    }
    assert_eq!(ALLOWED_SCHEMES, ["http", "https", "mailto"]);
}

#[test]
fn os_failure_is_reported_as_io_failed() {
    let result = open_checked(
        "w-io",
        "https://example.org/",
        |_| true,
        |_| Err(io::Error::other("нет браузера")),
    );
    assert_eq!(code_of(result), ErrorCode::IoFailed);
}

// ---- Пункты обзора 0.4 (docs/reviews/2026-10-25-task-0.4.md) -------------

/// Окно показало ровно ту строку (отдельной строкой текста), что ушла в ОС.
///
/// Метка окна у каждого вызова своя: тесты идут параллельно, а занятое окно
/// подтверждения в общей метке дало бы `cancelled` соседнему тесту.
fn shown_equals_launched(url: &str) -> (Confirmation, String) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let window = format!(
        "w-exact-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let seam = Seam::default();
    assert_eq!(
        seam.run(&window, url, true),
        Ok(OpenExternalOutcome::Opened),
        "{url}"
    );
    let confirmation = seam.asked.borrow()[0].clone();
    let launched = seam.launched.borrow()[0].clone();
    assert!(
        confirmation.message.lines().any(|line| line == launched),
        "{url}: в окне нет строки {launched:?}:\n{}",
        confirmation.message
    );
    assert!(
        launched
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)),
        "{url}: в ОС ушёл символ вне RFC 3986: {launched}"
    );
    (confirmation, launched)
}

#[test]
fn dialog_shows_exactly_the_string_sent_to_os() {
    let (_, launched) = shown_equals_launched("https://example.org/статья?id=1");
    assert_eq!(
        launched,
        "https://example.org/%D1%81%D1%82%D0%B0%D1%82%D1%8C%D1%8F?id=1"
    );
    // Пробелы и обратные косые черты нормализуются до показа, а не после.
    let (_, launched) = shown_equals_launched("https://example.org\\a b");
    assert_eq!(launched, "https://example.org/a%20b");
    let (_, launched) = shown_equals_launched("  https://example.org/x  ");
    assert_eq!(launched, "https://example.org/x");
    // Строка для ОС — только ASCII: подменить вид адреса нечем.
    for url in [
        "https://пример.рф/путь?q=знач#фрагмент",
        "mailto:иван@пример.рф?subject=Привет",
    ] {
        let (_, launched) = shown_equals_launched(url);
        assert!(launched.is_ascii(), "{launched}");
    }
}

#[test]
fn unicode_domain_is_shown_together_with_punycode() {
    let (confirmation, launched) = shown_equals_launched("https://пример.рф/");
    assert_eq!(launched, "https://xn--e1afmkfd.xn--p1ai/");
    assert!(
        confirmation
            .message
            .lines()
            .any(|line| line == "Сайт: xn--e1afmkfd.xn--p1ai (пример.рф)"),
        "{}",
        confirmation.message
    );
    // Смешение алфавитов: «аpple» с кириллической «а» видно по punycode.
    let (confirmation, launched) = shown_equals_launched("https://аpple.com/");
    assert_eq!(launched, "https://xn--pple-43d.com/");
    assert!(
        confirmation
            .message
            .lines()
            .any(|line| line == "Сайт: xn--pple-43d.com (аpple.com)"),
        "{}",
        confirmation.message
    );
    // ASCII-домен — без скобок.
    let (confirmation, _) = shown_equals_launched("https://Example.ORG/");
    assert!(
        confirmation
            .message
            .lines()
            .any(|l| l == "Сайт: example.org")
    );
}

#[test]
fn formatting_and_control_characters_are_rejected() {
    for url in [
        "https://example.org/\u{202e}fdp.exe",
        "https://example.org/\u{200b}",
        "https://exa\u{200d}mple.org/",
        "https://example.org/\u{2066}a\u{2069}",
        "https://example.org/\u{feff}",
        "https://example.org/\u{ad}",
        "https://example.org/\u{e0041}",
        "https://example.org/\nhttps://evil.example",
        "https://example.org/\r",
        "https://example.org/\t",
        "https://example.org/\u{7f}",
        "https://example.org/\u{85}",
        "https://example.org/\u{2028}",
    ] {
        let seam = Seam::default();
        let error = seam.run("w-hidden", url, true).expect_err(url);
        assert_eq!(error, CoreError::invalid_argument("url"), "{url:?}");
        assert!(seam.asked.borrow().is_empty(), "{url:?}: окна не было");
        assert!(seam.launched.borrow().is_empty());
    }
}

#[test]
fn length_is_limited_to_8_kib_including_encoding() {
    let base = "https://example.org/";
    let fits = format!("{base}{}", "a".repeat(8 * 1024 - base.len()));
    assert_eq!(check_url(&fits), Ok(()));
    let over = format!("{base}{}", "a".repeat(8 * 1024 - base.len() + 1));
    assert_eq!(check_url(&over), Err(CoreError::invalid_argument("url")));
    // 1 500 кириллических букв — 3 000 байт на входе, но 9 000 после
    // процентного кодирования: в ОС ушло бы больше 8 КБ.
    let encoded = format!("{base}{}", "я".repeat(1500));
    assert!(encoded.len() < 8 * 1024);
    assert_eq!(check_url(&encoded), Err(CoreError::invalid_argument("url")));
}

#[test]
fn mailto_attachments_are_rejected() {
    for url in [
        "mailto:a@example.org?attach=C:\\Users\\me\\secret.txt",
        "mailto:a@example.org?subject=x&attachment=/etc/passwd",
        "mailto:a@example.org?ATTACH=x",
        "mailto:a@example.org?att%61ch=x",
        "mailto:?Attachment=file:///C:/x",
        "mailto:a@example.org?%20attach=x",
    ] {
        let seam = Seam::default();
        assert_eq!(
            seam.run("w-attach", url, true),
            Err(CoreError::invalid_argument("url")),
            "{url}"
        );
        assert!(seam.launched.borrow().is_empty());
    }
    let (_, launched) =
        shown_equals_launched("mailto:a@example.org?subject=Отчёт&cc=b@example.org&body=x");
    assert!(launched.starts_with("mailto:a@example.org?subject="));
}

#[test]
fn credentials_in_web_address_are_rejected() {
    for url in [
        "https://bank.example@evil.example/",
        "https://user:pass@example.org/",
        "http://:pass@example.org/",
    ] {
        assert_eq!(
            check_url(url),
            Err(CoreError::invalid_argument("url")),
            "{url}"
        );
    }
}

#[test]
fn dialog_texts_follow_mockup_e6b() {
    let (confirmation, _) = shown_equals_launched("https://example.org/");
    assert_eq!(confirmation.title, "Rhizomind — внешняя ссылка");
    assert_eq!(confirmation.open_label, "Открыть в браузере");
    assert_eq!(confirmation.cancel_label, "Отмена");
    assert!(
        confirmation
            .message
            .starts_with("Открыть ссылку в браузере?\n")
    );
    assert!(
        confirmation
            .message
            .contains("Сам Rhizomind в сеть не выходит.")
    );
    assert!(confirmation.message.contains("Полный адрес:\n"));
}

#[test]
fn only_one_core_dialog_per_window() {
    let outer = Seam::default();
    let inner = Seam::default();
    let mut nested = None;
    let outcome = open_checked(
        "w-busy",
        "https://example.org/first",
        |_| {
            // Пока первое окно открыто, тот же интерфейс зовёт снова.
            nested = Some(inner.run("w-busy", "https://example.org/second", true));
            // Другое окно приложения — не занято.
            assert_eq!(
                Seam::default().run("w-other", "https://example.org/", false),
                Ok(OpenExternalOutcome::Cancelled)
            );
            false
        },
        |target| {
            outer.launched.borrow_mut().push(target.to_owned());
            Ok(())
        },
    );
    assert_eq!(outcome, Ok(OpenExternalOutcome::Cancelled));
    assert_eq!(nested, Some(Ok(OpenExternalOutcome::Cancelled)));
    assert!(inner.asked.borrow().is_empty(), "второе окно не показано");
    assert!(
        inner.launched.borrow().is_empty(),
        "второй адрес не ушёл в ОС"
    );
    // После закрытия окна подтверждения окно приложения снова свободно.
    assert_eq!(
        Seam::default().run("w-busy", "https://example.org/", true),
        Ok(OpenExternalOutcome::Opened)
    );
}

#[test]
fn dialog_slot_is_released_after_panic_in_dialog() {
    let result = std::panic::catch_unwind(|| {
        let _ = open_checked(
            "w-panic",
            "https://example.org/",
            |_| panic!("окно упало"),
            |_| Ok(()),
        );
    });
    assert!(result.is_err());
    assert_eq!(
        Seam::default().run("w-panic", "https://example.org/", false),
        Ok(OpenExternalOutcome::Cancelled)
    );
    // `Cancelled` бывает и при занятом окне — проверяем, что окно показано.
    let seam = Seam::default();
    let _ = seam.run("w-panic", "https://example.org/", false);
    assert_eq!(seam.asked.borrow().len(), 1);
}

// ---- Права интерфейса (ADR 0009) -----------------------------------------

#[test]
fn capabilities_grant_no_opener_shell_fs_or_dialog() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut files = 0;
    for entry in std::fs::read_dir(&dir).expect("папка capabilities") {
        let path = entry.expect("элемент папки").path();
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        files += 1;
        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let permissions = json["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{}: нет permissions", path.display()));
        for permission in permissions {
            // Разрешение — строка или объект { "identifier": ... }.
            let id = permission
                .as_str()
                .or_else(|| permission["identifier"].as_str())
                .unwrap_or_else(|| panic!("{}: {permission}", path.display()));
            for banned in ["opener:", "shell:", "fs:", "dialog:"] {
                assert!(
                    !id.starts_with(banned),
                    "{}: интерфейсу выдано {id}",
                    path.display()
                );
            }
        }
        // И ни одного упоминания в любом месте файла (на случай новых форм записи).
        for banned in ["\"opener:", "\"shell:", "\"fs:", "\"dialog:"] {
            assert!(!text.contains(banned), "{}: {banned}", path.display());
        }
    }
    assert!(files > 0, "в {} нет файлов разрешений", dir.display());
}

// ---- Вызов через настоящий тип окна в тестах -----------------------------

/// Тесты крейта (в том числе тесты прав команд, которые вызывают
/// `open_external` с правильным адресом) не показывают системное окно и не
/// открывают браузер: ответ — `cancelled`, без зависания.
#[test]
fn tests_never_show_system_dialog_or_open_browser() {
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_dialog::init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("тестовое приложение");
        let webview = tauri::WebviewWindowBuilder::new(&app, "w-real", Default::default())
            .build()
            .expect("тестовое окно");
        let window = webview.as_ref().window();
        let _ = done.send(open_with_confirmation(&window, "https://example.org/"));
    });
    let outcome = result
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("open_with_confirmation завис");
    assert_eq!(outcome, Ok(OpenExternalOutcome::Cancelled));
}

// ---- Ревью 1.5 (docs/reviews/2026-10-25-task-1.5.md) ----------------------

/// Б1: в строке для ОС — только символы RFC 3986. Иначе `"`, пробел, `\`
/// в непрозрачном пути `mailto` попали бы в командную строку обработчика
/// (`OUTLOOK.EXE ... /m "%1"`) как его ключи.
#[test]
fn only_rfc3986_characters_reach_os() {
    for url in [
        "mailto:x\" /a \"C:\\Users\\me\\secret.docx",
        "mailto:a b@example.org",
        "mailto:a\\b@example.org",
        "mailto:a@example.org?subject=x^y",
        "https://example.org/?q=`calc`",
        "https://example.org/?q=a|b",
        "https://example.org/?q={x}",
    ] {
        let seam = Seam::default();
        assert_eq!(
            seam.run("w-rfc3986", url, true),
            Err(CoreError::invalid_argument("url")),
            "{url}"
        );
        assert!(seam.asked.borrow().is_empty(), "{url}: окна не было");
        assert!(seam.launched.borrow().is_empty(), "{url}: в ОС не ушло");
    }
    // Эти символы `url` сам кодирует процентами — в ОС они уходят безопасными.
    for (url, encoded) in [
        ("mailto:a@example.org?subject=a\"b", "%22"),
        ("https://example.org/#a\"b", "%22"),
        ("https://example.org/?q=<x>", "%3C"),
    ] {
        let (_, launched) = shown_equals_launched(url);
        assert!(launched.contains(encoded), "{launched}");
    }
    // Допустимые символы RFC 3986 проходят.
    for url in [
        "https://example.org/a-b.c_d~e/f?g=h&i=j;k,l+m*n!o$p'q(r)s#t:u@v[w]",
        "mailto:a.b+c@example.org?subject=%D0%9F&body=x%20y",
    ] {
        assert_eq!(check_url(url), Ok(()), "{url}");
    }
}

/// П3: управляющие символы после раскодирования в `mailto` — в ключах всегда,
/// в значениях всех параметров, кроме `body`.
#[test]
fn mailto_decoded_control_characters_are_rejected() {
    for url in [
        "mailto:a@example.org?att%00ach=x",
        "mailto:a@example.org?subject=%0D%0ABcc:evil@example.org",
        "mailto:a@example.org?cc=b@example.org%0A",
        "mailto:a@example.org?x%0Ay=1",
    ] {
        assert_eq!(
            check_url(url),
            Err(CoreError::invalid_argument("url")),
            "{url}"
        );
    }
    // Переводы строк в тексте письма — обычное дело.
    assert_eq!(
        check_url("mailto:a@example.org?subject=Hi&body=line1%0D%0Aline2"),
        Ok(())
    );
}

/// Подпись первой кнопки: «Открыть в почте» для `mailto`, «Открыть в
/// браузере» для `http(s)`.
#[test]
fn open_button_label_depends_on_scheme() {
    let (mail, _) = shown_equals_launched("mailto:a@example.org");
    assert_eq!(mail.open_label, "Открыть в почте");
    assert_eq!(mail.cancel_label, "Отмена");
    let (web, _) = shown_equals_launched("http://example.org/");
    assert_eq!(web.open_label, "Открыть в браузере");
}
