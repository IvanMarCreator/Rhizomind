//! Разбор текста заметки для индекса (выпуск 0.2, С2-3.3, С2-5.1; Р1, Р5,
//! Р14, Р15).
//!
//! Один проход по тексту: блок свойств (из него — только теги `tags`/`tag`),
//! код в строке и блоки с оградой, заголовки ATX, вики-ссылки `[[…]]` и
//! `![[…]]` с байтовыми положениями, теги в тексте. Правила — те же, что у
//! редактора (`src/editor/live-preview/markdown.ts`, `src/core/wikilinks/`,
//! `src/core/tags/`); общий набор образцов — `tests/fixtures/` (задача
//! 0.2-0.3). Свой разбор, а не `pulldown-cmark`: индекс обязан видеть ссылки
//! и теги ровно там же, где их подсвечивает редактор (Р1,
//! `docs/ARCHITECTURE.md`).
//!
//! Модуль чистый: без диска, состояния и паники на любом входе. Интерфейс —
//! [`parse`], [`tag_key`] и типы результата. Модуль [`crate::index`] хранит
//! результат разбора, [`crate::relink`] находит по нему ссылки заново перед
//! переписыванием.
//!
//! Как устроен проход (задача 0.2-1.1): блок свойств в начале
//! ([`frontmatter`]), затем строки текста по порядку: каждая строка
//! относится к блоку кода (с оградой или с отступом) или к абзацу
//! ([`code::Blocks`]); строка абзаца может быть заголовком ATX
//! ([`headings`]). Когда абзац закончился, в нём ищутся код в строке
//! ([`code::code_spans`]), вики-ссылки ([`wikilinks`]) и теги вне кода, ссылок
//! и адресов ссылок Markdown ([`tags`]). Каждый поиск — линейный по длине
//! абзаца, без возвратов: время разбора растёт с размером текста линейно.
//! Классы символов Unicode для тегов (Р17 п.3–4) — свои таблицы
//! ([`unicode_tables`]), общие с редактором.

mod code;
mod frontmatter;
mod headings;
mod tags;
mod unicode_tables;
mod wikilinks;

use std::ops::Range;

/// Номер правил разбора (тегов, ссылок, заголовков). Хранится в
/// `index.json` (`rules_version`): другой номер — индекс пересобирается
/// (С2-4.1). Меняется при любом изменении правил, видимом в результате.
pub const RULES_VERSION: u32 = 1;

/// Заметка больше этого размера — блок свойств не распознаётся (решение 77,
/// как у редактора); остальной разбор — как обычно.
pub const FRONTMATTER_MAX_NOTE_BYTES: usize = 10 * 1024 * 1024;

/// Результат разбора заметки: всё, что из текста попадает в индекс. Самого
/// текста здесь нет (Р13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedNote {
    /// Заголовки ATX по порядку.
    pub headings: Vec<Heading>,
    /// Теги блока свойств и текста по порядку появления (повторы — тоже).
    pub tags: Vec<Tag>,
    /// Вики-ссылки и встраивания по порядку. Ссылки в коде и в блоке
    /// свойств сюда не попадают (решения 86, 87, Р5).
    pub links: Vec<WikiLink>,
}

/// Заголовок ATX (`## Текст`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Уровень 1–6.
    pub level: u8,
    /// Текст заголовка без `#` и концевых `#`.
    pub text: String,
    /// Номер строки, с 1.
    pub line: u32,
}

/// Где стоит тег.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagSource {
    /// В тексте заметки (`#тег`).
    Text,
    /// В блоке свойств (`tags:`/`tag:`).
    Frontmatter,
}

/// Тег (С2-3.3): плоский, `-` и `/` — часть имени (решение 113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Имя тега без `#`, как написано (для отображаемого написания).
    pub name: String,
    /// В тексте или в блоке свойств.
    pub source: TagSource,
    /// Номер строки, с 1.
    pub line: u32,
}

/// Вики-ссылка или встраивание с положениями в байтах UTF-8 от начала
/// текста.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// `![[…]]` — встраивание (учитывается как обратная ссылка, Р4).
    pub embed: bool,
    /// Цель — текст между `[[` и `#`, `|` или `]]`, как в файле
    /// (`Альфа/Встреча`, `/Встреча`); разбор цели — `links::parse_target`.
    pub target: String,
    /// Заголовок после `#`.
    pub heading: Option<String>,
    /// Псевдоним после `|`.
    pub alias: Option<String>,
    /// Вся ссылка: от `[[` (или `!` встраивания) до `]]` включительно.
    pub span: Range<usize>,
    /// Цель — единственное, что переписывает `relink` (С2-3.5).
    pub target_span: Range<usize>,
    /// Номер строки начала ссылки, с 1.
    pub line: u32,
}

/// Разбирает текст заметки. Не паникует ни на каком входе; текст больше
/// [`FRONTMATTER_MAX_NOTE_BYTES`] — без блока свойств.
pub fn parse(text: &str) -> ParsedNote {
    scan(text).into_parsed()
}

#[cfg(test)]
mod tests;

/// Тег с положением в тексте. Положения (`span` здесь и ниже) сверяются с
/// образцами и нужны переписыванию ссылок (`relink::rewrite`, задача 0.2-4.4);
/// индексу ([`parse`]) — нет.
#[derive(Debug, Clone)]
pub(crate) struct SpannedTag {
    pub tag: Tag,
    #[cfg_attr(not(test), allow(dead_code))]
    pub span: Range<usize>,
}

/// Заголовок с положением текста заголовка.
#[derive(Debug, Clone)]
pub(crate) struct SpannedHeading {
    pub heading: Heading,
    #[cfg_attr(not(test), allow(dead_code))]
    pub span: Range<usize>,
}

/// Ссылка с положениями заголовка и псевдонима.
#[derive(Debug, Clone)]
pub(crate) struct SpannedLink {
    pub link: WikiLink,
    #[cfg_attr(not(test), allow(dead_code))]
    pub heading_span: Option<Range<usize>>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub alias_span: Option<Range<usize>>,
}

#[derive(Debug, Default)]
pub(crate) struct Scanned {
    pub frontmatter: Option<Range<usize>>,
    pub headings: Vec<SpannedHeading>,
    pub tags: Vec<SpannedTag>,
    pub links: Vec<SpannedLink>,
}

impl Scanned {
    fn into_parsed(self) -> ParsedNote {
        ParsedNote {
            headings: self.headings.into_iter().map(|h| h.heading).collect(),
            tags: self.tags.into_iter().map(|t| t.tag).collect(),
            links: self.links.into_iter().map(|l| l.link).collect(),
        }
    }
}

/// Разбор с положениями (заголовков, тегов, частей ссылок) — для образцов и
/// для [`parse`].
pub(crate) fn scan(text: &str) -> Scanned {
    let mut out = Scanned::default();
    let mut body = Some((0, 1));
    if text.len() <= FRONTMATTER_MAX_NOTE_BYTES
        && let Some(block) = frontmatter::parse(text, &mut out.tags)
    {
        out.frontmatter = Some(block.span);
        body = block.body;
    }
    let Some((from, number)) = body else {
        return out;
    };
    let mut blocks = code::Blocks::default();
    let mut paragraph: Vec<Line> = Vec::new();
    for line in Lines::new(text, from, number) {
        match blocks.classify(&text[line.range()]) {
            code::Kind::Text => {
                if let Some((level, span)) = headings::parse_line(text, line.range()) {
                    let heading = Heading {
                        level,
                        text: text[span.clone()].to_owned(),
                        line: line.number,
                    };
                    out.headings.push(SpannedHeading { heading, span });
                }
                paragraph.push(line);
            }
            code::Kind::Blank | code::Kind::Code => {
                inline(text, &paragraph, &mut out);
                paragraph.clear();
            }
        }
    }
    inline(text, &paragraph, &mut out);
    out
}

/// Строки абзаца: код в строке, вики-ссылки, теги.
fn inline(text: &str, paragraph: &[Line], out: &mut Scanned) {
    let (Some(first), Some(last)) = (paragraph.first(), paragraph.last()) else {
        return;
    };
    let range = first.start..last.end;
    let code = code::code_spans(text, range.clone());
    let first_link = out.links.len();
    wikilinks::find(text, range, &code, paragraph, &mut out.links);
    let links: Vec<Range<usize>> = out.links[first_link..]
        .iter()
        .map(|l| l.link.span.clone())
        .collect();
    tags::find_in_paragraph(text, paragraph, &code, &links, &mut out.tags);
}

/// Строка текста: `start..end` без перевода строки, номер с 1.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line {
    pub start: usize,
    pub end: usize,
    pub number: u32,
}

impl Line {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// Строки с позиции `from`: переводы строки `\r\n`, `\r` и `\n` (как
/// `splitLines` в `src/core/wikilinks` и CodeMirror). После последнего
/// перевода строки — ещё одна (возможно, пустая) строка.
pub(crate) struct Lines<'a> {
    bytes: &'a [u8],
    next: Option<usize>,
    number: u32,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str, from: usize, number: u32) -> Self {
        Self {
            bytes: text.as_bytes(),
            next: Some(from),
            number,
        }
    }
}

impl Iterator for Lines<'_> {
    type Item = Line;

    fn next(&mut self) -> Option<Line> {
        let start = self.next?;
        let rest = self.bytes.get(start..)?;
        let number = self.number;
        self.number = self.number.saturating_add(1);
        match rest.iter().position(|&b| b == b'\n' || b == b'\r') {
            Some(at) => {
                let end = start + at;
                let crlf = self.bytes[end] == b'\r' && self.bytes.get(end + 1) == Some(&b'\n');
                self.next = Some(end + if crlf { 2 } else { 1 });
                Some(Line { start, end, number })
            }
            None => {
                self.next = None;
                Some(Line {
                    start,
                    end: self.bytes.len(),
                    number,
                })
            }
        }
    }
}

/// Имя подходит под правило тега (С2-3.3): только символы тела тега, хотя
/// бы одна не-цифра, `/` не в начале, не в конце и не два подряд, не длиннее
/// 200 кодовых точек после NFC. Проверка тегов из `index.json` при загрузке.
pub fn is_valid_tag_name(name: &str) -> bool {
    tags::valid_name(name)
}

/// Ключ тега: как ключ имени заметки (NFC → приведение регистра → NFC,
/// `names::key`). `#Проект` и `#проект` — один тег, `#ё` и `#е` — разные.
pub fn tag_key(name: &str) -> String {
    crate::names::key(name).as_str().to_owned()
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn tag_key_folds_case_like_note_names() {
        assert_eq!(tag_key("Проект"), tag_key("проект"));
        assert_ne!(tag_key("ё"), tag_key("е"));
        assert_eq!(tag_key("работа/Отчёт"), "работа/отчёт");
    }
}
