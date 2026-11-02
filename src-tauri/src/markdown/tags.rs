//! Теги в тексте (С2-3.3, Р15, Р17, решение 113).
//!
//! `#` — начало тега, если перед ним начало строки, пробельный символ
//! (Unicode White_Space) или знак категорий P*/S*, кроме `& / \ # _`; `#` не
//! внутри кода, вики-ссылки и адреса ссылки Markdown `[текст](адрес)`. Тело —
//! буквы (L*), комбинируемые знаки (M*), десятичные цифры (Nd), `_`, `-`,
//! `/`; тег кончается на первом другом символе. Тело не тег целиком, если
//! в нём одни цифры, `/` в начале, в конце или два подряд, или оно длиннее
//! 200 кодовых точек после NFC. Классы символов — свои таблицы Unicode
//! ([`super::unicode_tables`]), те же у редактора.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use super::unicode_tables::{self as table, DECIMAL_DIGIT, PUNCT_SYMBOL, TAG_WORD, WHITE_SPACE};
use super::{Line, SpannedTag, Tag, TagSource};

/// Наибольшая длина имени тега в кодовых точках после NFC (Р17 п.1).
pub(super) const MAX_TAG_CHARS: usize = 200;

/// Символ тела тега.
fn is_body(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/')
    } else {
        table::contains(TAG_WORD, c)
    }
}

fn is_digit(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_digit()
    } else {
        table::contains(DECIMAL_DIGIT, c)
    }
}

/// Пробельный символ — Unicode White_Space (Р17 п.4).
pub(super) fn is_space(c: char) -> bool {
    table::contains(WHITE_SPACE, c)
}

/// Символ, после которого `#` может начинать тег (Р17 п.3).
fn allows_tag_after(c: char) -> bool {
    if c.is_ascii() {
        // В ASCII все видимые символы, кроме букв и цифр, — категории P* и S*.
        c.is_ascii_whitespace()
            || c == '\x0b'
            || (c.is_ascii_punctuation() && !matches!(c, '&' | '/' | '\\' | '#' | '_'))
    } else {
        is_space(c) || table::contains(PUNCT_SYMBOL, c)
    }
}

/// Имя подходит под правило тега целиком (без `#`): непустое, только символы
/// тела, хотя бы одна не-цифра, `/` не в начале, не в конце и не два подряд,
/// не длиннее 200 кодовых точек после NFC. Так же проверяются значения блока
/// свойств.
pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(is_body)
        && !name.chars().all(is_digit)
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("//")
        && (name.len() <= MAX_TAG_CHARS
            || name.nfc().take(MAX_TAG_CHARS + 1).count() <= MAX_TAG_CHARS)
}

/// Теги абзаца (строки `lines`), кроме `#` в коде `code` и ссылках `links`
/// (оба списка — по порядку).
pub(super) fn find_in_paragraph(
    text: &str,
    lines: &[Line],
    code: &[Range<usize>],
    links: &[Range<usize>],
    out: &mut Vec<SpannedTag>,
) {
    let mut excluded = Excluded::new(code, links);
    let mut urls = Vec::new();
    for line in lines {
        let bytes = &text.as_bytes()[line.range()];
        if !bytes.contains(&b'#') {
            continue;
        }
        urls.clear();
        if bytes.contains(&b'[') {
            link_destinations(text, line.range(), &excluded, &mut urls);
            urls.sort_by_key(|r: &Range<usize>| r.start);
        }
        let mut url = 0;
        let mut i = line.start;
        while let Some(at) = text.as_bytes()[i..line.end].iter().position(|&b| b == b'#') {
            let p = i + at;
            i = p + 1;
            while url < urls.len() && urls[url].end <= p {
                url += 1;
            }
            if excluded.covers(p) || urls.get(url).is_some_and(|u| u.start <= p) {
                continue;
            }
            let prefix_ok =
                p == line.start || text[..p].chars().next_back().is_some_and(allows_tag_after);
            if !prefix_ok {
                continue;
            }
            let body = text[p + 1..line.end]
                .find(|c: char| !is_body(c))
                .map_or(line.end, |n| p + 1 + n);
            let name = &text[p + 1..body];
            if valid_name(name) {
                let tag = Tag {
                    name: name.to_owned(),
                    source: TagSource::Text,
                    line: line.number,
                };
                out.push(SpannedTag { tag, span: p..body });
                i = body;
            }
        }
    }
}

/// Код в строке и вики-ссылки абзаца: «внутри ли позиция» для позиций по
/// возрастанию.
struct Excluded<'a> {
    lists: [(&'a [Range<usize>], usize); 2],
}

impl<'a> Excluded<'a> {
    fn new(code: &'a [Range<usize>], links: &'a [Range<usize>]) -> Self {
        Self {
            lists: [(code, 0), (links, 0)],
        }
    }

    /// Позиция внутри участка; позиции запрашиваются по возрастанию.
    fn covers(&mut self, at: usize) -> bool {
        self.lists.iter_mut().any(|(list, next)| {
            while *next < list.len() && list[*next].end <= at {
                *next += 1;
            }
            list.get(*next).is_some_and(|r| r.start <= at)
        })
    }

    /// Участок, который начинается в позиции `at` или содержит её (без сдвига
    /// указателей — для поиска ссылок Markdown, который идёт своим ходом).
    fn containing(&self, at: usize) -> Option<Range<usize>> {
        self.lists.iter().find_map(|(list, _)| {
            let n = list.partition_point(|r| r.end <= at);
            list.get(n).filter(|r| r.start <= at).cloned()
        })
    }
}

/// Адреса ссылок Markdown `[текст](адрес "заголовок")` и картинок
/// `![текст](адрес)` в строке — от `]` до `)` включительно: там тегов нет (Р17
/// п.6). Как `link` и `destination` редактора: `[` в коде, вики-ссылке или
/// после `\` не открывает ссылку; пара `]` — с учётом вложенности и `\`;
/// адрес — в `<…>` или до пробельного символа или `)` без пары; заголовок —
/// в `"…"`, `'…'` или `(…)`. Текст ссылки разбирается как обычный текст.
fn link_destinations(
    text: &str,
    line: Range<usize>,
    excluded: &Excluded<'_>,
    out: &mut Vec<Range<usize>>,
) {
    let b = text.as_bytes();
    let mut ctx = Destinations::new(text, line.clone());
    // Границы вложенных текстов ссылок: дойдя до `]`, разбор продолжается
    // после адреса.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut to = line.end;
    let mut i = line.start;
    loop {
        while let Some(&(close, end)) = stack.last() {
            if i < close {
                break;
            }
            stack.pop();
            i = end;
            to = stack.last().map_or(line.end, |&(close, _)| close);
        }
        if i >= to {
            break;
        }
        if let Some(r) = excluded.containing(i) {
            i = r.end;
            continue;
        }
        match b[i] {
            b'\\' if i + 1 < to && b[i + 1].is_ascii_punctuation() => i += 2,
            b'[' if b.get(i + 1) == Some(&b'[') => i += 2,
            b'[' => {
                let found = ctx
                    .close_bracket(i)
                    .filter(|&c| c < to && b.get(c + 1) == Some(&b'('));
                match found.and_then(|close| ctx.destination(close + 2, to).map(|end| (close, end)))
                {
                    Some((close, end)) => {
                        out.push(close..end);
                        stack.push((close, end));
                        to = close;
                        i += 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
}

/// Поиски по строке для адресов ссылок — каждый один раз на строку, чтобы
/// разбор строки с множеством `[…](` оставался линейным (как `InlineContext`
/// редактора).
struct Destinations<'a> {
    text: &'a str,
    line: Range<usize>,
    /// `[` → парная `]`.
    brackets: Option<Vec<(usize, usize)>>,
    /// Где кончается адрес, начатый в позиции (индекс — от начала строки).
    ends: Option<Vec<usize>>,
    /// Последний поиск символа: (символ, с `\`), с какой позиции, где найден.
    next: Vec<((u8, bool), usize, usize)>,
}

impl<'a> Destinations<'a> {
    fn new(text: &'a str, line: Range<usize>) -> Self {
        Self {
            text,
            line,
            brackets: None,
            ends: None,
            next: Vec::new(),
        }
    }

    fn close_bracket(&mut self, open: usize) -> Option<usize> {
        let (b, line) = (self.text.as_bytes(), self.line.clone());
        let pairs = self.brackets.get_or_insert_with(|| {
            let mut pairs = Vec::new();
            let mut stack = Vec::new();
            let mut i = line.start;
            while i < line.end {
                match b[i] {
                    b'\\' => i += 1,
                    b'[' => stack.push(i),
                    b']' => {
                        if let Some(o) = stack.pop() {
                            pairs.push((o, i));
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            pairs.sort_unstable();
            pairs
        });
        pairs
            .binary_search_by_key(&open, |&(o, _)| o)
            .ok()
            .map(|n| pairs[n].1)
    }

    /// Первое вхождение `target` с позиции `from` до конца строки (`line.end` —
    /// нет); с `escapes` символ после `\` пропускается.
    fn next_of(&mut self, target: u8, from: usize, escapes: bool) -> usize {
        let key = (target, escapes);
        if let Some(&(_, _, at)) = self
            .next
            .iter()
            .find(|(k, f, at)| *k == key && (*f..=*at).contains(&from))
        {
            return at;
        }
        let b = self.text.as_bytes();
        let mut i = from;
        while i < self.line.end && b[i] != target {
            i += if escapes && b[i] == b'\\' { 2 } else { 1 };
        }
        let at = i.min(self.line.end);
        self.next.retain(|(k, _, _)| *k != key);
        self.next.push((key, from, at));
        at
    }

    /// Конец адреса, начатого в позиции: первый пробельный символ или первая
    /// `)` без пары `(` после начала (символ после `\` не в счёт). Два прохода
    /// по строке.
    fn destination_end(&mut self, at: usize) -> usize {
        let (text, line) = (self.text, self.line.clone());
        let ends = self.ends.get_or_insert_with(|| {
            let len = line.end - line.start;
            let mut balance = vec![0i32; len + 1];
            let mut escaped = vec![false; len + 1];
            let mut depth = 0i32;
            let mut chars = text[line.clone()].char_indices().peekable();
            while let Some((i, c)) = chars.next() {
                balance[i] = depth;
                match c {
                    '\\' => {
                        if let Some(&(j, _)) = chars.peek() {
                            escaped[i] = true;
                            escaped[j] = true;
                            balance[j] = depth;
                            chars.next();
                        }
                    }
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            let mut ends = vec![line.end; len + 1];
            let mut space = line.end;
            let mut close_at: std::collections::HashMap<i32, usize> =
                std::collections::HashMap::new();
            for (i, c) in text[line.clone()].char_indices().rev() {
                if !escaped[i] {
                    if is_space(c) {
                        space = line.start + i;
                    } else if c == ')' {
                        close_at.insert(balance[i], line.start + i);
                    }
                }
                ends[i] = space.min(close_at.get(&balance[i]).copied().unwrap_or(line.end));
            }
            ends
        });
        ends.get(at - line.start).copied().unwrap_or(line.end)
    }

    /// Адрес и необязательный заголовок с позиции `start` до `)`; конец — после `)`.
    fn destination(&mut self, start: usize, to: usize) -> Option<usize> {
        let b = self.text.as_bytes();
        let skip = |mut i: usize| {
            while i < to && matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
            i
        };
        let mut i = skip(start);
        if b.get(i) == Some(&b'<') {
            let close = self.next_of(b'>', i + 1, false);
            if close >= to || b[i + 1..close].contains(&b'<') {
                return None;
            }
            i = close + 1;
        } else {
            i = self.destination_end(i).min(to);
        }
        let after_url = i;
        i = skip(i);
        if i > after_url && i < to && matches!(b[i], b'"' | b'\'' | b'(') {
            let closing = if b[i] == b'(' { b')' } else { b[i] };
            let j = self.next_of(closing, i + 1, true);
            if j >= to {
                return None;
            }
            i = skip(j + 1);
        }
        (i < to && b[i] == b')').then_some(i + 1)
    }
}
