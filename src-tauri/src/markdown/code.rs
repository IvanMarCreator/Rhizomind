//! Код: блоки с оградой ``` и ~~~, код с отступом, код в строке (решения
//! 84–87, Р17 п.5). Правила — как `paragraphs` и `codeSpans` в
//! `src/core/wikilinks/index.ts` (образцы `tests/fixtures/wikilinks/text.json`).

use std::collections::HashMap;
use std::ops::Range;

/// К чему относится строка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Пустая строка (только пробелы и табуляции).
    Blank,
    /// Строка блока кода (ограда, содержимое, код с отступом).
    Code,
    /// Строка абзаца.
    Text,
}

/// Состояние разбора блоков: открытая ограда, вид предыдущей строки, список.
#[derive(Debug, Default)]
pub(super) struct Blocks {
    /// Символ и длина открывающей ограды.
    fence: Option<(u8, usize)>,
    /// `None` — начало текста.
    previous: Option<Kind>,
    in_list: bool,
}

impl Blocks {
    /// Вид очередной строки (без перевода строки).
    pub fn classify(&mut self, line: &str) -> Kind {
        let kind = if let Some((mark, len)) = self.fence {
            if closes_fence(line, mark, len) {
                self.fence = None;
            }
            Kind::Code
        } else if line.bytes().all(|b| b == b' ' || b == b'\t') {
            Kind::Blank
        } else {
            let indent = indent_columns(line);
            let after_break = self.previous != Some(Kind::Text);
            if indent >= 4 && after_break && !self.in_list {
                Kind::Code
            } else if let Some(fence) = opens_fence(line) {
                self.fence = Some(fence);
                Kind::Code
            } else {
                if is_list_item(line) {
                    self.in_list = true;
                } else if self.previous == Some(Kind::Blank) && indent < 2 {
                    self.in_list = false;
                }
                Kind::Text
            }
        };
        self.previous = Some(kind);
        kind
    }
}

/// Ширина отступа в колонках: табуляция — до следующей кратной 4.
fn indent_columns(line: &str) -> usize {
    let mut columns = 0;
    for b in line.bytes() {
        match b {
            b' ' => columns += 1,
            b'\t' => columns += 4 - columns % 4,
            _ => break,
        }
    }
    columns
}

/// До трёх пробелов отступа.
fn skip_three_spaces(line: &str) -> &str {
    let spaces = line.bytes().take(3).take_while(|&b| b == b' ').count();
    &line[spaces..]
}

/// Открывающая ограда: до трёх пробелов, затем три и больше `` ` `` или `~`;
/// после `` ` `` строка не содержит `` ` ``. Как `FENCE_OPEN` редактора: в
/// регулярном выражении JavaScript `.` не совпадает с U+2028 и U+2029, поэтому
/// строка с ними — не ограда.
fn opens_fence(line: &str) -> Option<(u8, usize)> {
    let rest = skip_three_spaces(line);
    let mark = *rest.as_bytes().first()?;
    if mark != b'`' && mark != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == mark).count();
    let info = &rest[len..];
    if len < 3 || info.contains(['\u{2028}', '\u{2029}']) || (mark == b'`' && info.contains('`')) {
        return None;
    }
    Some((mark, len))
}

/// Закрывающая ограда: до трёх пробелов, те же символы не короче открывающих,
/// дальше только пробелы и табуляции.
fn closes_fence(line: &str, mark: u8, len: usize) -> bool {
    let rest = skip_three_spaces(line);
    let run = rest.bytes().take_while(|&b| b == mark).count();
    run >= len && rest[run..].bytes().all(|b| b == b' ' || b == b'\t')
}

/// Пункт списка: до трёх пробелов, `-`, `*`, `+` или число из 1–9 цифр с `.`
/// или `)`, затем пробел, табуляция или конец строки.
fn is_list_item(line: &str) -> bool {
    let rest = skip_three_spaces(line).as_bytes();
    let after = match rest.first() {
        Some(b'-' | b'*' | b'+') => 1,
        Some(b'0'..=b'9') => {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 9 || !matches!(rest.get(digits), Some(b'.' | b')')) {
                return false;
            }
            digits + 1
        }
        _ => return false,
    };
    matches!(rest.get(after), None | Some(b' ' | b'\t'))
}

/// Код в строке на участке абзаца: серия обратных апострофов и следующая
/// серия той же длины (серии берутся целиком). Серия без пары — не код.
/// За один проход: для каждой длины — очередь серий этой длины.
pub(super) fn code_spans(text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let bytes = &text.as_bytes()[range.clone()];
    if !bytes.contains(&b'`') {
        return Vec::new();
    }
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let len = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            runs.push((range.start + i, len));
            i += len;
        } else {
            i += 1;
        }
    }
    // Длина → номера серий этой длины по порядку и указатель на первую ещё
    // не пройденную.
    let mut by_len: HashMap<usize, (Vec<usize>, usize)> = HashMap::new();
    for (n, &(_, len)) in runs.iter().enumerate() {
        by_len.entry(len).or_default().0.push(n);
    }
    let mut spans = Vec::new();
    let mut r = 0;
    while r < runs.len() {
        let (start, len) = runs[r];
        let queue = by_len.get_mut(&len).expect("серия этой длины учтена");
        while queue.1 < queue.0.len() && queue.0[queue.1] <= r {
            queue.1 += 1;
        }
        match queue.0.get(queue.1) {
            Some(&close) => {
                spans.push(start..runs[close].0 + len);
                r = close + 1;
            }
            None => r += 1,
        }
    }
    spans
}
