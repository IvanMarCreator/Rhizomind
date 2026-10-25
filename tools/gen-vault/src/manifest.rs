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
