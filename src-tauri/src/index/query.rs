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
    pub fn candidates(&self, index: &VaultIndex) -> CoreResult<Vec<VaultPath>> {
        let unavailable = || CoreError::new(ErrorCode::IoFailed);
        if index.search_failed {
            return Err(unavailable());
        }
        let guard = index
            .database
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(db) = guard.as_ref() else {
            return Ok(Vec::new());
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
            return Ok(vec![]);
        }
        parameters.push(text.to_lowercase());
        sql.push_str(&format!(" ORDER BY title=?{} DESC, path", parameters.len()));
        let mut statement = db.prepare(&sql).map_err(|_| unavailable())?;
        let paths = statement
            .query_map(params_from_iter(parameters.iter()), |r| {
                r.get::<_, String>(0)
            })
            .map_err(|_| unavailable())?;
        let mut candidates = Vec::new();
        for path in paths {
            let path = path.map_err(|_| unavailable())?;
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
                candidates.push(path);
            }
        }
        Ok(candidates)
    }

    pub fn results(&self, vault: &Vault, candidates: Vec<VaultPath>) -> SearchResults {
        let query = words(&self.text);
        let required: HashSet<_> = query.split_whitespace().collect();
        let tag = self
            .tag
            .as_deref()
            .map(|key| markdown::tag_key(key.trim_start_matches('#')));
        let mut items = Vec::new();
        for path in candidates {
            let Ok(note) = vault.read_note(&path) else {
                continue;
            };
            let name = path
                .file_name()
                .and_then(names::note_stem)
                .unwrap_or_default();
            let parsed =
                (tag.is_some() || self.mention().is_some()).then(|| markdown::parse(&note.text));
            if let (Some(key), Some(parsed)) = (&tag, &parsed)
                && !parsed
                    .tags
                    .iter()
                    .any(|t| markdown::tag_key(&t.name) == *key)
            {
                continue;
            }
            if let Some(location) =
                evidence(&note.text, name, &required, self.mention(), parsed.as_ref())
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
            if c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(c) {
                start.get_or_insert(at);
                None
            } else {
                start.take().map(|s| s..at)
            }
        })
}

fn evidence(
    text: &str,
    title: &str,
    required: &HashSet<&str>,
    mention: Option<&str>,
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
        if let Some(name) = mention
            && let Some(links) = links.as_ref()
            && first.is_none()
        {
            // Имена UTF-8 сравниваются в исходных границах, сохраняя адрес
            // строки даже при изменении длины слова после приведения регистра.
            for (start, _) in line.char_indices() {
                if !line[start..]
                    .chars()
                    .next()
                    .zip(name.chars().next())
                    .is_some_and(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                {
                    continue;
                }
                let end = line[start..]
                    .char_indices()
                    .nth(name.chars().count())
                    .map_or(line.len(), |(n, _)| start + n);
                if names::key(&line[start..end]) != names::key(name) {
                    continue;
                }
                if line[..start]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric)
                    || line[end..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
                {
                    continue;
                }
                if !links
                    .iter()
                    .any(|l| l.span.start < at + end && at + start < l.span.end)
                {
                    first = Some(position(line, i, start..end));
                    break;
                }
            }
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
