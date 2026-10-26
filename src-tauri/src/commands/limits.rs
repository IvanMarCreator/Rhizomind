//! Пределы параметров команд ядра API v1 и их проверка.
//!
//! Проверяются в слое команд, до вызова модулей: модуль получает параметры,
//! уже уложенные в пределы. Нарушение — `invalid_argument` с именем поля.
//! Те же числа — в `src/api/limits.ts` и в снимке `contracts/v1/limits.snap`.

use serde::Serialize;

use super::error::{CoreError, CoreResult};
use super::settings::{NewNoteLocation, Settings};
use super::types::VaultPath;

const MIB: u64 = 1024 * 1024;

/// Не больше стольких целей в одном `links_resolve`.
pub const MAX_LINK_TARGETS: usize = 10_000;
/// Длина строки поиска (`links_suggest`, `switcher_search`), символов Unicode.
pub const MAX_QUERY_CHARS: usize = 256;
/// Длина одного компонента пути или имени, единиц UTF-16 (как в NTFS).
pub const MAX_COMPONENT_UTF16: usize = 255;
/// Компонентов в одном пути.
pub const MAX_PATH_COMPONENTS: usize = 64;
/// Жёсткий потолок текста `note_write`, байт UTF-8 (лимит из настроек — не больше него).
pub const MAX_NOTE_BYTES: u64 = 100 * MIB;
/// Раскладка окна после сериализации в JSON, байт.
pub const MAX_WORKSPACE_BYTES: usize = MIB as usize;
/// Адрес `open_external`, байт UTF-8.
pub const MAX_URL_BYTES: usize = 8 * 1024;
/// Полный путь к папке на диске (`vault_create.parent`), единиц UTF-16
/// (длинный путь Windows).
pub const MAX_FOLDER_PATH_UTF16: usize = 32_767;
/// Диапазон настройки «максимальный размер файла для редактирования», байт.
pub const EDIT_SIZE_RANGE: (u64, u64) = (MIB, MAX_NOTE_BYTES);
/// Диапазон размера шрифта текста, пиксели.
pub const FONT_SIZE_RANGE: (u16, u16) = (10, 32);

/// Пределы для снимка контракта.
#[derive(Debug, Serialize)]
pub struct Limits {
    pub max_link_targets: usize,
    pub max_query_chars: usize,
    pub max_component_utf16: usize,
    pub max_path_components: usize,
    pub max_note_bytes: u64,
    pub max_workspace_bytes: usize,
    pub max_url_bytes: usize,
    pub max_folder_path_utf16: usize,
    pub edit_size_range: (u64, u64),
    pub font_size_range: (u16, u16),
}

/// Все пределы одним значением.
pub const LIMITS: Limits = Limits {
    max_link_targets: MAX_LINK_TARGETS,
    max_query_chars: MAX_QUERY_CHARS,
    max_component_utf16: MAX_COMPONENT_UTF16,
    max_path_components: MAX_PATH_COMPONENTS,
    max_note_bytes: MAX_NOTE_BYTES,
    max_workspace_bytes: MAX_WORKSPACE_BYTES,
    max_url_bytes: MAX_URL_BYTES,
    max_folder_path_utf16: MAX_FOLDER_PATH_UTF16,
    edit_size_range: EDIT_SIZE_RANGE,
    font_size_range: FONT_SIZE_RANGE,
};

fn ensure(ok: bool, field: &str) -> CoreResult<()> {
    if ok {
        Ok(())
    } else {
        Err(CoreError::invalid_argument(field))
    }
}

/// Имя или компонент пути укладывается в предел NTFS.
pub fn component_fits(component: &str) -> bool {
    component.encode_utf16().count() <= MAX_COMPONENT_UTF16
}

/// Путь: не больше [`MAX_PATH_COMPONENTS`] компонентов, каждый — не длиннее
/// [`MAX_COMPONENT_UTF16`]. Правила самих компонентов — `VaultPath::check`
/// в модуле `vault`.
pub fn path(path: &VaultPath, field: &str) -> CoreResult<()> {
    let components = path.components();
    ensure(
        components.len() <= MAX_PATH_COMPONENTS && components.iter().all(|c| component_fits(c)),
        field,
    )
}

/// Имя элемента (одно имя, не путь).
pub fn name(name: &str, field: &str) -> CoreResult<()> {
    ensure(component_fits(name), field)
}

/// Полный путь к папке на диске: длина и без управляющих символов.
pub fn folder_path(path: &str, field: &str) -> CoreResult<()> {
    ensure(
        !path.is_empty()
            && path.encode_utf16().count() <= MAX_FOLDER_PATH_UTF16
            && !path.chars().any(char::is_control),
        field,
    )
}

/// Строка поиска.
pub fn query(query: &str) -> CoreResult<()> {
    ensure(query.chars().count() <= MAX_QUERY_CHARS, "query")
}

/// Число целей `links_resolve`.
pub fn link_targets(count: usize) -> CoreResult<()> {
    ensure(count <= MAX_LINK_TARGETS, "targets")
}

/// Текст заметки: не больше `limit` (из настроек) и жёсткого потолка.
pub fn note_text(text: &str, limit: u64) -> CoreResult<()> {
    let bytes = text.len() as u64;
    ensure(bytes <= limit.min(MAX_NOTE_BYTES), "text")
}

/// Раскладка окна после сериализации.
pub fn workspace_bytes(bytes: usize) -> CoreResult<()> {
    ensure(bytes <= MAX_WORKSPACE_BYTES, "workspace")
}

/// Адрес внешней ссылки: длина и отсутствие управляющих символов (в том
/// числе переносов строк, которые могли бы спрятать часть адреса в окне
/// подтверждения).
pub fn url(url: &str) -> CoreResult<()> {
    ensure(
        url.len() <= MAX_URL_BYTES && !url.chars().any(char::is_control),
        "url",
    )
}

/// Значения настроек в своих диапазонах; путь папки новых заметок — как путь.
pub fn settings(settings: &Settings) -> CoreResult<()> {
    let (min_edit, max_edit) = EDIT_SIZE_RANGE;
    ensure(
        (min_edit..=max_edit).contains(&settings.max_edit_size_bytes),
        "settings.max_edit_size_bytes",
    )?;
    let (min_font, max_font) = FONT_SIZE_RANGE;
    ensure(
        (min_font..=max_font).contains(&settings.font_size),
        "settings.font_size",
    )?;
    if let NewNoteLocation::Folder { path: folder } = &settings.new_note_location {
        path(folder, "settings.new_note_location.path")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{EditorMode, ErrorCode, TextWidth, Theme};

    fn field_of(result: CoreResult<()>) -> Option<String> {
        let error = result.expect_err("ожидался отказ");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        error.field
    }

    fn valid_settings() -> Settings {
        Settings {
            theme: Theme::System,
            editor_mode: EditorMode::LivePreview,
            text_width: TextWidth::Column,
            font_size: 16,
            new_note_location: NewNoteLocation::VaultRoot,
            confirm_delete: true,
            max_edit_size_bytes: 10 * MIB,
        }
    }

    #[test]
    fn path_limits_count_components_and_utf16_length() {
        let long = "я".repeat(255);
        assert!(path(&VaultPath::from_components([long.as_str()]), "path").is_ok());
        let too_long = "я".repeat(256);
        assert_eq!(
            field_of(path(
                &VaultPath::from_components([too_long.as_str()]),
                "dir"
            )),
            Some("dir".into())
        );
        // Символ вне BMP занимает две единицы UTF-16.
        let emoji = "😀".repeat(128);
        assert!(path(&VaultPath::from_components([emoji.as_str()]), "path").is_err());
        let deep = VaultPath::from_components(std::iter::repeat_n("a", 65));
        assert!(path(&deep, "path").is_err());
        let ok_depth = VaultPath::from_components(std::iter::repeat_n("a", 64));
        assert!(path(&ok_depth, "path").is_ok());
    }

    #[test]
    fn query_counts_unicode_characters() {
        assert!(query(&"ж".repeat(256)).is_ok());
        assert_eq!(field_of(query(&"ж".repeat(257))), Some("query".into()));
    }

    #[test]
    fn link_targets_up_to_ten_thousand() {
        assert!(link_targets(10_000).is_ok());
        assert_eq!(field_of(link_targets(10_001)), Some("targets".into()));
    }

    #[test]
    fn note_text_respects_settings_limit_and_hard_ceiling() {
        assert!(note_text("абв", 6).is_ok());
        assert!(note_text("абв", 5).is_err());
        let huge_limit = u64::MAX;
        assert!(note_text("a", huge_limit).is_ok());
    }

    #[test]
    fn workspace_up_to_one_mebibyte() {
        assert!(workspace_bytes(1024 * 1024).is_ok());
        assert!(workspace_bytes(1024 * 1024 + 1).is_err());
    }

    #[test]
    fn url_rejects_control_characters_and_overlong() {
        assert!(url("https://example.org/a?b=1").is_ok());
        assert!(url("https://example.org/\nhttps://evil.example").is_err());
        assert!(
            url("https://example.org/\u{202e}").is_ok(),
            "не управляющий, а форматирующий — проверка 1.5"
        );
        assert!(url("https://example.org/\u{7f}").is_err());
        assert!(url(&format!("https://example.org/{}", "a".repeat(8 * 1024))).is_err());
    }

    #[test]
    fn settings_ranges() {
        assert!(settings(&valid_settings()).is_ok());
        let small = Settings {
            max_edit_size_bytes: MIB - 1,
            ..valid_settings()
        };
        assert_eq!(
            field_of(settings(&small)),
            Some("settings.max_edit_size_bytes".into())
        );
        let big = Settings {
            max_edit_size_bytes: 100 * MIB + 1,
            ..valid_settings()
        };
        assert!(settings(&big).is_err());
        let font = Settings {
            font_size: 9,
            ..valid_settings()
        };
        assert_eq!(field_of(settings(&font)), Some("settings.font_size".into()));
        let folder = Settings {
            new_note_location: NewNoteLocation::Folder {
                path: VaultPath::from_components(std::iter::repeat_n("a", 65)),
            },
            ..valid_settings()
        };
        assert_eq!(
            field_of(settings(&folder)),
            Some("settings.new_note_location.path".into())
        );
    }
}
