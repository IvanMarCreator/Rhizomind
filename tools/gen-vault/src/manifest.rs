//! Манифест — сводка созданного хранилища для тестов ядра (1.1, 1.2, 1.4)
//! и нагрузочных тестов. Все пути — от корня хранилища, через `/`.

use serde::{Deserialize, Serialize};

/// Версия формата манифеста; меняется при несовместимых правках.
pub const FORMAT_VERSION: u32 = 1;

/// Ключ имени генератора — упрощённый. Для его имён (кириллица, латиница,
/// цифры, одно имя в NFD) он совпадает с ключом ядра; ядро (задача 1.1)
/// использует полное приведение регистра и на нём не проверяется.
pub const KEY_RULE: &str = "упрощённый ключ: NFC → нижний регистр (str::to_lowercase) → NFC; \
годится только для имён этого генератора, правило ядра — полное приведение регистра (задача 1.1)";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub generator: String,
    pub seed: u64,
    /// Число заметок (`.md`), включая образцы.
    pub notes: usize,
    /// Число папок без корня.
    pub folders: usize,
    pub max_depth: usize,
    /// Правило ключа имени в этом манифесте (поле `key` у дубликатов).
    pub key_rule: String,
    /// Скрытые файлы (точка в начале имени файла или папки); в `notes` не входят.
    pub hidden: Vec<String>,
    /// Группы заметок с одинаковым ключом имени (см. `key_rule`).
    pub duplicates: Vec<DuplicateGroup>,
    pub samples: Samples,
    pub attachments: Attachments,
    pub encodings: Encodings,
    pub links: LinkStats,
    /// Ожидаемые разрешения ссылок: все ссылки образцов и ссылки из части
    /// массовых заметок (примерно 300 заметок).
    pub expected_links: Vec<ExpectedLink>,
}

/// Число ссылок по видам (вне кода). Виды пересекаются: `[[Папка/Имя#З|п]]`
/// считается и в `path`, и в `heading`, и в `alias`; `plain` — без всего этого.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkStats {
    pub total: usize,
    /// `[[Имя]]`.
    pub plain: usize,
    /// `[[…|псевдоним]]`.
    pub alias: usize,
    /// `[[…#Заголовок]]`.
    pub heading: usize,
    /// `[[Папка/Имя]]` (путь не от корня).
    pub path: usize,
    /// `[[/Имя]]`, `[[/Папка/Имя]]`.
    pub root: usize,
    /// Ссылки на заметки, которых нет.
    pub unresolved: usize,
    /// Ссылки без пути на имя, которое есть у нескольких заметок.
    pub ambiguous_bare: usize,
    /// Имя в ссылке написано иначе, чем имя файла (регистр, форма Unicode).
    pub case_variant: usize,
    /// `[[` внутри кода (блоки ``` и ~~~, отступ, код в строке) — не ссылки.
    pub in_code: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Ведёт ровно на одну заметку (`target`).
    Unique,
    /// Ссылка без пути, заметок с именем несколько (`candidates`); ведёт на
    /// самую старую по дате создания — у генератора это созданная первой (`target`).
    Oldest,
    /// Заметки нет.
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedLink {
    /// Заметка, в которой стоит ссылка.
    pub source: String,
    /// Текст ссылки со скобками, как в файле.
    pub link: String,
    pub resolution: Resolution,
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    pub heading: Option<String>,
    /// Есть ли заголовок в целевой заметке (для ссылок с `#`).
    pub heading_exists: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    /// Упрощённый ключ имени (см. `KEY_RULE`).
    pub key: String,
    pub notes: Vec<DuplicateNote>,
    /// Заметка, созданная первой: на неё ведёт ссылка без пути (С3.4).
    /// Файлы пишутся по порядку создания, но дата создания на диске может
    /// совпасть у нескольких файлов — тест ядра задаёт её сам.
    pub created_first: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateNote {
    pub path: String,
    /// Кратчайший различающий путь без скобок: `Альфа/Встреча`, `/План`.
    pub shortest_link: String,
}

/// Образцы с известными именами.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Samples {
    /// «Заметка 2», «Заметка 10»… в одной папке — естественная сортировка.
    pub natural_sort: Vec<String>,
    /// «Ёлка» и «Елка» — разные имена.
    pub yo_e: Vec<String>,
    /// Одно имя в разном регистре в разных папках.
    pub case_variants: Vec<String>,
    /// Имя в разложенной форме Unicode (NFD).
    pub nfd_name: String,
    /// Заметка в корне и заметка с тем же именем в папке.
    pub root_and_folder: Vec<String>,
    /// Имя с `#`: ссылок на эту заметку нет (`#` в ссылке — начало заголовка).
    pub hash_in_name: String,
    /// Заметка на глубине 5 папок.
    pub deepest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachments {
    pub png: usize,
    pub pdf: usize,
    /// Пути PNG и PDF.
    pub paths: Vec<String>,
    /// Вложения других форматов: TXT, CSV, ZIP.
    pub other: Vec<String>,
}

/// Кодировки и переносы строк всех заметок (`.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encodings {
    /// Только LF.
    pub lf: usize,
    /// Только CRLF.
    pub crlf: usize,
    /// И LF, и CRLF в одном файле.
    pub mixed: usize,
    /// UTF-8 с BOM.
    pub bom: usize,
    /// Последняя строка без перевода строки.
    pub no_final_newline: usize,
    pub samples: EncodingSamples,
}

/// Образцы с известными свойствами (все — в папке «Кодировки»).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingSamples {
    pub lf: String,
    pub crlf: String,
    pub lf_bom: String,
    pub crlf_bom: String,
    pub mixed: String,
    pub no_final_newline: String,
    /// Невалидный UTF-8 посередине: ядро открывает только для чтения.
    pub invalid_utf8: String,
    /// Файл больше лимита (`--big-mb`); нет — `null`.
    pub big: Option<BigFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigFile {
    pub path: String,
    pub bytes: u64,
    pub limit_mb: u64,
}
