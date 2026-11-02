//! Вики-ссылки `[[…]]` и встраивания `![[…]]` в абзаце — как `findWikilinks`
//! в `src/core/wikilinks/index.ts` (образцы `tests/fixtures/wikilinks/text.json`):
//! ссылка не переходит через перенос строки и не заходит в код в строке;
//! ссылку открывает последняя `[[` перед первой `]]`. Части ссылки — как
//! `link_parts` образцов `tests/fixtures/relink/links.json`: псевдоним — после
//! первой `|` (`\|` в таблице — тоже разделитель), заголовок — после первого
//! `#` до псевдонима; пробелы и табуляции по краям частей не входят.

use std::ops::Range;

use super::{Line, SpannedLink, WikiLink};

/// Ссылки абзаца `range` (строки `lines`, код в строке `code` по порядку).
pub(super) fn find(
    text: &str,
    range: Range<usize>,
    code: &[Range<usize>],
    lines: &[Line],
    out: &mut Vec<SpannedLink>,
) {
    let b = text.as_bytes();
    let (start, end) = (range.start, range.end);
    let mut span = 0;
    let mut line = 0;
    let mut i = start;
    while i + 1 < end {
        while span < code.len() && code[span].end <= i {
            span += 1;
        }
        if let Some(c) = code.get(span)
            && c.start <= i
        {
            i = c.end;
            continue;
        }
        if b[i] != b'[' || b[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let code_start = code.get(span).map_or(end, |c| c.start);
        let mut open = i;
        let mut k = i + 2;
        let mut closed = None;
        while k < code_start {
            match b[k] {
                b'\n' | b'\r' => break,
                b']' if b.get(k + 1) == Some(&b']') => {
                    closed = Some(k);
                    break;
                }
                b'[' if b.get(k + 1) == Some(&b'[') => open = k,
                _ => {}
            }
            k += 1;
        }
        let Some(close) = closed.filter(|&c| c + 2 <= code_start) else {
            i = k.max(i + 2);
            continue;
        };
        let embed = open > start && b[open - 1] == b'!';
        let from = if embed { open - 1 } else { open };
        while line + 1 < lines.len() && lines[line + 1].start <= from {
            line += 1;
        }
        let number = lines.get(line).map_or(1, |l| l.number);
        out.push(parts(text, from..close + 2, open + 2..close, embed, number));
        i = close + 2;
    }
}

/// Цель, заголовок и псевдоним ссылки `span` с текстом между скобками `inner`.
fn parts(
    text: &str,
    span: Range<usize>,
    inner: Range<usize>,
    embed: bool,
    line: u32,
) -> SpannedLink {
    let s = &text[inner.clone()];
    let (mut target_end, alias) = match s.find('|') {
        Some(bar) => {
            let end = if bar > 0 && s.as_bytes()[bar - 1] == b'\\' {
                bar - 1
            } else {
                bar
            };
            (inner.start + end, Some(inner.start + bar + 1..inner.end))
        }
        None => (inner.end, None),
    };
    let heading = text[inner.start..target_end].find('#').map(|hash| {
        let h = inner.start + hash + 1..target_end;
        target_end = inner.start + hash;
        h
    });
    let target_span = trim(text, inner.start..target_end);
    let heading_span = heading.map(|r| trim(text, r));
    let alias_span = alias.map(|r| trim(text, r));
    let owned = |r: &Option<Range<usize>>| r.clone().map(|r| text[r].to_owned());
    SpannedLink {
        link: WikiLink {
            embed,
            target: text[target_span.clone()].to_owned(),
            heading: owned(&heading_span),
            alias: owned(&alias_span),
            span,
            target_span,
            line,
        },
        heading_span,
        alias_span,
    }
}

/// Без пробелов и табуляций по краям.
fn trim(text: &str, r: Range<usize>) -> Range<usize> {
    let b = text.as_bytes();
    let (mut start, mut end) = (r.start, r.end);
    while start < end && matches!(b[start], b' ' | b'\t') {
        start += 1;
    }
    while end > start && matches!(b[end - 1], b' ' | b'\t') {
        end -= 1;
    }
    start..end
}
