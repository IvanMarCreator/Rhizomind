//! Передача подтверждённого адреса ОС (заглушка — красный тест).

use std::io;

pub(super) fn scheme_of(_target: &str) -> io::Result<&str> {
    Err(io::Error::other("не реализовано"))
}

pub(super) fn launch(target: &str) -> io::Result<()> {
    open::that_detached(target)
}
