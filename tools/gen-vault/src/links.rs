//! Вики-ссылки: создание, разрешение по правилам С3.4 и учёт для манифеста.

use crate::layout::{Group, Layout, Special};
use crate::names::name_key;
use crate::rng::Rng;
use crate::shortest::shortest_link;
use crate::words::ALIASES;

/// Чем разрешается ссылка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Ровно одна заметка.
    Unique(usize),
    /// Ссылка без пути, заметок с этим именем несколько — ведёт на самую старую.
    Oldest(Vec<usize>),
    Unresolved,
}

/// Записанная в заметку ссылка.
#[derive(Debug, Clone)]
pub struct LinkRec {
    pub source: usize,
    /// Текст ссылки со скобками, без `!` встраивания.
    pub raw: String,
    /// Встраивание `![[…]]`.
    pub embed: bool,
    pub target_text: String,
    pub heading: Option<String>,
    pub alias: bool,
    pub target: Target,
    /// Имя в ссылке написано не так, как имя файла (регистр, форма Unicode).
    pub case_variant: bool,
}

/// Разрешение текста цели ссылки (без `#` и `|`): `/` в начале — путь от
/// корня, иначе — совпадение конца пути; сравнение по ключу имени.
pub fn resolve(l: &Layout, target_text: &str) -> Target {
    let (anchored, body) = match target_text.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, target_text),
    };
    let want: Vec<String> = body.split('/').map(name_key).collect();
    let Some(group) = l.by_key.get(want.last().unwrap()) else {
        return Target::Unresolved;
    };
    let hits: Vec<usize> = group
        .iter()
        .copied()
        .filter(|&i| {
            let have: Vec<String> = l.note_link_path(i).iter().map(|s| name_key(s)).collect();
            if anchored {
                have == want
            } else {
                have.ends_with(&want)
            }
        })
        .collect();
    match hits.len() {
        0 => Target::Unresolved,
        1 => Target::Unique(hits[0]),
        _ => {
            assert!(
                want.len() == 1 && !anchored,
                "ссылка с путём неоднозначна: {target_text}"
            );
            Target::Oldest(hits)
        }
    }
}

/// Сборщик ссылок и `[[...]]` внутри кода по всему хранилищу.
pub struct Links<'a> {
    pub layout: &'a Layout,
    shortest: Vec<String>,
    pub records: Vec<LinkRec>,
    pub in_code: usize,
    unresolved_n: usize,
}

impl<'a> Links<'a> {
    pub fn new(layout: &'a Layout) -> Self {
        let shortest = (0..layout.notes.len())
            .map(|i| shortest_link(layout, i))
            .collect();
        Links {
            layout,
            shortest,
            records: Vec::new(),
            in_code: 0,
            unresolved_n: 0,
        }
    }

    /// Ссылка с заданным текстом; записывается и возвращается со скобками.
    pub fn link(
        &mut self,
        source: usize,
        target_text: &str,
        heading: Option<&str>,
        alias: Option<&str>,
    ) -> String {
        self.record(source, target_text, heading, alias, false)
    }

    /// Ссылка или встраивание (`embed`): записывается и возвращается как в тексте.
    fn record(
        &mut self,
        source: usize,
        target_text: &str,
        heading: Option<&str>,
        alias: Option<&str>,
        embed: bool,
    ) -> String {
        let mut raw = format!("[[{target_text}");
        if let Some(h) = heading {
            raw.push('#');
            raw.push_str(h);
        }
        if let Some(a) = alias {
            raw.push('|');
            raw.push_str(a);
        }
        raw.push_str("]]");
        let target = resolve(self.layout, target_text);
        let written = target_text.rsplit('/').next().unwrap();
        let case_variant =
            matches!(target, Target::Unique(t) if self.layout.notes[t].name != written);
        let written_raw = if embed {
            format!("!{raw}")
        } else {
            raw.clone()
        };
        self.records.push(LinkRec {
            source,
            embed,
            raw,
            target_text: target_text.to_string(),
            heading: heading.map(str::to_string),
            alias: alias.is_some(),
            target,
            case_variant,
        });
        written_raw
    }

    /// Случайная заметка-цель для ссылки из `source`: не она сама, не имя с `#`,
    /// не «узел» и не большая папка (на них ведут только заданные ссылки).
    fn random_target(&self, source: usize, rng: &mut Rng) -> usize {
        let notes = &self.layout.notes;
        loop {
            let t = rng.below(notes.len());
            if t != source
                && notes[t].special != Special::NotLinked
                && notes[t].group == Group::General
            {
                return t;
            }
        }
    }

    /// Встраивание `![[…]]` случайной заметки, иногда — её заголовка.
    pub fn embed(&mut self, source: usize, rng: &mut Rng) -> String {
        let t = self.random_target(source, rng);
        let note = &self.layout.notes[t];
        let heading = rng.chance(20).then(|| rng.pick(&note.headings).clone());
        let text = self.shortest[t].clone();
        self.record(source, &text, heading.as_deref(), None, true)
    }

    /// Заданная ссылка из `source` на «узел» или заметку большой папки — в
    /// разных видах: имя, псевдоним, заголовок, встраивание, путь.
    pub fn directed(&mut self, source: usize, target: usize, rng: &mut Rng) -> String {
        let l = self.layout;
        let note = &l.notes[target];
        let name = note.name.as_str();
        let alias = |rng: &mut Rng| Some(*rng.pick(ALIASES));
        let heading = |rng: &mut Rng| rng.pick(&note.headings).clone();
        match note.group {
            Group::Hub => match rng.below(100) {
                0..=39 => self.link(source, name, None, None),
                40..=54 => self.link(source, name, None, alias(rng)),
                55..=69 => self.link(source, name, Some(&heading(rng)), None),
                70..=84 => self.record(source, name, None, None, true),
                _ => self.link(source, &format!("/{name}"), None, None),
            },
            Group::Folder => {
                let full = l.note_link_path(target).join("/");
                let part = format!("{}/{name}", l.folders[note.folder].last().unwrap());
                let roll = rng.below(100);
                if l.notes[source].group == Group::Folder {
                    let text = match roll {
                        0..=49 => name.to_string(),
                        50..=79 => part,
                        _ => full,
                    };
                    return self.link(source, &text, None, None);
                }
                match roll {
                    0..=39 => self.link(source, name, None, None),
                    40..=49 => self.link(source, name, None, alias(rng)),
                    50..=74 => self.link(source, &full, None, None),
                    75..=84 => self.link(source, &format!("/{full}"), None, None),
                    85..=91 => self.link(source, &part, None, None),
                    92..=95 => self.link(source, name, Some(&heading(rng)), None),
                    _ => self.record(source, name, None, None, true),
                }
            }
            Group::General => {
                let text = self.shortest[target].clone();
                self.link(source, &text, None, None)
            }
        }
    }

    /// Текст кода с `[[...]]` внутри: учитывается, но ссылкой не является.
    pub fn code(&mut self, text: String) -> String {
        self.in_code += text.matches("[[").count();
        text
    }

    /// Случайная ссылка из заметки `source`: на другую заметку в одном из
    /// видов (имя, кратчайший путь, полный путь, от корня, без пути при
    /// дубликатах, другой регистр, заголовок, псевдоним) или неразрешённая.
    pub fn random_link(&mut self, source: usize, rng: &mut Rng) -> String {
        let alias = rng.chance(25).then(|| *rng.pick(ALIASES));
        if rng.chance(6) {
            self.unresolved_n += 1;
            let n = self.unresolved_n;
            let text = match rng.below(4) {
                0 => format!("Несуществующая заметка {n}"),
                1 => format!("Нет такой папки/Заметка {n}"),
                2 => format!("/Нет в корне {n}"),
                _ => format!("Missing note {n}"),
            };
            return self.link(source, &text, None, alias);
        }
        let l = self.layout;
        let t = self.random_target(source, rng);
        let note = &l.notes[t];
        let unique = l.by_key[&name_key(&note.name)].len() == 1;
        let roll = rng.below(100);
        let mut text = if unique && note.folder == 0 && roll < 20 {
            format!("/{}", note.name)
        } else if unique && note.folder != 0 && roll < 10 {
            l.note_link_path(t).join("/")
        } else if !unique && roll < 15 {
            note.name.clone()
        } else {
            self.shortest[t].clone()
        };
        if unique && rng.chance(4) {
            let (folders, name) = match text.rsplit_once('/') {
                Some((f, n)) => (format!("{f}/"), n.to_string()),
                None => (String::new(), text.clone()),
            };
            text = format!("{folders}{}", name.to_lowercase());
        }
        let heading = rng.chance(20).then(|| {
            if rng.chance(5) {
                "Нет такого заголовка".to_string()
            } else {
                rng.pick(&note.headings).clone()
            }
        });
        self.link(source, &text, heading.as_deref(), alias)
    }
}
