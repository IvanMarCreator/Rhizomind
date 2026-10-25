//! Служебные файлы хранилища: `.rhizomind/settings.json` с версией формата и
//! терпимым чтением.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::commands::{NewNoteLocation, Settings, limits};

/// Версия формата `settings.json`, которую пишет это ядро.
pub const SETTINGS_FORMAT_VERSION: u32 = 1;

/// `.rhizomind/settings.json` на диске — отдельно от типа контракта
/// [`Settings`]. Чтение терпимое: неизвестные поля, значения, которых это
/// ядро не знает, и файл более новой версии не ломают чтение — непонятное
/// значение заменяется значением по умолчанию, а неизвестные поля
/// сохраняются при записи. Версию формата ставит ядро.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    /// Версия формата файла. Читается терпимо: строка, дробь, отрицательное
    /// число или `null` — 0 (файл записан неизвестно кем, читаем по
    /// известным полям).
    #[serde(default, deserialize_with = "lenient_version")]
    pub format_version: u32,
    /// Все остальные поля как есть.
    #[serde(flatten)]
    pub values: Map<String, Value>,
}

/// Версия формата из любого значения JSON: неотрицательное целое в пределах
/// `u32` — как есть, остальное — 0.
fn lenient_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0))
}

impl SettingsFile {
    /// Файл с диска; не JSON-объект — `None` (вызывающий берёт значения по
    /// умолчанию).
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match serde_json::from_slice::<Value>(bytes).ok()? {
            object @ Value::Object(_) => serde_json::from_value(object).ok(),
            _ => None,
        }
    }

    /// Настройки для интерфейса: известные поля, остальное — по умолчанию.
    pub fn to_settings(&self) -> Settings {
        fn field<T: serde::de::DeserializeOwned>(
            values: &Map<String, Value>,
            key: &str,
        ) -> Option<T> {
            values
                .get(key)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }
        let defaults = Settings::default();
        // Значение вне диапазона (файл правили руками) — по умолчанию.
        let (min_font, max_font) = limits::FONT_SIZE_RANGE;
        let (min_edit, max_edit) = limits::EDIT_SIZE_RANGE;
        let new_note_location = field(&self.values, "new_note_location")
            .filter(|location| match location {
                NewNoteLocation::Folder { path } => limits::path(path, "path").is_ok(),
                _ => true,
            })
            .unwrap_or_else(|| defaults.new_note_location.clone());
        Settings {
            theme: field(&self.values, "theme").unwrap_or(defaults.theme),
            editor_mode: field(&self.values, "editor_mode").unwrap_or(defaults.editor_mode),
            text_width: field(&self.values, "text_width").unwrap_or(defaults.text_width),
            font_size: field(&self.values, "font_size")
                .filter(|size| (min_font..=max_font).contains(size))
                .unwrap_or(defaults.font_size),
            new_note_location,
            confirm_delete: field(&self.values, "confirm_delete")
                .unwrap_or(defaults.confirm_delete),
            max_edit_size_bytes: field(&self.values, "max_edit_size_bytes")
                .filter(|size| (min_edit..=max_edit).contains(size))
                .unwrap_or(defaults.max_edit_size_bytes),
        }
    }

    /// Записывает настройки поверх файла: известные поля заменяются,
    /// неизвестные сохраняются, версия формата — [`SETTINGS_FORMAT_VERSION`]
    /// (не ниже прочитанной: более новый файл не понижается).
    ///
    /// Значение на диске, которое это ядро не понимает (`"theme": "sepia"`
    /// из более новой версии), не затирается, пока пользователь не выбрал
    /// в этой настройке другое: интерфейс получил вместо него значение по
    /// умолчанию и вернул его же.
    pub fn update(&mut self, settings: &Settings) {
        let presented = serde_json::to_value(self.to_settings()).ok();
        if let Ok(Value::Object(known)) = serde_json::to_value(settings) {
            for (key, value) in known {
                let unchanged = presented.as_ref().and_then(|p| p.get(&key)) == Some(&value);
                if unchanged && self.values.contains_key(&key) {
                    continue;
                }
                self.values.insert(key, value);
            }
        }
        self.format_version = self.format_version.max(SETTINGS_FORMAT_VERSION);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Theme;
    use serde_json::json;

    #[test]
    fn newer_settings_file_with_unknown_fields_and_values_still_reads() {
        let file: SettingsFile = serde_json::from_value(json!({
            "format_version": 2,
            "theme": "sepia",
            "font_size": 18,
            "graph": { "physics": true }
        }))
        .expect("файл следующей версии читается");

        let settings = file.to_settings();

        assert_eq!(
            settings.theme,
            Theme::System,
            "неизвестная тема — по умолчанию"
        );
        assert_eq!(settings.font_size, 18);
        assert_eq!(settings.new_note_location, NewNoteLocation::VaultRoot);
    }

    #[test]
    fn update_keeps_unknown_fields_and_sets_format_version() {
        let mut file: SettingsFile =
            serde_json::from_value(json!({ "graph": { "physics": true } })).expect("файл");

        file.update(&Settings {
            theme: Theme::Dark,
            ..Settings::default()
        });

        let written = serde_json::to_value(&file).expect("запись");
        assert_eq!(written["format_version"], 1);
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["graph"], json!({ "physics": true }));
    }

    #[test]
    fn out_of_range_values_fall_back_to_defaults() {
        let file: SettingsFile =
            serde_json::from_value(json!({ "max_edit_size_bytes": 1, "theme": "dark" }))
                .expect("файл");
        let settings = file.to_settings();
        assert_eq!(
            settings.max_edit_size_bytes,
            Settings::default().max_edit_size_bytes
        );
        assert_eq!(settings.theme, Theme::Dark);
    }

    #[test]
    fn missing_file_content_gives_defaults() {
        assert_eq!(SettingsFile::default().to_settings(), Settings::default());
    }
}
