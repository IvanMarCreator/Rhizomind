//! Адрес внешней ссылки: разбор крейтом `url`, проверки и строка для ОС.
//!
//! В ОС уходит сериализация `url` (`Url::as_str`): только ASCII, домен в
//! punycode, остальное — процентным кодированием. Ровно эта строка
//! показывается в окне подтверждения, поэтому скрытые символы (`U+202E`,
//! нулевой ширины и т.п.) не могут исказить то, что видит пользователь.

use url::Url;

use super::ALLOWED_SCHEMES;
use crate::commands::{CoreError, CoreResult, ErrorCode};

/// Предел длины строки, которая уйдёт в ОС, байт (как `MAX_URL_BYTES` у
/// слоя команд: процентное кодирование может удлинить адрес).
const MAX_TARGET_BYTES: usize = crate::commands::limits::MAX_URL_BYTES;

/// Проверенный адрес внешней ссылки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAddress {
    url: Url,
}

impl ExternalAddress {
    /// Разбирает и проверяет адрес.
    ///
    /// - не `http`/`https`/`mailto` или неразборчивый адрес — `scheme_not_allowed`;
    /// - управляющие и форматирующие символы (`U+202E`, `U+200B` и т.п.),
    ///   длина больше 8 КБ (на входе и после кодирования), символы вне
    ///   RFC 3986 в итоговой строке, параметр `attach`/`attachment` у
    ///   `mailto`, управляющие символы после раскодирования в ключах и
    ///   значениях `mailto` (в `body` — кроме `\r`, `\n`, `\t`), `%` не в
    ///   виде `%HH`, имя пользователя или пароль в `http(s)` —
    ///   `invalid_argument` (`url`).
    pub fn parse(input: &str) -> CoreResult<Self> {
        if input.len() > MAX_TARGET_BYTES || input.chars().any(is_hidden) {
            return Err(CoreError::invalid_argument("url"));
        }
        let url = Url::parse(input).map_err(|_| scheme_not_allowed())?;
        if !ALLOWED_SCHEMES.contains(&url.scheme()) {
            return Err(scheme_not_allowed());
        }
        if url.scheme() == "mailto" {
            let bad_parameter = url.query_pairs().any(|(key, value)| {
                is_attachment(&key)
                    || key.chars().any(char::is_control)
                    || if key.eq_ignore_ascii_case("body") {
                        // В тексте письма — переносы строк и табуляция.
                        value
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
                    } else {
                        value.chars().any(char::is_control)
                    }
            });
            if bad_parameter {
                return Err(CoreError::invalid_argument("url"));
            }
        } else if !url.username().is_empty() || url.password().is_some() {
            // `https://bank.example@evil.example/` — домен за «@» легко не заметить.
            return Err(CoreError::invalid_argument("url"));
        }
        // У `mailto` путь непрозрачный: `url` не кодирует в нём `"`, пробел и
        // `\`, а обработчик почты получает адрес в своей командной строке
        // (`OUTLOOK.EXE ... /m "%1"`). Поэтому в ОС — только символы RFC 3986.
        // `%` — только как `%HH`: одиночный знак процента обработчик может
        // раскодировать по-своему.
        if url.as_str().len() > MAX_TARGET_BYTES
            || !url.as_str().chars().all(is_rfc3986)
            || !percent_escapes_are_complete(url.as_str())
        {
            return Err(CoreError::invalid_argument("url"));
        }
        Ok(Self { url })
    }

    /// Ровно та строка, которая уйдёт в ОС и показывается в окне подтверждения.
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// Это адрес почты (`mailto:`).
    pub fn is_mail(&self) -> bool {
        self.url.scheme() == "mailto"
    }

    /// Домен для отдельной строки окна: у `http(s)` — домен как в адресе
    /// (punycode); если у него есть запись в Unicode — «punycode (unicode)».
    pub fn host_line(&self) -> Option<String> {
        let ascii = self.url.host_str()?;
        if self.is_mail() {
            return None;
        }
        let unicode = url::quirks::domain_to_unicode(ascii);
        if unicode.is_empty() || unicode == ascii || unicode.chars().any(is_hidden) {
            Some(ascii.to_owned())
        } else {
            // Первым — punycode, ровно как в адресе; Unicode — подсказкой.
            Some(format!("{ascii} ({unicode})"))
        }
    }
}

/// Каждый `%` в строке — начало `%HH` (две шестнадцатеричные цифры).
fn percent_escapes_are_complete(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(i, &b)| {
        b != b'%'
            || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    })
}

/// Символ из RFC 3986: незарезервированные, зарезервированные и `%`.
fn is_rfc3986(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

fn scheme_not_allowed() -> CoreError {
    CoreError::new(ErrorCode::SchemeNotAllowed)
}

/// Параметр вложения у `mailto`: `attach`, `attachment` и их варианты
/// регистра и кодирования (`Attach`, `att%61ch`, ` attach`).
fn is_attachment(key: &str) -> bool {
    key.to_lowercase().contains("attach")
}

/// Управляющие символы и невидимые форматирующие (категория Unicode Cf и
/// близкие): переопределение направления текста, нулевая ширина, мягкий
/// перенос, метки языка.
fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{17B4}'
                | '\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF0}'..='\u{FFFB}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
        )
}
