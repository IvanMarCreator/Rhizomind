//! Правка байтов ссылок (выпуск 0.2, задача 0.2-4.4; С2-3.5, С2-5.4 шаг 4,
//! С2-9 п. 4, Р17 п. 10).
//!
//! Чистая функция текста: без диска и состояния. Ссылки находятся заново тем
//! же разбором, что у индекса ([`markdown::parse`]): смещения из индекса
//! могли устареть. Меняются только байты целей — между `[[`/`![[` и
//! `#`/`|`/`\|`/`]]` без пробелов по краям; всё остальное (BOM, CRLF/LF,
//! концевые пробелы, заголовок и псевдоним ссылки, пробелы вокруг цели)
//! остаётся байт в байт. Ссылки в коде и в блоке свойств — не ссылки
//! (решения 86, 87, Р5): разбор их не находит, и они не меняются.

use std::ops::Range;

use crate::links::parse_target;
use crate::markdown::{self, WikiLink};
use crate::names;

const BOM: char = '\u{feff}';

/// Символы, которые нельзя вставлять в цель: они изменили бы границы ссылки
/// или разбор вокруг неё (код в строке, другая ссылка, перенос строки).
const UNSAFE_IN_TARGET: &[char] = &['[', ']', '|', '#', '`', '\\', '\r', '\n'];

/// Новый текст заметки: ссылки находятся заново тем же разбором
/// (`markdown::parse`), у каждой `new_target` решает, на что заменить цель
/// (`None` — оставить). Меняются только байты целей; BOM, переносы строк,
/// всё остальное — как было. `None` в ответе — менять нечего.
///
/// `text` — файл целиком, как на диске (BOM в начале допустим и не часть
/// текста: положения в [`WikiLink`] — от начала текста после BOM, как у
/// индекса). `new_target` — цель так, как её вставило бы автодополнение
/// (`Планёрка`, `Бета/Встреча`, `/Альфа/Встреча`); решение «ссылка всё ещё
/// ведёт на нужную заметку» — за ней (С2-5.4, шаг 4).
///
/// Правила замены:
/// - суффикс `.md` в старой цели сохраняется как написан, с регистром
///   (`[[Встреча.MD]]` → `[[Планёрка.MD]]`, Р17 п. 10);
/// - цель, равная прежней, не считается правкой: уже переписанная ссылка
///   (доделка, повтор) файл не меняет;
/// - цель, которая сломала бы ссылку или разбор вокруг (пустая, с пробелом
///   по краям, с `[`, `]`, `|`, `#`, `` ` ``, `\`, переносом строки, или не
///   путь — `..`, `//`, `:`), не вставляется: ссылка остаётся как была;
/// - ссылки без цели (`[[#Заголовок]]`) не меняются.
///
/// Перед ответом новый текст разбирается ещё раз: если ссылок стало другое
/// число или у какой-то не та цель (например, текст перерос предел
/// распознавания блока свойств), ответ — `None`: ничего не меняется.
pub fn rewrite(text: &str, new_target: &dyn Fn(&WikiLink) -> Option<String>) -> Option<String> {
    let (prefix, body) = match text.strip_prefix(BOM) {
        Some(body) => (BOM.len_utf8(), body),
        None => (0, text),
    };
    let links = markdown::parse(body).links;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for link in &links {
        let span = link.target_span.clone();
        let old = &body[span.clone()];
        if old.is_empty() {
            continue;
        }
        let Some(new) = new_target(link) else {
            continue;
        };
        if !writable(&new) {
            continue;
        }
        let new = keep_suffix(old, new);
        if new != old {
            edits.push((span, new));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let added: usize = edits.iter().map(|(_, new)| new.len()).sum();
    let mut out = String::with_capacity(text.len() + added);
    out.push_str(&text[..prefix]);
    let mut at = 0;
    for (span, new) in &edits {
        out.push_str(&body[at..span.start]);
        out.push_str(new);
        at = span.end;
    }
    out.push_str(&body[at..]);
    same_links(&links, &edits, body, &out[prefix..]).then_some(out)
}

/// Цель можно вставить, не меняя границ ссылки и разбора вокруг, и она
/// разбирается как путь без заголовка и псевдонима.
fn writable(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with([' ', '\t'])
        && !target.ends_with([' ', '\t'])
        && !target.contains(UNSAFE_IN_TARGET)
        && parse_target(target).is_ok()
}

/// `.md` старой цели (как написан) переходит в новую. Если новая цель сама
/// кончается на `.md` (имя заметки кончается на `.md`, `Итоги.md.md`), её
/// суффикс заменяется написанием старого.
fn keep_suffix(old: &str, new: String) -> String {
    let Some(old_stem) = names::note_stem(old) else {
        return new;
    };
    let suffix = &old[old_stem.len()..];
    let stem = names::note_stem(&new).unwrap_or(&new);
    format!("{stem}{suffix}")
}

/// Новый текст даёт те же ссылки: столько же, с теми же частями, цели — по
/// правкам.
fn same_links(
    old: &[WikiLink],
    edits: &[(Range<usize>, String)],
    old_body: &str,
    new_body: &str,
) -> bool {
    let new = markdown::parse(new_body).links;
    if new.len() != old.len() {
        return false;
    }
    let mut edits = edits.iter().peekable();
    old.iter().zip(&new).all(|(a, b)| {
        let target = match edits.peek() {
            Some((span, text)) if *span == a.target_span => {
                edits.next();
                text.as_str()
            }
            _ => &old_body[a.target_span.clone()],
        };
        &new_body[b.target_span.clone()] == target
            && a.embed == b.embed
            && a.heading == b.heading
            && a.alias == b.alias
            && a.line == b.line
    })
}

#[cfg(test)]
#[path = "rewrite_tests.rs"]
mod tests;
