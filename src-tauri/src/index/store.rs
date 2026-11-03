//! Файл индекса `.rhizomind/index.json` (задача 0.2-1.3; С2-4, С2-4.1, Р12,
//! Р13): запись, чтение, сверка при открытии.
//!
//! **Формат** — компактный JSON (без отступов), UTF-8:
//!
//! ```text
//! {"format_version":1,"rules_version":1,"notes":[
//!   {"p":["папка","Заметка.md"],        путь компонентами (как в API)
//!    "s":1234,"m":1730000000000,        размер, дата изменения (мс UTC)
//!    "c":1720000000000,                 дата создания (мс UTC)
//!    "h":[[1,"Заголовок",3]],           заголовки: уровень, текст, строка
//!    "t":[["тег",0,5]],                 теги: имя, 0 — текст / 1 — свойства, строка
//!    "l":[[0,"Цель","Раздел","псевдоним",10,40,12,16,7]]}]}
//!                                       ссылки: 1 — встраивание `!`, цель,
//!                                       заголовок, псевдоним, начало и конец
//!                                       ссылки, начало и конец цели (байты
//!                                       текста `read_note`), строка
//! ```
//!
//! Пустые `h`, `t`, `l` не пишутся. **Текста заметок в файле нет** — только
//! заголовки, теги, цели и псевдонимы ссылок и их положения (Р13); строки
//! для панели обратных ссылок читаются из заметок по запросу (задача 0.2-1.4).
//!
//! **Чтение** ([`load`]) — с собственным пределом [`MAX_INDEX_FILE_BYTES`]
//! (256 МБ) вместо общих 4 МБ служебных файлов. Файла нет, он больше
//! предела, не разбирается, оборван, версия формата или правил разбора не
//! та, путь или положения не могут быть настоящими — [`Load::Rebuild`]:
//! индекс собирается с нуля ([`super::rebuild`]) без копии старого файла
//! (индекс производный, Р12). Иначе — [`reconcile`]: сверка с диском по
//! размеру и дате изменения.
//!
//! **Запись** ([`IndexStore`]) — только готового индекса, атомарно через
//! `Vault::write_service_file` (временный файл в `.rhizomind/tmp/`, сброс на
//! диск, замена; служебная папка и временный файл проверены по дескриптору).
//! Не чаще раза в [`WRITE_INTERVAL`] и только если индекс изменился
//! ([`IndexStore::write_if_due`], время передаёт вызывающий); при закрытии
//! окна — [`IndexStore::write_now`] после записи заметок, если успевает в
//! предел закрытия (`windows::closing`). Запись не удалась (носитель только
//! для чтения, нет места) — молча: индекс остаётся в памяти, следующая
//! попытка — при следующем изменении. Вне хранилища ничего не пишется, сети
//! нет.

use std::collections::HashSet;
use std::sync::{PoisonError, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{FORMAT_VERSION, MAX_INDEX_FILE_BYTES, NoteEntry, VaultIndex, read_entry};
use crate::commands::{CoreResult, IndexState, VaultPath};
use crate::markdown::{Heading, ParsedNote, RULES_VERSION, Tag, TagSource, WikiLink};
use crate::vault::{ServiceRead, Vault, is_service_dir};

/// Имя файла индекса в `.rhizomind/`.
pub const FILE_NAME: &str = "index.json";

/// Запись при изменениях — не чаще этого (С2-4.1, Р12).
pub const WRITE_INTERVAL: Duration = Duration::from_secs(30);

/// Где лежит файл индекса: в программе — [`Vault`] (`.rhizomind/index.json`),
/// в тестах — подмена (носитель только для чтения).
pub trait IndexFile {
    /// Содержимое файла с пределом [`MAX_INDEX_FILE_BYTES`].
    fn read_index(&self) -> CoreResult<ServiceRead>;
    /// Атомарная запись файла.
    fn write_index(&self, bytes: &[u8]) -> CoreResult<()>;
}

impl IndexFile for Vault {
    fn read_index(&self) -> CoreResult<ServiceRead> {
        self.read_service_file_limited(FILE_NAME, MAX_INDEX_FILE_BYTES)
    }

    fn write_index(&self, bytes: &[u8]) -> CoreResult<()> {
        self.write_service_file(FILE_NAME, bytes)
    }
}

/// Почему индекс собирается с нуля, а не берётся из файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildReason {
    /// Файла нет (первое открытие, файл удалили — С2-9, п. 3).
    Missing,
    /// Больше [`MAX_INDEX_FILE_BYTES`] или не обычный файл.
    TooLarge,
    /// Не разбирается, оборван, путь или положения не могут быть настоящими.
    Broken,
    /// `format_version` или `rules_version` не те (старый файл или файл более
    /// новой версии программы).
    WrongVersion,
    /// Не прочитать (файл-ссылка за контуром, ошибка чтения).
    Unavailable,
}

/// Итог чтения файла индекса при открытии хранилища.
#[derive(Debug)]
pub enum Load {
    /// Готовый индекс из файла — дальше [`reconcile`].
    Loaded(Box<VaultIndex>),
    /// Индекс собирается с нуля ([`super::rebuild`]); файл перезапишется
    /// после сборки.
    Rebuild(RebuildReason),
}

/// Итог попытки записи.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Файл записан.
    Written,
    /// Индекс не менялся с последней попытки — не пишется.
    Unchanged,
    /// Прошлая попытка была меньше [`WRITE_INTERVAL`] назад.
    Throttled,
    /// Индекс ещё строится — неполный индекс не пишется.
    NotReady,
    /// Запись не удалась (носитель только для чтения и т.п.) — молча, индекс
    /// остаётся в памяти.
    Failed,
}

/// Читает файл индекса: готовый индекс или причина пересборки с нуля.
/// Сверку с диском делает [`reconcile`].
pub fn load(file: &dyn IndexFile) -> Load {
    match file.read_index() {
        Err(_) => Load::Rebuild(RebuildReason::Unavailable),
        Ok(ServiceRead::Missing) => Load::Rebuild(RebuildReason::Missing),
        Ok(ServiceRead::TooLarge) => Load::Rebuild(RebuildReason::TooLarge),
        Ok(ServiceRead::Bytes(bytes)) => match decode(&bytes) {
            Ok(index) => Load::Loaded(Box::new(index)),
            Err(reason) => Load::Rebuild(reason),
        },
    }
}

/// Сверка загруженного индекса с диском (С2-4.1): заметки с той же датой
/// изменения и размером берутся из индекса, остальные перечитываются,
/// исчезнувшие удаляются, новые читаются. Файлы читаются **вне замка**
/// индекса; запись, которую за это время обновил кто-то другой (наблюдатель,
/// своя запись), не перекрывается. Хранилище недоступно — ошибка.
pub fn reconcile(vault: &Vault, index: &RwLock<VaultIndex>) -> CoreResult<()> {
    let disk = vault.note_stats()?;
    // Снимок под замком на чтение: «размер, дата изменения, дата создания»
    // из индекса для каждой заметки с диска и заметки индекса, которых на
    // диске нет. Пути не копируются, кроме исчезнувших: копии всех путей
    // остались бы за процессом и после сверки (замер 0.2-7.1).
    let (known, gone): (Vec<Option<Stamp>>, Vec<(VaultPath, Stamp)>) = {
        let index = index.read().unwrap_or_else(PoisonError::into_inner);
        let known = disk
            .iter()
            .map(|stat| index.entry(&stat.note.path).map(stamp))
            .collect();
        let on_disk: HashSet<&VaultPath> = disk.iter().map(|stat| &stat.note.path).collect();
        let gone = index
            .entries()
            .filter(|(path, _)| !on_disk.contains(path))
            .map(|(path, entry)| (path.clone(), stamp(entry)))
            .collect();
        (known, gone)
    };

    let mut stale = Vec::new();
    let mut created = Vec::new();
    for (stat, &known) in disk.iter().zip(&known) {
        match known {
            Some((size, modified, created_ms))
                if size == stat.size && modified == stat.modified_ms =>
            {
                if created_ms != stat.note.created_ms {
                    created.push((stat, known));
                }
            }
            _ => stale.push((stat, known)),
        }
    }

    // Чтение и разбор — без замка.
    let read: Vec<(&VaultPath, Option<Stamp>, Option<NoteEntry>)> = stale
        .iter()
        .map(|&(stat, known)| (&stat.note.path, known, read_entry(vault, &stat.note).ok()))
        .collect();

    let mut index = index.write().unwrap_or_else(PoisonError::into_inner);
    // Запись, которую сменили за время чтения, — главнее прочитанного здесь.
    let untouched = |index: &VaultIndex, path: &VaultPath, known: Option<Stamp>| {
        index.entry(path).map(stamp) == known
    };
    for (path, known) in &gone {
        if untouched(&index, path, Some(*known)) {
            index.remove(path);
        }
    }
    for (path, known, entry) in read {
        if !untouched(&index, path, known) {
            continue;
        }
        match entry {
            Some(entry) => index.upsert(path.clone(), entry),
            None => index.remove(path),
        }
    }
    for (stat, known) in created {
        if !untouched(&index, &stat.note.path, known) {
            continue;
        }
        if let Some(entry) = index.entry(&stat.note.path) {
            let entry = NoteEntry {
                created_ms: stat.note.created_ms,
                ..entry.clone()
            };
            index.upsert(stat.note.path.clone(), entry);
        }
    }
    Ok(())
}

/// Размер, дата изменения и дата создания записи — сверка с диском.
type Stamp = (u64, i64, i64);

fn stamp(entry: &NoteEntry) -> Stamp {
    (entry.size, entry.modified_ms, entry.created_ms)
}

/// Когда и что писать: номер изменения индекса последней попытки и её время.
/// Живёт рядом с индексом открытого хранилища (задача 0.2-1.4); время
/// передаёт вызывающий — логика проверяется без ожидания.
#[derive(Debug, Default)]
pub struct IndexStore {
    /// [`VaultIndex::revision`] последней попытки записи (удачной или нет)
    /// или загруженного файла.
    attempted: Option<u64>,
    /// Время последней попытки по сроку ([`Self::write_if_due`]).
    last_attempt: Option<Instant>,
}

impl IndexStore {
    /// Файл индекса ещё не писался (сборка с нуля).
    pub fn new() -> Self {
        Self::default()
    }

    /// Индекс только что загружен из файла: в файле то же, что в индексе с
    /// номером изменения `revision`.
    pub fn loaded(revision: u64) -> Self {
        Self {
            attempted: Some(revision),
            last_attempt: None,
        }
    }

    /// Пишет индекс, если он готов, изменился с последней попытки и с неё
    /// прошло не меньше [`WRITE_INTERVAL`] (`now` — текущее время).
    pub fn write_if_due(
        &mut self,
        file: &dyn IndexFile,
        index: &RwLock<VaultIndex>,
        now: Instant,
    ) -> WriteOutcome {
        let throttled = self
            .last_attempt
            .is_some_and(|last| now.saturating_duration_since(last) < WRITE_INTERVAL);
        let outcome = self.write(file, index, throttled);
        if matches!(outcome, WriteOutcome::Written | WriteOutcome::Failed) {
            self.last_attempt = Some(now);
        }
        outcome
    }

    /// Пишет индекс сразу, без срока (закрытие окна), если он готов и
    /// изменился с последней попытки.
    pub fn write_now(&mut self, file: &dyn IndexFile, index: &RwLock<VaultIndex>) -> WriteOutcome {
        self.write(file, index, false)
    }

    fn write(
        &mut self,
        file: &dyn IndexFile,
        index: &RwLock<VaultIndex>,
        throttled: bool,
    ) -> WriteOutcome {
        let (revision, bytes) = {
            let index = index.read().unwrap_or_else(PoisonError::into_inner);
            if index.status().state != IndexState::Ready {
                return WriteOutcome::NotReady;
            }
            let revision = index.revision();
            if self.attempted == Some(revision) {
                return WriteOutcome::Unchanged;
            }
            if throttled {
                return WriteOutcome::Throttled;
            }
            (revision, encode(&index))
        };
        // Диск — уже без замка индекса.
        self.attempted = Some(revision);
        match file.write_index(&bytes) {
            Ok(()) => WriteOutcome::Written,
            Err(_) => WriteOutcome::Failed,
        }
    }
}

// --- Формат файла ------------------------------------------------------

/// Заголовок файла: версии проверяются до разбора заметок, чтобы файл другой
/// версии с другим составом записей давал «версия не та», а не «испорчен».
/// Версии разбираются только как числа; любое другое значение (строка,
/// массив, объект) пропускается без накопления в памяти и считается «версия
/// не та» (ревью 0.2-1.3: подменённый массив на сотни МБ не строится).
#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    format_version: Version,
    #[serde(default)]
    rules_version: Version,
}

/// Номер версии из файла; не целое неотрицательное число — `None`.
#[derive(Default)]
struct Version(Option<u64>);

impl Version {
    fn is(&self, expected: u32) -> bool {
        self.0 == Some(u64::from(expected))
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(VersionVisitor)
    }
}

struct VersionVisitor;

impl<'de> serde::de::Visitor<'de> for VersionVisitor {
    type Value = Version;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("номер версии")
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Version, E> {
        Ok(Version(Some(v)))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Version, E> {
        Ok(Version(u64::try_from(v).ok()))
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Version, E> {
        Ok(Version(None))
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Version, E> {
        Ok(Version(None))
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Version, E> {
        Ok(Version(None))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Version, E> {
        Ok(Version(None))
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Version, E> {
        Ok(Version(None))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Version, A::Error> {
        while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
        Ok(Version(None))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Version, A::Error> {
        while map
            .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
            .is_some()
        {}
        Ok(Version(None))
    }
}

#[derive(Serialize)]
struct FileRepr {
    format_version: u32,
    rules_version: u32,
    notes: Vec<NoteRepr>,
}

/// Файл при чтении: заметки сразу идут в индекс, по одной, без
/// промежуточного списка всех записей (замер 0.2-7.1: память, освобождённая
/// после загрузки, остаётся за процессом).
#[derive(Deserialize)]
struct FileIn {
    notes: NotesIn,
}

/// Индекс из массива `notes`; запись, которая не может быть настоящей, —
/// ошибка разбора (файл испорчен).
struct NotesIn(VaultIndex);

impl<'de> Deserialize<'de> for NotesIn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(NotesVisitor)
    }
}

struct NotesVisitor;

impl<'de> serde::de::Visitor<'de> for NotesVisitor {
    type Value = NotesIn;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("массив записей заметок")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<NotesIn, A::Error> {
        let mut error = None;
        let notes = std::iter::from_fn(|| match seq.next_element::<NoteRepr>() {
            Ok(note) => note.and_then(|note| {
                let entry = note_entry(note);
                if entry.is_none() {
                    error = Some(serde::de::Error::custom("запись не может быть настоящей"));
                }
                entry
            }),
            Err(e) => {
                error = Some(e);
                None
            }
        });
        let index = VaultIndex::from_entries(notes);
        match error {
            Some(e) => Err(e),
            None => Ok(NotesIn(index)),
        }
    }
}

/// Заголовок: уровень, текст, строка.
type HeadingRepr = (u8, String, u32);
/// Тег: имя, источник (0 — текст, 1 — блок свойств), строка.
type TagRepr = (String, u8, u32);
/// Ссылка: встраивание (0/1), цель, заголовок, псевдоним, начало и конец
/// ссылки, начало и конец цели, строка.
type LinkRepr = (
    u8,
    String,
    Option<String>,
    Option<String>,
    usize,
    usize,
    usize,
    usize,
    u32,
);

#[derive(Serialize, Deserialize)]
struct NoteRepr {
    p: Vec<String>,
    s: u64,
    m: i64,
    c: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    h: Vec<HeadingRepr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    t: Vec<TagRepr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    l: Vec<LinkRepr>,
}

/// Компактный JSON индекса; заметки по порядку пути (одинаковый индекс —
/// одинаковые байты).
fn encode(index: &VaultIndex) -> Vec<u8> {
    let mut entries: Vec<(&VaultPath, &NoteEntry)> = index.entries().collect();
    entries.sort_by(|a, b| a.0.components().cmp(b.0.components()));
    let file = FileRepr {
        format_version: FORMAT_VERSION,
        rules_version: RULES_VERSION,
        notes: entries
            .into_iter()
            .map(|(path, entry)| note_repr(path, entry))
            .collect(),
    };
    // Сериализация своих структур в память не отказывает.
    serde_json::to_vec(&file).unwrap_or_default()
}

fn note_repr(path: &VaultPath, entry: &NoteEntry) -> NoteRepr {
    let parsed = &entry.parsed;
    NoteRepr {
        p: path.components().to_vec(),
        s: entry.size,
        m: entry.modified_ms,
        c: entry.created_ms,
        h: parsed
            .headings
            .iter()
            .map(|h| (h.level, h.text.clone(), h.line))
            .collect(),
        t: parsed
            .tags
            .iter()
            .map(|t| {
                let source = match t.source {
                    TagSource::Text => 0,
                    TagSource::Frontmatter => 1,
                };
                (t.name.clone(), source, t.line)
            })
            .collect(),
        l: parsed
            .links
            .iter()
            .map(|l| {
                (
                    u8::from(l.embed),
                    l.target.clone(),
                    l.heading.clone(),
                    l.alias.clone(),
                    l.span.start,
                    l.span.end,
                    l.target_span.start,
                    l.target_span.end,
                    l.line,
                )
            })
            .collect(),
    }
}

/// Готовый индекс из байтов файла или причина пересборки.
fn decode(bytes: &[u8]) -> Result<VaultIndex, RebuildReason> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| RebuildReason::Broken)?;
    if !header.format_version.is(FORMAT_VERSION) || !header.rules_version.is(RULES_VERSION) {
        return Err(RebuildReason::WrongVersion);
    }
    let file: FileIn = serde_json::from_slice(bytes).map_err(|_| RebuildReason::Broken)?;
    Ok(file.notes.0)
}

/// Запись заметки из файла; путь или положения, которые не могут быть
/// настоящими, — `None` (файл подменён или испорчен).
fn note_entry(note: NoteRepr) -> Option<(VaultPath, NoteEntry)> {
    let path = VaultPath::from_components(note.p);
    let name = path.file_name()?;
    if path.check().is_err()
        || !crate::names::is_note(name)
        || path.components().first().is_some_and(|c| is_service_dir(c))
    {
        return None;
    }
    let size = usize::try_from(note.s).ok()?;
    let headings = note
        .h
        .into_iter()
        .map(|(level, text, line)| {
            (1..=6)
                .contains(&level)
                .then_some(Heading { level, text, line })
        })
        .collect::<Option<Vec<_>>>()?;
    let tags = note
        .t
        .into_iter()
        .map(|(name, source, line)| {
            // Тег, которого разбор не дал бы (длинный, с пробелом и т.п.), —
            // файл подменён (ревью безопасности 0.2-1.4).
            if !crate::markdown::is_valid_tag_name(&name) {
                return None;
            }
            let source = match source {
                0 => TagSource::Text,
                1 => TagSource::Frontmatter,
                _ => return None,
            };
            Some(Tag { name, source, line })
        })
        .collect::<Option<Vec<_>>>()?;
    let links = note
        .l
        .into_iter()
        .map(
            |(embed, target, heading, alias, start, end, t_start, t_end, line)| {
                let embed = match embed {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                // Текст `read_note` не длиннее файла (BOM и `\r` только убираются).
                let sane = start <= t_start && t_start <= t_end && t_end <= end && end <= size;
                sane.then_some(WikiLink {
                    embed,
                    target,
                    heading,
                    alias,
                    span: start..end,
                    target_span: t_start..t_end,
                    line,
                })
            },
        )
        .collect::<Option<Vec<_>>>()?;
    Some((
        path,
        NoteEntry {
            size: note.s,
            modified_ms: note.m,
            created_ms: note.c,
            parsed: ParsedNote {
                headings,
                tags,
                links,
            },
        },
    ))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
