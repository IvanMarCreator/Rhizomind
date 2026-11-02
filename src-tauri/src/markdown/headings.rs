//! Заголовки ATX (`## Текст`), как `HEADING` и `headingClose` редактора
//! (`src/editor/live-preview/markdown.ts`): до трёх пробелов отступа, 1–6
//! `#`, затем пробел, табуляция или конец строки. Закрывающие `#` (после
//! пробела или табуляции) и пробелы по краям в текст не входят; разметка в
//! тексте не убирается (образцы `tests/fixtures/headings/text.json`).

use std::ops::Range;

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Уровень и положение текста заголовка в строке `line` текста `text`. У
/// пустого заголовка положение — пустое, в конце строки.
pub(super) fn parse_line(text: &str, line: Range<usize>) -> Option<(u8, Range<usize>)> {
    let bytes = text.as_bytes();
    let mut i = line.start;
    while i < line.end && i - line.start < 3 && bytes[i] == b' ' {
        i += 1;
    }
    let marks = bytes[i..line.end]
        .iter()
        .take_while(|&&b| b == b'#')
        .count();
    if !(1..=6).contains(&marks) {
        return None;
    }
    let from = i + marks;
    if from < line.end && !is_blank(bytes[from]) {
        return None;
    }
    // Закрывающие `#`: с конца строки — пробелы, `#`, перед ними пробел.
    let mut end = line.end;
    while end > from && is_blank(bytes[end - 1]) {
        end -= 1;
    }
    let mut hashes = end;
    while hashes > from && bytes[hashes - 1] == b'#' {
        hashes -= 1;
    }
    if hashes < end {
        let mut spaces = hashes;
        while spaces > from && is_blank(bytes[spaces - 1]) {
            spaces -= 1;
        }
        if spaces < hashes {
            end = spaces;
        }
    }
    let mut start = from;
    while start < end && is_blank(bytes[start]) {
        start += 1;
    }
    let level = u8::try_from(marks).ok()?;
    if start == end {
        return Some((level, line.end..line.end));
    }
    Some((level, start..end))
}
