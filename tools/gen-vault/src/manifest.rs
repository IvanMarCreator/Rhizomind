//! Манифест — сводка созданного хранилища для тестов ядра (1.1, 1.2, 1.4)
//! и нагрузочных тестов. Все пути — от корня хранилища, через `/`.

use serde::{Deserialize, Serialize};

/// Версия формата манифеста; меняется при несовместимых правках.
pub const FORMAT_VERSION: u32 = 1;

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
    /// Группы заметок с одинаковым именем (ключ имени: NFC, нижний регистр).
    pub duplicates: Vec<DuplicateGroup>,
    pub samples: Samples,
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
    /// Заметка на глубине 5 папок.
    pub deepest: String,
}
