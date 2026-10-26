//! Ошибки команд ядра: машинный код и ключ словаря интерфейса.
//!
//! Интерфейс получает ошибку как отклонённое обещание с объектом
//! `{ "code": "...", "message_key": "errors....", ... }`. По `code` интерфейс
//! решает, что делать; по `message_key` берёт текст из словаря `ru`
//! (`src/i18n/`, задача 2.2). Текстов для человека ядро не отправляет, и в
//! ошибках нет ни содержимого заметок, ни путей вне хранилища (инварианты 2, 3).

use serde::{Deserialize, Serialize};

/// Результат операций ядра.
pub type CoreResult<T> = Result<T, CoreError>;

/// Машинный код ошибки. Набор закрыт: новый код — изменение контракта
/// (снимки `contracts/v1/`, типы `src/api/types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Модуль ядра ещё не реализован (заглушки контракта, задача 0.4).
    NotImplemented,
    /// Параметр не прошёл проверку типа или границ; поле — в `field`.
    InvalidArgument,
    /// В этом окне не открыто хранилище.
    NoVaultOpen,
    /// Хранилища с таким идентификатором нет в списке хранилищ.
    VaultNotFound,
    /// Папка хранилища недоступна (удалена, отключён диск).
    VaultUnavailable,
    /// Путь отклонён проверкой компонентов (`..`, разделители,
    /// зарезервированные имена Windows и т.п.).
    InvalidPath,
    /// Путь выводит за контур хранилища (символическая ссылка, точка соединения).
    OutsideVault,
    /// Файла или папки нет.
    NotFound,
    /// Файл или папка с таким именем уже есть (сравнение по ключу имени).
    AlreadyExists,
    /// Имя не прошло правила имён; причина — в `name_problem`.
    InvalidName,
    /// Операция только для заметок (`.md`), а путь ведёт на другой файл или папку.
    NotANote,
    /// Заметка открыта только для чтения (больше лимита или не UTF-8) — запись отклонена.
    ReadOnly,
    /// Внешняя ссылка со схемой не из списка `http`, `https`, `mailto`.
    SchemeNotAllowed,
    /// Ошибка ввода-вывода ОС. Подробности — только в журнале ошибок ядра.
    IoFailed,
    /// Папка лежит внутри другого хранилища или его служебной папки: новое
    /// хранилище там создать нельзя (задача 2.8б).
    NestedVault,
}

impl ErrorCode {
    /// Все коды — для тестов полноты словаря.
    pub const ALL: [ErrorCode; 15] = [
        Self::NotImplemented,
        Self::InvalidArgument,
        Self::NoVaultOpen,
        Self::VaultNotFound,
        Self::VaultUnavailable,
        Self::InvalidPath,
        Self::OutsideVault,
        Self::NotFound,
        Self::AlreadyExists,
        Self::InvalidName,
        Self::NotANote,
        Self::ReadOnly,
        Self::SchemeNotAllowed,
        Self::IoFailed,
        Self::NestedVault,
    ];

    /// Ключ словаря интерфейса: `errors.<код>`.
    pub fn message_key(self) -> String {
        format!("errors.{}", self.as_str())
    }

    /// Машинный код строкой, как в JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::InvalidArgument => "invalid_argument",
            Self::NoVaultOpen => "no_vault_open",
            Self::VaultNotFound => "vault_not_found",
            Self::VaultUnavailable => "vault_unavailable",
            Self::InvalidPath => "invalid_path",
            Self::OutsideVault => "outside_vault",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::InvalidName => "invalid_name",
            Self::NotANote => "not_a_note",
            Self::ReadOnly => "read_only",
            Self::SchemeNotAllowed => "scheme_not_allowed",
            Self::IoFailed => "io_failed",
            Self::NestedVault => "nested_vault",
        }
    }
}

/// Почему имя файла или папки не подходит (спецификация 0.1, С3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameProblem {
    /// Пустое имя.
    Empty,
    /// Запрещённые в Windows символы `\ / : * ? " < > |` или управляющие.
    ForbiddenChars,
    /// Зарезервированное имя Windows (`CON`, `NUL`, `COM1`…, в том числе `nul.md`).
    ReservedName,
    /// Точка или пробел в конце.
    TrailingDotOrSpace,
    /// Слишком длинное имя.
    TooLong,
    /// В папке уже есть элемент с таким же ключом имени (без учёта регистра, NFC).
    Duplicate,
    /// Переименование меняет вид элемента: заметка перестала бы быть `.md`
    /// или другой файл стал бы заметкой.
    NoteExtension,
}

impl NameProblem {
    /// Все причины — для тестов полноты словаря.
    pub const ALL: [NameProblem; 7] = [
        Self::Empty,
        Self::ForbiddenChars,
        Self::ReservedName,
        Self::TrailingDotOrSpace,
        Self::TooLong,
        Self::Duplicate,
        Self::NoteExtension,
    ];

    /// Ключ словаря с пояснением под полем ввода: `names.<причина>`.
    pub fn message_key(self) -> String {
        let name = match self {
            Self::Empty => "empty",
            Self::ForbiddenChars => "forbidden_chars",
            Self::ReservedName => "reserved_name",
            Self::TrailingDotOrSpace => "trailing_dot_or_space",
            Self::TooLong => "too_long",
            Self::Duplicate => "duplicate",
            Self::NoteExtension => "note_extension",
        };
        format!("names.{name}")
    }
}

/// Ошибка команды ядра в том виде, в котором её получает интерфейс.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreError {
    /// Машинный код.
    pub code: ErrorCode,
    /// Ключ словаря интерфейса, всегда `code.message_key()`.
    pub message_key: String,
    /// Для `invalid_argument`: имя параметра запроса.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Для `invalid_name`: причина.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_problem: Option<NameProblem>,
}

impl CoreError {
    /// Ошибка с кодом и без подробностей.
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            message_key: code.message_key(),
            field: None,
            name_problem: None,
        }
    }

    /// Модуль ядра ещё не реализован.
    pub fn not_implemented() -> Self {
        Self::new(ErrorCode::NotImplemented)
    }

    /// Параметр `field` не прошёл проверку.
    pub fn invalid_argument(field: &str) -> Self {
        Self {
            field: Some(field.to_owned()),
            ..Self::new(ErrorCode::InvalidArgument)
        }
    }

    /// Имя не прошло правила имён.
    pub fn invalid_name(problem: NameProblem) -> Self {
        Self {
            name_problem: Some(problem),
            ..Self::new(ErrorCode::InvalidName)
        }
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code.as_str())
    }
}

impl std::error::Error for CoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_key_follows_code_as_serialized_in_json() {
        for code in ErrorCode::ALL {
            let json = serde_json::to_value(code).expect("код");
            assert_eq!(json, code.as_str());
            assert_eq!(code.message_key(), format!("errors.{}", code.as_str()));
        }
    }

    #[test]
    fn name_problem_key_follows_json_name() {
        for problem in NameProblem::ALL {
            let json = serde_json::to_value(problem).expect("причина");
            assert_eq!(
                problem.message_key(),
                format!("names.{}", json.as_str().expect("строка"))
            );
        }
    }

    #[test]
    fn error_without_details_has_only_code_and_key() {
        assert_eq!(
            serde_json::to_value(CoreError::not_implemented()).expect("ошибка"),
            serde_json::json!({ "code": "not_implemented", "message_key": "errors.not_implemented" })
        );
    }
}
