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
    ///   длина больше 8 КБ, параметр `attach`/`attachment` у `mailto`,
    ///   имя пользователя или пароль в `http(s)` — `invalid_argument` (`url`).
    pub fn parse(input: &str) -> CoreResult<Self> {
        if input.len() > MAX_TARGET_BYTES || input.chars().any(is_hidden) {
            return Err(CoreError::invalid_argument("url"));
        }
        let url = Url::parse(input).map_err(|_| scheme_not_allowed())?;
        if !ALLOWED_SCHEMES.contains(&url.scheme()) {
            return Err(scheme_not_allowed());
        }
        if url.scheme() == "mailto" {
            if url.query_pairs().any(|(key, _)| is_attachment(&key)) {
                return Err(CoreError::invalid_argument("url"));
            }
        } else if !url.username().is_empty() || url.password().is_some() {
            // `https://bank.example@evil.example/` — домен за «@» легко не заметить.
            return Err(CoreError::invalid_argument("url"));
        }
        if url.as_str().len() > MAX_TARGET_BYTES {
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
    /// (punycode); если у него есть запись в Unicode — «unicode (punycode)».
    pub fn host_line(&self) -> Option<String> {
        let ascii = self.url.host_str()?;
        if self.is_mail() {
            return None;
        }
        let unicode = url::quirks::domain_to_unicode(ascii);
        if unicode.is_empty() || unicode == ascii || unicode.chars().any(is_hidden) {
            Some(ascii.to_owned())
        } else {
            Some(format!("{unicode} ({ascii})"))
        }
    }
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
