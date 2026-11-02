//! Блок свойств (frontmatter) и теги из него (С2-3.3, Р14, Р17 п.7, Р18).
//!
//! Блок — от строки `---` в самом начале текста до следующей строки `---`
//! или `...` (концевые пробелы и табуляции у ограничителей допустимы; как
//! `PROPERTIES_OPEN`/`PROPERTIES_CLOSE` редактора). Незакрытый блок — не блок.
//! Из блока читаются только ключи `tags` и `tag` (регистр ключа не важен) в
//! формах `tags: [a, b]`, `tags: a, b`, `tags: a` и списком строками
//! `- a` (с отступом или без); значения в кавычках `"…"`/`'…'` и с `#`
//! впереди допустимы. Значение берётся целиком и проверяется правилом тега;
//! не подходит — пропускается. Остальные ключи не разбираются. Свой разбор
//! ровно этих форм, без библиотеки YAML (Р14).

use std::ops::Range;

use super::{Lines, SpannedTag, Tag, TagSource, tags};

/// Найденный блок свойств.
pub(super) struct Block {
    /// От первой `---` до конца закрывающей строки без перевода строки.
    pub span: Range<usize>,
    /// Где начинается текст после блока (позиция и номер строки); `None` —
    /// после закрывающей строки текста нет.
    pub body: Option<(usize, u32)>,
}

/// Блок свойств в начале `text`; его теги добавляются в `tags`.
pub(super) fn parse(text: &str, tags: &mut Vec<SpannedTag>) -> Option<Block> {
    let mut lines = Lines::new(text, 0, 1);
    let first = lines.next()?;
    if !is_delimiter(&text[first.range()], false) {
        return None;
    }
    let mut found = Vec::new();
    let mut in_list = false;
    for line in lines {
        let s = &text[line.range()];
        if is_delimiter(s, true) {
            tags.append(&mut found);
            let body = (line.end < text.len()).then(|| {
                let crlf = text.as_bytes().get(line.end..line.end + 2) == Some(b"\r\n");
                (
                    line.end + if crlf { 2 } else { 1 },
                    line.number.saturating_add(1),
                )
            });
            return Some(Block {
                span: 0..line.end,
                body,
            });
        }
        if in_list && let Some(item) = list_item(s) {
            push_value(text, line.start + item..line.end, line.number, &mut found);
            continue;
        }
        in_list = false;
        if let Some(value) = tags_key(s) {
            let at = line.start + value;
            let raw = trim_range(text, at..line.end);
            if raw.is_empty() {
                in_list = true;
            } else if text[raw.clone()].starts_with('[') {
                if text[raw.clone()].ends_with(']') && raw.len() >= 2 {
                    split_values(text, raw.start + 1..raw.end - 1, line.number, &mut found);
                }
            } else {
                split_values(text, raw, line.number, &mut found);
            }
        }
    }
    None
}

/// `---` (открывает и закрывает) или `...` (только закрывает) с концевыми
/// пробелами и табуляциями.
fn is_delimiter(line: &str, closing: bool) -> bool {
    let body = line.trim_end_matches([' ', '\t']);
    body == "---" || (closing && body == "...")
}

/// Ключ `tags:`/`tag:` в начале строки (без отступа) — позиция значения.
fn tags_key(line: &str) -> Option<usize> {
    let colon = line.find(':')?;
    let key = &line[..colon];
    (key.eq_ignore_ascii_case("tags") || key.eq_ignore_ascii_case("tag")).then_some(colon + 1)
}

/// Элемент списка `- значение` (отступ пробелами и табуляциями допустим) —
/// позиция значения.
fn list_item(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let after = rest.strip_prefix('-')?;
    (after.is_empty() || after.starts_with([' ', '\t'])).then_some(indent + 1)
}

/// Значения через запятую.
fn split_values(text: &str, range: Range<usize>, line: u32, out: &mut Vec<SpannedTag>) {
    let mut start = range.start;
    for part in text[range.clone()].split(',') {
        let end = start + part.len();
        push_value(text, start..end, line, out);
        start = end + 1;
    }
}

/// Одно значение: без пробелов по краям, без парных кавычек и `#` впереди;
/// тег, если подходит под правило тега целиком.
fn push_value(text: &str, range: Range<usize>, line: u32, out: &mut Vec<SpannedTag>) {
    let mut r = trim_range(text, range);
    let s = &text[r.clone()];
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        r = r.start + 1..r.end - 1;
    }
    if text[r.clone()].starts_with('#') {
        r.start += 1;
    }
    let name = &text[r.clone()];
    if tags::valid_name(name) {
        out.push(SpannedTag {
            tag: Tag {
                name: name.to_owned(),
                source: TagSource::Frontmatter,
                line,
            },
            span: r,
        });
    }
}

/// Участок без пробелов и табуляций по краям.
fn trim_range(text: &str, range: Range<usize>) -> Range<usize> {
    let s = &text[range.clone()];
    let start = range.start + (s.len() - s.trim_start_matches([' ', '\t']).len());
    let end = range.end - (s.len() - s.trim_end_matches([' ', '\t']).len());
    start..end.max(start)
}
