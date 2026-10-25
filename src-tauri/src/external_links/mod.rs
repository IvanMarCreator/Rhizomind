//! Внешние ссылки — интерфейс модуля, реализация в задаче 1.5.
//!
//! Спецификация 0.1, С3.7 и ADR 0009: только схемы `http`, `https`,
//! `mailto`; окно подтверждения с полным адресом показывает ядро
//! (`tauri-plugin-dialog` только из Rust), по умолчанию выбрана кнопка
//! «Открыть в браузере»; после согласия адрес передаётся ОС крейтом `open`.
//! У интерфейса нет ни прав `opener:*`, ни другой команды открытия, поэтому
//! обойти подтверждение он не может.

use crate::commands::{CoreError, CoreResult, OpenExternalOutcome};

/// Схемы, которые можно открыть.
pub const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Проверяет схему адреса; другая схема или неразборчивый адрес —
/// `scheme_not_allowed`. Заглушка 0.4: реализация — задача 1.5.
pub fn check_url(_url: &str) -> CoreResult<()> {
    Err(CoreError::not_implemented())
}

/// Проверяет адрес, показывает окно подтверждения ядра — модально к окну
/// `window`, из которого пришла ссылка, — и после согласия передаёт адрес
/// ОС. Блокирует поток до ответа пользователя. Длина и управляющие символы
/// уже проверены слоем команд (`commands::limits::url`).
pub fn open_with_confirmation<R: tauri::Runtime>(
    _window: &tauri::Window<R>,
    url: &str,
) -> CoreResult<OpenExternalOutcome> {
    check_url(url)?;
    Err(CoreError::not_implemented())
}
