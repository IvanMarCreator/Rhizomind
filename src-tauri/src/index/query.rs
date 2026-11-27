//! Поиск в индексе окна. Строку результата берём из текущего файла в хранилище.
use std::collections::HashSet;
use std::ops::Range;
use std::sync::PoisonError;

use rusqlite::params_from_iter;
use serde::{Deserialize, Serialize};

use super::{VaultIndex, search::words};
use crate::commands::{CoreError, CoreResult, ErrorCode, VaultPath, checked_path, limits, snippet};
use crate::{markdown, names, vault::Vault};

pub const PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    pub text: String,
    pub folder: Option<VaultPath>,
    pub tag: Option<String>,
    pub mentions_of: Option<VaultPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPosition {
    pub line: u32,
    pub column_utf16: u32,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: VaultPath,
    pub location: Option<SearchPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub items: Vec<SearchHit>,
    pub truncated: bool,
}

#[derive(Default)]
pub(crate) struct CandidatePage {
    pub paths: Vec<VaultPath>,
    pub has_more: bool,
}

impl SearchQuery {
    pub fn validate(&self) -> CoreResult<()> {
        limits::query(&self.text)?;
        if let Some(folder) = &self.folder {
            checked_path(folder, "folder")?;
        }
        if let Some(tag) = &self.tag {
            limits::tag_key(tag)?;
        }
        if let Some(path) = &self.mentions_of {
            checked_path(path, "mentions_of")?;
            if path.file_name().and_then(names::note_stem).is_none() {
                return Err(CoreError::invalid_argument("mentions_of"));
            }
        }
        Ok(())
    }

    fn mention(&self) -> Option<&str> {
        self.mentions_of
            .as_ref()?
            .file_name()
            .and_then(names::note_stem)
    }

    /// Проверка путей по индексу в памяти защищает чтение заметок от подмены
    /// результатов в SQLite. Файлы перечитываются после освобождения замка.
    pub(crate) fn candidates(
        &self,
        index: &VaultIndex,
        limit: Option<usize>,
    ) -> CoreResult<CandidatePage> {
        let unavailable = || CoreError::new(ErrorCode::IoFailed);
        if index.search_failed {
            return Err(unavailable());
        }
        let guard = index
            .database
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(db) = guard.as_ref() else {
            return Ok(CandidatePage::default());
        };
        let text = self.text.trim();
        let mut parameters = Vec::new();
        let mut sql = String::from("SELECT path FROM documents WHERE 1");
        let terms = words(&format!("{} {}", text, self.mention().unwrap_or_default()));
        let mut seen = HashSet::new();
        for term in terms.split_whitespace().filter(|word| seen.insert(*word)) {
            // Параметр FTS — один экранированный токен. Пользовательский текст
            // никогда не становится SQL, оператором или выражением FTS.
            parameters.push(format!("\"{term}\""));
            let n = parameters.len();
            sql.push_str(&format!(" AND id IN (SELECT rowid FROM titles WHERE titles MATCH ?{n} UNION SELECT rowid FROM bodies WHERE bodies MATCH ?{n})"));
        }
        if let Some(folder) = &self.folder
            && !folder.components().is_empty()
        {
            parameters.push(format!("{}/", folder.components().join("/")));
            sql.push_str(&format!(" AND instr(path,?{})=1", parameters.len()));
        }
        let tag = self
            .tag
            .as_deref()
            .map(|s| markdown::tag_key(s.trim_start_matches('#')));
        if terms.is_empty() && tag.is_none() && self.folder.is_none() && self.mention().is_none() {
            return Ok(CandidatePage::default());
        }
        parameters.push(text.to_lowercase());
        sql.push_str(&format!(" ORDER BY title=?{} DESC, path", parameters.len()));
        if let Some(limit) = limit {
            // Одна дополнительная строка сообщает, есть ли ещё кандидаты.
            sql.push_str(&format!(" LIMIT {}", limit.saturating_add(1)));
        }
        let mut statement = db.prepare_cached(&sql).map_err(|_| unavailable())?;
        let paths = statement
            .query_map(params_from_iter(parameters.iter()), |r| {
                r.get::<_, String>(0)
            })
            .map_err(|_| unavailable())?;
        let mut page = CandidatePage::default();
        for (offset, path) in paths.enumerate() {
            let path = path.map_err(|_| unavailable())?;
            if Some(offset) == limit {
                page.has_more = true;
                break;
            }
            let path = VaultPath::from_components(path.split('/'));
            if Some(&path) == self.mentions_of.as_ref() {
                continue;
            }
            if let Some(entry) = index.entry(&path)
                && tag.as_ref().is_none_or(|t| {
                    entry
                        .parsed
                        .tags
                        .iter()
                        .any(|v| markdown::tag_key(&v.name) == *t)
                })
            {
                page.paths.push(path);
            }
        }
        Ok(page)
    }

    /// Обычно достаточно 51 пути: последняя строка определяет признак усечения.
    /// Если файлы устарели или фильтр убрал часть строк, проверяем всю выборку.
    /// Замок индекса удерживается только внутри select, до чтения Markdown.
    pub(crate) fn first_page(
        &self,
        vault: &Vault,
        mut select: impl FnMut(Option<usize>) -> CoreResult<CandidatePage>,
    ) -> CoreResult<SearchResults> {
        let page = select(Some(PAGE_SIZE + 1))?;
        let found = self.results(vault, page.paths);
        if found.truncated || !page.has_more {
            return Ok(found);
        }
        let all = select(None)?;
        Ok(self.results(vault, all.paths))
    }

    fn results(&self, vault: &Vault, candidates: Vec<VaultPath>) -> SearchResults {
        let query = words(&self.text);
        let mention = self.mention().map(Mention::new);
        let required: HashSet<_> = query.split_whitespace().collect();
        let tag = self
            .tag
            .as_deref()
            .map(|key| markdown::tag_key(key.trim_start_matches('#')));
        let mut items = Vec::new();
        for path in candidates {
            let Ok(text) = vault.read_search_text(&path) else {
                continue;
            };
            let name = path
                .file_name()
                .and_then(names::note_stem)
                .unwrap_or_default();
            let parsed = (tag.is_some() || mention.is_some()).then(|| markdown::parse(&text));
            if let (Some(key), Some(parsed)) = (&tag, &parsed)
                && !parsed
                    .tags
                    .iter()
                    .any(|t| markdown::tag_key(&t.name) == *key)
            {
                continue;
            }
            if let Some(location) =
                evidence(&text, name, &required, mention.as_ref(), parsed.as_ref())
            {
                if items.len() == PAGE_SIZE {
                    return SearchResults {
                        items,
                        truncated: true,
                    };
                }
                items.push(SearchHit { path, location });
            }
        }
        SearchResults {
            items,
            truncated: false,
        }
    }
}

/// Границы слова в исходных байтах для перехода по строке и единицам UTF-16.
fn spans(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = None;
    text.char_indices()
        .chain(std::iter::once((text.len(), ' ')))
        .filter_map(move |(at, c)| {
            if word_char(c) {
                start.get_or_insert(at);
                None
            } else {
                start.take().map(|s| s..at)
            }
        })
}

/// Сравниваем целые слова по тому же ключу, что и имена заметок.
struct Mention {
    key: names::NameKey,
    units: usize,
}

impl Mention {
    fn new(name: &str) -> Self {
        Self {
            key: names::key(name),
            units: name_units(name).count(),
        }
    }

    fn find(
        &self,
        line: &str,
        offset: usize,
        links: &[markdown::WikiLink],
    ) -> Option<Range<usize>> {
        // Два итератора очерчивают окно длиной в имя в исходной строке.
        // Составная и разложенная буквы занимают одну единицу внутри слова.
        let ends = name_units(line).skip(self.units.saturating_sub(1));
        name_units(line)
            .zip(ends)
            .map(|(left, right)| left.start..right.end)
            .find(|span| {
                !line[..span.start]
                    .chars()
                    .next_back()
                    .is_some_and(word_char)
                    && !line[span.end..].chars().next().is_some_and(word_char)
                    && !links.iter().any(|link| {
                        link.span.start < offset + span.end && offset + span.start < link.span.end
                    })
                    && names::key(&line[span.clone()]) == self.key
            })
    }
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(c)
}

/// Слово со всеми его комбинируемыми знаками или один разделитель имени.
fn name_units(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, c) = chars.next()?;
        if word_char(c) {
            while chars.next_if(|(_, next)| word_char(*next)).is_some() {}
        }
        let end = chars.peek().map_or(text.len(), |(i, _)| *i);
        Some(start..end)
    })
}

fn evidence(
    text: &str,
    title: &str,
    required: &HashSet<&str>,
    mention: Option<&Mention>,
    parsed: Option<&markdown::ParsedNote>,
) -> Option<Option<SearchPosition>> {
    if required.is_empty() && mention.is_none() {
        return Some(None);
    }
    let mut found: HashSet<String> = words(title)
        .split_whitespace()
        .filter(|w| required.contains(*w))
        .map(str::to_owned)
        .collect();
    let mut first = None;
    let mut at = 0;
    let links = parsed.map(|parsed| &parsed.links);
    for (i, line) in text.split('\n').enumerate() {
        for span in spans(line) {
            let stem = words(&line[span.clone()]);
            if required.contains(stem.as_str()) {
                found.insert(stem);
                if first.is_none() && mention.is_none() {
                    first = Some(position(line, i, span));
                }
            }
        }
        if let (Some(mention), Some(links)) = (mention, links)
            && first.is_none()
            && let Some(span) = mention.find(line, at, links)
        {
            first = Some(position(line, i, span));
        }
        if found.len() == required.len() && first.is_some() {
            return Some(first);
        }
        at += line.len() + 1;
    }
    (found.len() == required.len() && (mention.is_none() || first.is_some())).then_some(first)
}

fn position(line: &str, i: usize, span: Range<usize>) -> SearchPosition {
    SearchPosition {
        line: u32::try_from(i + 1).unwrap_or(u32::MAX),
        column_utf16: u32::try_from(line[..span.start].encode_utf16().count()).unwrap_or(u32::MAX),
        snippet: snippet(line, span).0,
    }
}
