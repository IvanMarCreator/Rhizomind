//! Хранилище: контур, файлы и служебные данные — интерфейс модуля,
//! реализация в задаче 1.2.
//!
//! Модуль — единственное место ядра, которое обращается к файлам хранилища.
//! Всё, что приходит от интерфейса, проходит здесь один порядок проверки
//! контура (исследование крейтов, раздел «Контур хранилища»):
//! 1. каждый компонент пути — [`check_component`];
//! 2. корень канонизирован один раз в [`Vault::open`] (`dunce::canonicalize`);
//! 3. каждый промежуточный компонент — `symlink_metadata`: символическая
//!    ссылка или точка соединения NTFS → `outside_vault`;
//! 4. итоговый путь начинается с корня.
//!
//! Запись заметки атомарная (временный файл в `.rhizomind/tmp/` → замена),
//! с проверкой ожидаемой версии файла. Удаление — в корзину ОС; на томе без
//! корзины — безвозвратно и только после отдельного подтверждения
//! (инвариант 4). Служебные данные — `.rhizomind/settings.json`
//! ([`SettingsFile`]) и `.rhizomind/workspace.json` с версией формата.
//!
//! **Дата создания заметки** — источник «самой старой» заметки при
//! дубликатах имён (С3.4, [`NoteRef::created_ms`]). Берётся из файловой
//! системы, поэтому атомарная запись обязана её сохранять: на Windows
//! замена через `ReplaceFileW` (сохраняет дату создания и идентификатор
//! файла), на других ОС — перенос времени создания на новый файл, где ФС
//! это позволяет (macOS — `setattrlist`; на Linux времени создания обычно не
//! задать — там дата создания после записи может сдвинуться, это известное
//! ограничение). Это уточнение к ADR 0006, раздел «Последствия»: ADR нужно
//! дописать, выбор сделан в пользу `ReplaceFileW`, а не «даты появления».

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::commands::{
    CoreError, CoreResult, FileVersion, NameCheck, NewNoteLocation, NoteReadResponse,
    NoteWriteResponse, Settings, TrashOutcome, TreeEntry, VaultPath, Workspace, limits,
};

/// Заметка в списке, по которому разрешаются ссылки и идёт быстрый переход.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    /// Путь заметки (с `.md`).
    pub path: VaultPath,
    /// Дата создания файла, миллисекунды UTC — для выбора самой старой
    /// заметки при дубликатах имён.
    pub created_ms: i64,
}

/// Открытое хранилище: корень и операции над файлами внутри контура.
///
/// Все методы принимают пути [`VaultPath`] от интерфейса как есть и сами
/// проверяют их компоненты; пределы длины уже проверены слоем команд.
/// Методы блокируют поток (диск, корзина) — команды вызывают их вне
/// главного потока.
#[derive(Debug)]
pub struct Vault {
    #[allow(dead_code)]
    root: PathBuf,
}

impl Vault {
    /// Создаёт папку нового хранилища `parent/name` (не существующую).
    /// Имя уже проверено правилами имён и [`check_component`]; возвращает
    /// путь к созданной папке. `.rhizomind/` создаёт [`Vault::open`].
    pub fn create(_parent: &Path, _name: &str) -> CoreResult<PathBuf> {
        Err(CoreError::not_implemented())
    }

    /// Открывает папку как хранилище: канонизирует корень, создаёт
    /// `.rhizomind/` и `.rhizomind/tmp/`, убирает остатки временных файлов.
    ///
    /// Заглушка 0.4 только запоминает путь и к диску не обращается.
    pub fn open(root: &Path) -> CoreResult<Self> {
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Все папки и файлы хранилища, кроме `.rhizomind/` и имён с точки в
    /// начале. Плоский список; дерево и сортировку строит интерфейс.
    pub fn tree(&self) -> CoreResult<Vec<TreeEntry>> {
        Err(CoreError::not_implemented())
    }

    /// Все заметки хранилища с датой создания — для модулей `links` и
    /// `switcher`. «Самая старая» из заметок с одним именем — с наименьшей
    /// `created_ms`; при равной дате — первая по пути в естественном
    /// алфавитном порядке (С3.4).
    pub fn notes(&self) -> CoreResult<Vec<NoteRef>> {
        Err(CoreError::not_implemented())
    }

    /// Читает заметку: текст без BOM с переносами `\n`, версию файла.
    /// Больше лимита из настроек или не UTF-8 — только чтение.
    pub fn read_note(&self, _path: &VaultPath) -> CoreResult<NoteReadResponse> {
        Err(CoreError::not_implemented())
    }

    /// Записывает заметку атомарно по правилу «байт в байт»
    /// (`commands::note`): BOM и переносы — из файла на диске; текст не
    /// изменился — файл не пишется. Версия на диске не равна `expected` —
    /// изменения снаружи не затираются: версия с диска сохраняется копией
    /// рядом, текст из редактора — в заметку (`conflict_saved`, С3.6).
    /// Файла нет — `not_found`. Дата создания сохраняется (см. описание
    /// модуля).
    pub fn write_note(
        &self,
        _path: &VaultPath,
        _text: &str,
        _expected: &FileVersion,
    ) -> CoreResult<NoteWriteResponse> {
        Err(CoreError::not_implemented())
    }

    /// Создаёт пустую заметку `name` (полное имя с `.md`; UTF-8 без BOM,
    /// переносы LF) в папке `dir`.
    pub fn create_note(&self, _dir: &VaultPath, _name: &str) -> CoreResult<VaultPath> {
        Err(CoreError::not_implemented())
    }

    /// Создаёт папку `name` в папке `dir`.
    pub fn create_folder(&self, _dir: &VaultPath, _name: &str) -> CoreResult<VaultPath> {
        Err(CoreError::not_implemented())
    }

    /// Совпадение имени с соседями в папке `dir` по ключу имени
    /// (`names::key`); правила самого имени уже проверены слоем команд.
    /// `renaming` — переименовываемый элемент: он не совпадение сам с собой,
    /// поэтому смена только регистра разрешена.
    pub fn check_new_name(
        &self,
        _dir: &VaultPath,
        _name: &str,
        _renaming: Option<&VaultPath>,
    ) -> CoreResult<NameCheck> {
        Err(CoreError::not_implemented())
    }

    /// Переименовывает файл или папку; возвращает новый путь. Смена только
    /// регистра разрешена (на Windows — через промежуточное имя).
    pub fn rename(&self, _path: &VaultPath, _new_name: &str) -> CoreResult<VaultPath> {
        Err(CoreError::not_implemented())
    }

    /// Перемещает файл или папку в `new_dir`; возвращает новый путь.
    pub fn move_entry(&self, _path: &VaultPath, _new_dir: &VaultPath) -> CoreResult<VaultPath> {
        Err(CoreError::not_implemented())
    }

    /// Удаляет в корзину ОС. На томе без корзины спрашивает
    /// `confirm_permanent` (окно подтверждения ядра) и удаляет безвозвратно
    /// только при согласии.
    pub fn trash(
        &self,
        _path: &VaultPath,
        _confirm_permanent: &dyn Fn(&VaultPath) -> bool,
    ) -> CoreResult<TrashOutcome> {
        Err(CoreError::not_implemented())
    }

    /// Настройки хранилища из `.rhizomind/settings.json`
    /// ([`SettingsFile::to_settings`]); файла нет — значения по умолчанию.
    pub fn read_settings(&self) -> CoreResult<Settings> {
        Err(CoreError::not_implemented())
    }

    /// Записывает настройки хранилища атомарно ([`SettingsFile::update`]).
    pub fn write_settings(&self, _settings: &Settings) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }

    /// Раскладка окна из `.rhizomind/workspace.json`; файла нет — `None`.
    pub fn read_workspace(&self) -> CoreResult<Option<Workspace>> {
        Err(CoreError::not_implemented())
    }

    /// Записывает раскладку окна атомарно.
    pub fn write_workspace(&self, _workspace: &Workspace) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }
}

/// Проверка одного компонента пути от интерфейса: нет `..`, `.`, `:`,
/// разделителей `/` и `\`, NUL и управляющих символов, зарезервированных имён
/// Windows (в том числе с расширением: `nul.md`), точки или пробела в конце;
/// не пустой. Ошибка — `invalid_path`.
///
/// Заглушка 0.4: правила — задача 1.2.
pub fn check_component(_component: &str) -> CoreResult<()> {
    Err(CoreError::not_implemented())
}

/// Версия формата `settings.json`, которую пишет это ядро.
pub const SETTINGS_FORMAT_VERSION: u32 = 1;

/// `.rhizomind/settings.json` на диске — отдельно от типа контракта
/// [`Settings`]. Чтение терпимое: неизвестные поля, значения, которых это
/// ядро не знает, и файл более новой версии не ломают чтение — непонятное
/// значение заменяется значением по умолчанию, а неизвестные поля
/// сохраняются при записи. Версию формата ставит ядро.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    /// Версия формата файла.
    #[serde(default)]
    pub format_version: u32,
    /// Все остальные поля как есть.
    #[serde(flatten)]
    pub values: Map<String, Value>,
}

impl SettingsFile {
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
    pub fn update(&mut self, settings: &Settings) {
        if let Ok(Value::Object(known)) = serde_json::to_value(settings) {
            self.values.extend(known);
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
