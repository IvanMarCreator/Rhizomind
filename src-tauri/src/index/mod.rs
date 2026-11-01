//! Индекс хранилища (выпуск 0.2, С2-4, С2-5.2, ADR 0004): для каждой заметки
//! — размер, даты, заголовки, теги и ссылки с положениями; по ним —
//! обратные ссылки (заметка → ссылки на неё) и теги (ключ → заметки).
//!
//! Не путать со **списком заметок** (`commands::NoteList`): тот знает только
//! пути и даты создания и разрешает ссылки; индекс знает, что внутри заметок
//! (план 0.2, Т3). Ссылки индекс разрешает через [`Links`] списка заметок —
//! правила С3.4 в одном месте.
//!
//! Индекс целиком в памяти; модуль — единственный, кто знает формат файла
//! `.rhizomind/index.json` (в 0.3 его заменит SQLite без изменения
//! остального кода). Текста заметок в индексе нет (Р13).
//!
//! Задача 0.2-0.1 — только типы и сигнатуры: индекс всегда пустой и
//! «строится». Индекс в памяти и сборка — 0.2-1.2, файл — 0.2-1.3, работа в
//! окне — 0.2-1.4.

use std::ops::Range;

use crate::commands::{IndexState, IndexStatus, VaultPath};
use crate::links::Links;
use crate::markdown::ParsedNote;

/// Версия формата `index.json` (`format_version`). Не та — пересборка с нуля
/// без копии старого файла (С2-4.1, Р12).
pub const FORMAT_VERSION: u32 = 1;

/// Предел размера `index.json` при чтении, байт — собственный, вместо общих
/// 4 МБ служебных файлов (С2-4.1). Больше — пересборка.
pub const MAX_INDEX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Запись индекса об одной заметке.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteEntry {
    /// Размер файла, байт (сверка при открытии).
    pub size: u64,
    /// Дата изменения, мс UTC (сверка при открытии).
    pub modified_ms: i64,
    /// Дата создания, мс UTC (отображаемое написание тега — из самой старой
    /// заметки при равенстве, решение 113).
    pub created_ms: i64,
    /// Заголовки, теги и ссылки (`markdown::parse`).
    pub parsed: ParsedNote,
}

/// Где в заметке стоит ссылка на запрошенную заметку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSite {
    /// Номер строки, с 1.
    pub line: u32,
    /// Вся ссылка в байтах от начала файла (на момент разбора; строка для
    /// панели читается по нему из файла).
    pub span: Range<usize>,
}

/// Заметка, ссылающаяся на запрошенную, и её ссылки по порядку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklinkSource {
    /// Ссылающаяся заметка (не сама запрошенная: ссылка на себя не
    /// считается).
    pub from: VaultPath,
    /// Ссылки на запрошенную заметку.
    pub sites: Vec<LinkSite>,
}

/// Тег хранилища: отображаемое написание, ключ и число заметок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    /// Самое частое написание, при равенстве — из самой старой заметки.
    pub display: String,
    /// Ключ тега (`markdown::tag_key`).
    pub key: String,
    /// Заметок с тегом.
    pub notes: u32,
}

/// Индекс хранилища в памяти.
#[derive(Debug, Default)]
pub struct VaultIndex {
    // Записи по ключу пути, карта «ключ имени цели → ссылки», обратные
    // ссылки и теги — задача 0.2-1.2.
}

impl VaultIndex {
    /// Пустой индекс, который ещё строится.
    pub fn new() -> Self {
        Self::default()
    }

    /// Строится или готов и ход сборки (команда `index_status`, событие
    /// `index:status`).
    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            state: IndexState::Building,
            done: 0,
            total: 0,
        }
    }

    /// Добавляет или заменяет запись заметки (сборка, своя запись, пачка
    /// наблюдателя). Разбор и чтение файла — до вызова, вне замка индекса.
    pub fn upsert(&mut self, path: VaultPath, entry: NoteEntry) {
        let _ = (path, entry);
    }

    /// Убирает заметку (удалена, переименована в не-заметку).
    pub fn remove(&mut self, path: &VaultPath) {
        let _ = path;
    }

    /// Заметки, ссылающиеся на `note`, по ссылкам, которые `links`
    /// разрешает в `note` сейчас (правила С3.4); без самой `note`.
    pub fn backlinks(&self, note: &VaultPath, links: &Links) -> Vec<BacklinkSource> {
        let _ = (note, links);
        Vec::new()
    }

    /// Все теги хранилища, у которых есть заметки.
    pub fn tags(&self) -> Vec<TagCount> {
        Vec::new()
    }

    /// Заметки с тегом по ключу; неизвестный ключ — пусто.
    pub fn tag_notes(&self, key: &str) -> Vec<VaultPath> {
        let _ = key;
        Vec::new()
    }
}
