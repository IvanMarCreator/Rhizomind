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
        assert_eq!(check_url(url).unwrap_err().code, ErrorCode::SchemeNotAllowed);
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
