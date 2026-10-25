//! Общие типы контракта: путь в хранилище, версия файла, хранилище.
//!
//! Это словарь ядра: его используют и команды, и модули (`vault`, `links`,
//! `watcher`…), чтобы между ними не было перекладывания одних и тех же данных.

use serde::{Deserialize, Serialize};

use super::error::CoreResult;

/// Версия API ядра. Возвращается командой `vault_current`; интерфейс
/// сверяет её при запуске (`src/api/version.ts`).
pub const API_VERSION: u32 = 1;

/// Путь внутри хранилища — **относительный, по компонентам**:
/// `["Проекты", "Альфа", "Встреча.md"]`. Корень хранилища — пустой список.
///
/// В JSON — массив строк. Разделителей внутри компонента нет и быть не может:
/// путь не разбирается из строки, поэтому `a/../b` не превращается в `b`.
/// Тип сам по себе не гарантирует допустимость компонентов: перед любым
/// обращением к диску модуль `vault` проверяет каждый компонент
/// ([`VaultPath::check`]) — `..`, `.`, `:`, разделители, зарезервированные
/// имена Windows, точка или пробел в конце, `\\?\` (порядок проверки — в
/// исследовании крейтов, раздел «Контур хранилища»).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultPath(Vec<String>);

impl VaultPath {
    /// Корень хранилища.
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Путь из компонентов без проверки (проверка — [`VaultPath::check`]).
    pub fn from_components<I, S>(components: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(components.into_iter().map(Into::into).collect())
    }

    /// Компоненты пути.
    pub fn components(&self) -> &[String] {
        &self.0
    }

    /// Имя последнего компонента (`None` для корня).
    pub fn file_name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }

    /// Проверяет каждый компонент правилами контура хранилища
    /// (`vault::check_component`). Ошибка — `invalid_path`.
    pub fn check(&self) -> CoreResult<()> {
        self.0
            .iter()
            .try_for_each(|component| crate::vault::check_component(component))
    }
}

/// Версия файла: хэш содержимого `blake3`, размер и дата изменения (словарь:
/// «Версия файла»). По ней ядро отличает свои записи от внешних и
/// обнаруживает конфликт при `note_write`.
///
/// Для интерфейса это непрозрачный знак: он хранит версию из `note_read` и
/// возвращает её в `note_write` как ожидаемую, сравнивая версии только на
/// равенство всех полей.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileVersion {
    /// Хэш `blake3` содержимого, 64 шестнадцатеричных символа в нижнем регистре.
    pub hash: String,
    /// Размер в байтах.
    pub size: u64,
    /// Дата изменения, миллисекунды с 1970-01-01 UTC.
    pub modified_ms: i64,
}

/// Идентификатор хранилища в списке хранилищ (`vaults.json`). Непрозрачная
/// строка; путь к папке из него не выводится.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultId(pub String);

/// Хранилище, как его видит интерфейс.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultSummary {
    /// Идентификатор в списке хранилищ.
    pub id: VaultId,
    /// Имя хранилища (имя папки).
    pub name: String,
    /// Полный путь к папке — только для показа в окне «Управление хранилищами».
    pub path: String,
}

/// Вид элемента дерева файлов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Папка.
    Folder,
    /// Заметка (`.md`).
    Note,
    /// Другой файл (вложение): в 0.1 не открывается, но переименовывается,
    /// перемещается и удаляется.
    File,
}
