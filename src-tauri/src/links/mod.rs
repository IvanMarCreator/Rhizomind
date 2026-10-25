//! Вики-ссылки: разрешение, кратчайший различающий путь, подсказки —
//! интерфейс модуля, реализация в задаче 1.1.
//!
//! Правила — спецификация 0.1, С3.4: разрешение по имени файла без `.md` по
//! ключу имени (модуль `names`); ссылка с путём ведёт ровно на этот файл;
//! ссылка без пути при дубликатах — на самую старую по дате создания, а при
//! равной дате — на первую по пути в естественном алфавитном порядке;
//! `[[/Встреча]]` — заметка в корне. Модуль чистый: работает со списком
//! заметок в памяти, к диску не обращается.

use std::collections::HashMap;

use crate::commands::{CoreError, CoreResult, ErrorCode, LinkSuggestion, VaultPath};
use crate::names::{self, KeyedPath, NameKey};
use crate::switcher::{self, Haystack};
use crate::vault::NoteRef;

/// Цель вики-ссылки после разбора текста между `[[` и `]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    /// Папки пути (`Проекты/Альфа/Встреча` → `["Проекты", "Альфа"]`).
    pub folders: Vec<String>,
    /// Имя заметки без `.md`.
    pub name: String,
    /// Заголовок после `#`.
    pub heading: Option<String>,
    /// Псевдоним после `|`.
    pub alias: Option<String>,
    /// Путь от корня хранилища (`[[/Встреча]]`).
    pub from_root: bool,
}

/// Почему текст ссылки не может быть целью.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidTarget {
    /// Нет имени: `[[]]`, `[[#Заголовок]]`, `[[|псевдоним]]`, `[[/]]`.
    EmptyTarget,
    /// Пустой компонент пути: `[[Альфа//Встреча]]`, `[[Альфа/]]`.
    EmptyComponent,
    /// Компонент `.` или `..`.
    DotComponent,
    /// Запрещённый в имени символ (`\ : * ? " < >`) или управляющий.
    ForbiddenChars,
}

impl InvalidTarget {
    /// Причина строкой — как в образцах `tests/fixtures/wikilinks/parse.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyTarget => "empty_target",
            Self::EmptyComponent => "empty_component",
            Self::DotComponent => "dot_component",
            Self::ForbiddenChars => "forbidden_chars",
        }
    }
}

/// Пробелы и табуляции вокруг цели, заголовка и псевдонима. Другие
/// пробельные символы (неразрывный пробел) Windows разрешает в конце имени,
/// поэтому они остаются.
fn trim_blanks(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

fn non_empty(text: &str) -> Option<String> {
    let text = trim_blanks(text);
    (!text.is_empty()).then(|| text.to_owned())
}

/// Разбор текста между `[[` и `]]` по правилам общих образцов
/// (`tests/fixtures/wikilinks/parse.json`; тот же разбор — в
/// `src/core/wikilinks` интерфейса). Проверяется только синтаксис пути:
/// зарезервированное имя или точка в конце дают допустимую цель, которая
/// просто не разрешается.
pub fn parse_target(inner: &str) -> Result<LinkTarget, InvalidTarget> {
    let (target, alias) = match inner.split_once('|') {
        // `\|` — разделитель псевдонима в таблице Markdown.
        Some((target, alias)) => (
            target.strip_suffix('\\').unwrap_or(target),
            non_empty(alias),
        ),
        None => (inner, None),
    };
    let (path, heading) = match target.split_once('#') {
        Some((path, heading)) => (path, non_empty(heading)),
        None => (target, None),
    };
    let path = trim_blanks(path);
    let (from_root, path) = match path.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, path),
    };
    if path.is_empty() {
        return Err(InvalidTarget::EmptyTarget);
    }
    let mut components: Vec<String> = path.split('/').map(str::to_owned).collect();
    if components.iter().any(String::is_empty) {
        return Err(InvalidTarget::EmptyComponent);
    }
    if components.iter().any(|c| c == "." || c == "..") {
        return Err(InvalidTarget::DotComponent);
    }
    if components
        .iter()
        .any(|c| c.chars().any(names::is_forbidden_char))
    {
        return Err(InvalidTarget::ForbiddenChars);
    }
    let file = components.pop().unwrap_or_default();
    let name = match names::note_stem(&file) {
        Some(stem) => stem.to_owned(),
        None => file,
    };
    Ok(LinkTarget {
        folders: components,
        name,
        heading,
        alias,
        from_root,
    })
}

/// Заметка в списке ссылок с заранее посчитанными ключами.
#[derive(Debug)]
struct Entry {
    path: VaultPath,
    created_ms: i64,
    /// Папки и имя без `.md` — как пишется путь в ссылке.
    link_path: Vec<String>,
    /// Ключи `link_path`.
    keyed: KeyedPath,
    /// Место в порядке путей ядра (при равной дате создания).
    rank: u32,
    haystack: Haystack,
}

impl Entry {
    fn name(&self) -> &str {
        self.link_path.last().map_or("", String::as_str)
    }

    fn keys(&self) -> &[NameKey] {
        self.keyed.keys()
    }
}

/// Ссылки одного хранилища. Список заметок заменяется целиком при изменении
/// дерева (`replace_notes`); запросы не меняют состояние.
#[derive(Debug, Default)]
pub struct Links {
    entries: Vec<Entry>,
    /// Заметки по ключу имени без `.md`.
    by_name: HashMap<NameKey, Vec<usize>>,
    /// Все заметки в порядке имён, затем путей — подсказки на пустой запрос.
    by_name_order: Vec<usize>,
}

impl Links {
    /// Ссылки по списку заметок хранилища (`Vault::notes`).
    pub fn new(notes: Vec<NoteRef>) -> Self {
        let mut links = Self::default();
        links.replace_notes(notes);
        links
    }

    /// Заменяет список заметок (после изменения дерева). Файлы не `.md` в
    /// списке пропускаются. Ключи и порядки считаются здесь, один раз.
    pub fn replace_notes(&mut self, notes: Vec<NoteRef>) {
        let notes: Vec<(NoteRef, Vec<String>)> = notes
            .into_iter()
            .filter_map(|note| {
                let link_path = switcher::link_path(&note.path)?;
                Some((note, link_path))
            })
            .collect();
        let keyed: Vec<KeyedPath> = notes
            .iter()
            .map(|(_, link_path)| KeyedPath::new(link_path))
            .collect();
        let ranks = switcher::natural_ranks(&keyed);
        self.entries = notes
            .into_iter()
            .zip(keyed.into_iter().zip(ranks))
            .map(|((note, link_path), (keyed, rank))| Entry {
                path: note.path,
                created_ms: note.created_ms,
                link_path,
                haystack: Haystack::new(&keyed),
                keyed,
                rank,
            })
            .collect();
        self.by_name.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some(name) = entry.keys().last() {
                self.by_name.entry(name.clone()).or_default().push(i);
            }
        }
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&self.entries[a], &self.entries[b]);
            a.keyed
                .name_sort_key()
                .cmp(&b.keyed.name_sort_key())
                .then(a.rank.cmp(&b.rank))
        });
        self.by_name_order = order;
    }

    /// Куда ведёт цель ссылки `target` (текст между `[[` и `|`/`#`; если
    /// они есть, разбор их отбрасывает) из заметки `from`. `Ok(None)` —
    /// неразрешённая ссылка, в том числе для цели, которая не может быть
    /// путём (`..`, `:` и т.п.): плохая цель — не ошибка.
    ///
    /// Правила (С3.4), сравнение по ключу имени:
    /// - `/путь/имя` — ровно этот путь от корня;
    /// - `папка/имя` — заметки, чей путь кончается на `папка/имя` (заметка
    ///   `Альфа/Встреча.md` в корне не перехватывает ссылку, которая вела на
    ///   `Проекты/Альфа/Встреча.md`, если та старше);
    /// - `имя` — заметки с этим именем;
    ///
    /// из нескольких подходящих — самая старая по дате создания, при равной
    /// дате — первая в порядке путей ядра (`names::natural_cmp`). `from` в
    /// 0.1 на выбор не влияет.
    pub fn resolve(&self, _from: &VaultPath, target: &str) -> CoreResult<Option<VaultPath>> {
        let Ok(target) = parse_target(target) else {
            return Ok(None);
        };
        let want: Vec<NameKey> = target
            .folders
            .iter()
            .chain(std::iter::once(&target.name))
            .map(|c| names::key(c))
            .collect();
        let Some(group) = want.last().and_then(|name| self.by_name.get(name)) else {
            return Ok(None);
        };
        let hits: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&i| {
                let keys = self.entries[i].keys();
                if target.from_root {
                    keys == want.as_slice()
                } else {
                    keys.ends_with(&want)
                }
            })
            .collect();
        Ok(self.oldest(&hits).map(|i| self.entries[i].path.clone()))
    }

    /// Самая старая по дате создания, при равной дате — первая по пути.
    fn oldest(&self, candidates: &[usize]) -> Option<usize> {
        candidates.iter().copied().min_by_key(|&i| {
            let e = &self.entries[i];
            (e.created_ms, e.rank)
        })
    }

    /// Подсказки автодополнения по части имени или пути, не больше
    /// [`switcher::MAX_RESULTS`], лучшие первыми (нечёткий поиск как в
    /// быстром переходе; при равной оценке — короче имя, затем по пути).
    /// Пустой запрос — заметки в порядке имён ядра. Вставляемый текст — как
    /// у [`Links::shortest_unique_path`].
    pub fn suggest(&self, _from: &VaultPath, query: &str) -> CoreResult<Vec<LinkSuggestion>> {
        let chosen = switcher::best_matches(
            query,
            &self.entries,
            |e| &e.haystack,
            |a, b| {
                let (a, b) = (&self.entries[a], &self.entries[b]);
                a.haystack
                    .name_len()
                    .cmp(&b.haystack.name_len())
                    .then(a.rank.cmp(&b.rank))
            },
        )
        .unwrap_or_else(|| {
            self.by_name_order
                .iter()
                .copied()
                .take(switcher::MAX_RESULTS)
                .collect()
        });
        Ok(chosen
            .into_iter()
            .map(|i| {
                let e = &self.entries[i];
                LinkSuggestion {
                    insert: self.shortest_of(i),
                    name: e.name().to_owned(),
                    path: e.path.clone(),
                }
            })
            .collect())
    }

    /// Кратчайший различающий путь заметки без `.md`: `Встреча`,
    /// `Альфа/Встреча` или `/Встреча` для заметки в корне.
    ///
    /// Вложенные пути — путь одной заметки целиком совпадает с концом пути
    /// другой, включая корень (`Здоровье/Звонок.md` и
    /// `Команда/Здоровье/Звонок.md`): обе получают полный путь от корня,
    /// верхняя — с `/` в начале: `/Здоровье/Звонок` и
    /// `Команда/Здоровье/Звонок` (С3.4). Если имя без `.md` само кончается на
    /// `.md` (`Итоги.md.md`), `.md` дописывается — иначе разбор ссылки его
    /// отбросит. Заметки нет в списке — `not_found`.
    pub fn shortest_unique_path(&self, note: &VaultPath) -> CoreResult<String> {
        self.entries
            .iter()
            .position(|e| &e.path == note)
            .map(|i| self.shortest_of(i))
            .ok_or_else(|| CoreError::new(ErrorCode::NotFound))
    }

    fn shortest_of(&self, i: usize) -> String {
        let me = &self.entries[i];
        let keys = me.keys();
        let others: Vec<&[NameKey]> = keys
            .last()
            .and_then(|name| self.by_name.get(name))
            .into_iter()
            .flatten()
            .filter(|&&j| j != i)
            .map(|&j| self.entries[j].keys())
            .collect();
        let n = keys.len();
        if others.is_empty() {
            link_text(&me.link_path[n - 1..])
        } else if others.iter().any(|o| o.ends_with(keys)) {
            // Мой путь — конец пути другой заметки: только путь от корня.
            format!("/{}", link_text(&me.link_path))
        } else if others.iter().any(|o| keys.ends_with(o)) {
            // Путь другой заметки — конец моего: полный путь (С3.4).
            link_text(&me.link_path)
        } else {
            let k = (2..=n)
                .find(|&k| !others.iter().any(|o| o.ends_with(&keys[n - k..])))
                .unwrap_or(n);
            link_text(&me.link_path[n - k..])
        }
    }
}

/// Текст пути в ссылке: компоненты через `/`; имя, которое само кончается на
/// `.md`, получает ещё одно `.md` (разбор отбрасывает последнее).
fn link_text(parts: &[String]) -> String {
    let mut text = parts.join("/");
    if parts
        .last()
        .is_some_and(|name| names::note_stem(name).is_some())
    {
        text.push_str(".md");
    }
    text
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    use serde_json::{Value, json};

    /// Общие образцы разбора — те же проверяет Vitest (`src/core/wikilinks`).
    const PARSE_FIXTURE: &str = include_str!("../../../tests/fixtures/wikilinks/parse.json");

    fn as_json(result: Result<LinkTarget, InvalidTarget>) -> Value {
        match result {
            Ok(t) => json!({
                "folders": t.folders,
                "name": t.name,
                "heading": t.heading,
                "alias": t.alias,
                "from_root": t.from_root,
            }),
            Err(reason) => json!({ "invalid": reason.as_str() }),
        }
    }

    #[test]
    fn parse_matches_shared_fixture() {
        let doc: Value = serde_json::from_str(PARSE_FIXTURE).unwrap();
        let cases = doc["cases"].as_array().unwrap();
        assert!(cases.len() >= 40);
        for case in cases {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                as_json(parse_target(input)),
                case["expected"],
                "{input:?}: {}",
                case["note"]
            );
        }
    }

    mod props {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn parse_never_panics(input in "\\PC{0,80}|[\\[\\]|#/\\\\. a-zА-я]{0,40}") {
                let _ = parse_target(&input);
            }

            #[test]
            fn parsed_components_are_never_empty_or_dots(input in "[|#/\\\\. aбв]{0,24}") {
                if let Ok(t) = parse_target(&input) {
                    prop_assert!(!t.name.is_empty());
                    for c in t.folders.iter().chain(std::iter::once(&t.name)) {
                        prop_assert!(!c.is_empty() && c != "." && c != "..");
                        prop_assert!(!c.contains('/') && !c.contains('\\'));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    fn note(path: &str, created_ms: i64) -> NoteRef {
        NoteRef {
            path: p(path),
            created_ms,
        }
    }

    fn links(notes: &[(&str, i64)]) -> Links {
        Links::new(notes.iter().map(|&(path, at)| note(path, at)).collect())
    }

    fn resolve(links: &Links, target: &str) -> Option<String> {
        links
            .resolve(&p("Откуда.md"), target)
            .unwrap()
            .map(|path| path.components().join("/"))
    }

    fn shortest(links: &Links, path: &str) -> String {
        links.shortest_unique_path(&p(path)).unwrap()
    }

    #[test]
    fn resolves_unique_name_by_name_key() {
        let l = links(&[("Проекты/Альфа/Встреча.md", 1), ("План.md", 2)]);
        for target in [
            "Встреча",
            "встреча",
            "ВСТРЕЧА",
            "Встреча.md",
            "Встреча#Итоги",
            "Встреча|встречи",
        ] {
            assert_eq!(
                resolve(&l, target).as_deref(),
                Some("Проекты/Альфа/Встреча.md"),
                "{target}"
            );
        }
        // Разложенная «й» в ссылке и составная в имени файла.
        let l = links(&[("Мой план.md", 1)]);
        assert_eq!(
            resolve(&l, "Мои\u{306} план").as_deref(),
            Some("Мой план.md")
        );
        // Полное приведение регистра.
        let l = links(&[("Straße.md", 1)]);
        assert_eq!(resolve(&l, "STRASSE").as_deref(), Some("Straße.md"));
    }

    #[test]
    fn yo_and_e_are_different_names() {
        let l = links(&[("Учёба/Ёлка.md", 1)]);
        assert_eq!(resolve(&l, "Елка"), None);
        assert_eq!(resolve(&l, "ёлка").as_deref(), Some("Учёба/Ёлка.md"));
    }

    #[test]
    fn resolves_link_with_path_to_exactly_that_note() {
        let l = links(&[
            ("Проекты/Альфа/Встреча.md", 1),
            ("Проекты/Бета/Встреча.md", 2),
        ]);
        assert_eq!(
            resolve(&l, "Альфа/Встреча").as_deref(),
            Some("Проекты/Альфа/Встреча.md")
        );
        assert_eq!(
            resolve(&l, "Бета/Встреча").as_deref(),
            Some("Проекты/Бета/Встреча.md")
        );
        assert_eq!(
            resolve(&l, "проекты/бета/встреча").as_deref(),
            Some("Проекты/Бета/Встреча.md")
        );
        assert_eq!(resolve(&l, "Гамма/Встреча"), None);
        assert_eq!(resolve(&l, "Альфа/Проекты/Встреча"), None);
    }

    #[test]
    fn resolves_link_from_root() {
        let l = links(&[("Встреча.md", 5), ("Проекты/Альфа/Встреча.md", 1)]);
        assert_eq!(resolve(&l, "/Встреча").as_deref(), Some("Встреча.md"));
        assert_eq!(
            resolve(&l, "/Проекты/Альфа/Встреча").as_deref(),
            Some("Проекты/Альфа/Встреча.md")
        );
        assert_eq!(resolve(&l, "/Альфа/Встреча"), None);
        let l = links(&[("Проекты/Альфа/Встреча.md", 1)]);
        assert_eq!(resolve(&l, "/Встреча"), None);
    }

    #[test]
    fn bare_link_to_duplicates_goes_to_the_oldest() {
        let l = links(&[
            ("Проекты/Бета/Встреча.md", 100),
            ("Проекты/Альфа/Встреча.md", 200),
        ]);
        assert_eq!(
            resolve(&l, "Встреча").as_deref(),
            Some("Проекты/Бета/Встреча.md")
        );
        let l = links(&[("Работа/План.md", 50), ("План.md", 70)]);
        assert_eq!(resolve(&l, "План").as_deref(), Some("Работа/План.md"));
    }

    #[test]
    fn equal_dates_go_to_first_path_in_natural_order() {
        let l = links(&[
            ("Б/Встреча.md", 100),
            ("А/Встреча.md", 100),
            ("В/Встреча.md", 100),
        ]);
        assert_eq!(resolve(&l, "Встреча").as_deref(), Some("А/Встреча.md"));
        let l = links(&[("Папка 10/Итоги.md", 7), ("Папка 2/Итоги.md", 7)]);
        assert_eq!(resolve(&l, "Итоги").as_deref(), Some("Папка 2/Итоги.md"));
        // Сравнение по компонентам пути: «План.md» раньше «Работа/…»,
        // «Альфа/…» раньше «Яблоко.md».
        let l = links(&[("Работа/План.md", 3), ("План.md", 3)]);
        assert_eq!(resolve(&l, "План").as_deref(), Some("План.md"));
        let l = links(&[("Яблоко.md", 3), ("Альфа/Яблоко.md", 3)]);
        assert_eq!(resolve(&l, "Яблоко").as_deref(), Some("Альфа/Яблоко.md"));
    }

    #[test]
    fn path_link_without_slash_does_not_jump_to_newer_note() {
        // С3.4 (решение владельца 25.10.2026): `[[Альфа/Встреча]]` — по концу
        // пути; появление `Альфа/Встреча.md` в корне её не перенаправляет.
        let mut notes = vec![note("Проекты/Альфа/Встреча.md", 100)];
        let before = Links::new(notes.clone());
        assert_eq!(resolve(&before, "Альфа/Встреча").as_deref(), Some("Проекты/Альфа/Встреча.md"));
        notes.push(note("Альфа/Встреча.md", 200));
        let after = Links::new(notes);
        assert_eq!(resolve(&after, "Альфа/Встреча").as_deref(), Some("Проекты/Альфа/Встреча.md"));
        // Путь от корня — ровно этот путь.
        assert_eq!(resolve(&after, "/Альфа/Встреча").as_deref(), Some("Альфа/Встреча.md"));
        // Из нескольких подходящих по концу пути — самая старая.
        let l = links(&[("Команда/Здоровье/Звонок.md", 1), ("Здоровье/Звонок.md", 9)]);
        assert_eq!(resolve(&l, "Здоровье/Звонок").as_deref(), Some("Команда/Здоровье/Звонок.md"));
        let l = links(&[("X/Альфа/Встреча.md", 200), ("Y/Альфа/Встреча.md", 100)]);
        assert_eq!(resolve(&l, "Альфа/Встреча").as_deref(), Some("Y/Альфа/Встреча.md"));
    }

    #[test]
    fn bad_or_missing_targets_are_unresolved() {
        let l = links(&[("Встреча.md", 1), ("CON/Встреча.md", 2)]);
        for target in [
            "Нет такой",
            "",
            "../Встреча",
            "Альфа//Встреча",
            "Встреча: итоги",
            "CON",
            "Встреча.",
            "#Итоги",
        ] {
            assert_eq!(resolve(&l, target), None, "{target:?}");
        }
        assert_eq!(resolve(&Links::default(), "Встреча"), None);
    }

    #[test]
    fn note_with_uppercase_extension_resolves() {
        let l = links(&[("Архив/Старое.MD", 1)]);
        assert_eq!(resolve(&l, "старое").as_deref(), Some("Архив/Старое.MD"));
    }

    #[test]
    fn replace_notes_takes_effect() {
        let mut l = links(&[("Встреча.md", 1)]);
        l.replace_notes(vec![note("Итоги.md", 1)]);
        assert_eq!(resolve(&l, "Встреча"), None);
        assert_eq!(resolve(&l, "Итоги").as_deref(), Some("Итоги.md"));
    }

    #[test]
    fn shortest_path_is_name_for_unique_note() {
        let l = links(&[
            ("Проекты/Альфа/Уникальная.md", 1),
            ("Проекты/Альфа/Встреча.MD", 1),
        ]);
        assert_eq!(shortest(&l, "Проекты/Альфа/Уникальная.md"), "Уникальная");
        assert_eq!(shortest(&l, "Проекты/Альфа/Встреча.MD"), "Встреча");
    }

    #[test]
    fn shortest_path_adds_folders_from_the_end() {
        let l = links(&[
            ("Проекты/Альфа/Встреча.md", 1),
            ("Проекты/Бета/Встреча.md", 2),
        ]);
        assert_eq!(shortest(&l, "Проекты/Альфа/Встреча.md"), "Альфа/Встреча");
        assert_eq!(shortest(&l, "Проекты/Бета/Встреча.md"), "Бета/Встреча");
        let l = links(&[("Архив/2024/Отчёт.md", 1), ("Черновики/2024/Отчёт.md", 2)]);
        assert_eq!(shortest(&l, "Архив/2024/Отчёт.md"), "Архив/2024/Отчёт");
        assert_eq!(
            shortest(&l, "Черновики/2024/Отчёт.md"),
            "Черновики/2024/Отчёт"
        );
        // Регистр имени различается, ключ — один; путь — в написании с диска.
        let l = links(&[("Учёба/Идеи.md", 1), ("Личное/идеи.md", 2)]);
        assert_eq!(shortest(&l, "Учёба/Идеи.md"), "Учёба/Идеи");
        assert_eq!(shortest(&l, "Личное/идеи.md"), "Личное/идеи");
    }

    #[test]
    fn shortest_path_for_root_and_nested_notes_is_full_path() {
        // С3.4: по концу пути различить нельзя — обе получают полный путь,
        // верхняя — с косой чертой в начале.
        let l = links(&[("Встреча.md", 1), ("Проекты/Альфа/Встреча.md", 2)]);
        assert_eq!(shortest(&l, "Встреча.md"), "/Встреча");
        assert_eq!(
            shortest(&l, "Проекты/Альфа/Встреча.md"),
            "Проекты/Альфа/Встреча"
        );
        let l = links(&[("Здоровье/Звонок.md", 1), ("Команда/Здоровье/Звонок.md", 2)]);
        assert_eq!(shortest(&l, "Здоровье/Звонок.md"), "/Здоровье/Звонок");
        assert_eq!(
            shortest(&l, "Команда/Здоровье/Звонок.md"),
            "Команда/Здоровье/Звонок"
        );
        let l = links(&[("План.md", 1), ("Работа/План.md", 2)]);
        assert_eq!(shortest(&l, "План.md"), "/План");
        assert_eq!(shortest(&l, "Работа/План.md"), "Работа/План");
    }

    #[test]
    fn shortest_path_mixes_rules_in_one_group() {
        let l = links(&[
            ("Встреча.md", 1),
            ("Альфа/Встреча.md", 2),
            ("Проекты/Бета/Встреча.md", 3),
            ("Архив/Бета/Встреча.md", 4),
        ]);
        assert_eq!(shortest(&l, "Встреча.md"), "/Встреча");
        assert_eq!(shortest(&l, "Альфа/Встреча.md"), "Альфа/Встреча");
        assert_eq!(
            shortest(&l, "Проекты/Бета/Встреча.md"),
            "Проекты/Бета/Встреча"
        );
        assert_eq!(shortest(&l, "Архив/Бета/Встреча.md"), "Архив/Бета/Встреча");
        // Цепочка вложенных: средняя — и конец пути верхней, и сама — конец нижней.
        let l = links(&[
            ("Звонок.md", 1),
            ("Здоровье/Звонок.md", 2),
            ("Команда/Здоровье/Звонок.md", 3),
        ]);
        assert_eq!(shortest(&l, "Звонок.md"), "/Звонок");
        assert_eq!(shortest(&l, "Здоровье/Звонок.md"), "/Здоровье/Звонок");
        assert_eq!(
            shortest(&l, "Команда/Здоровье/Звонок.md"),
            "Команда/Здоровье/Звонок"
        );
    }

    #[test]
    fn shortest_path_resolves_back_to_the_same_note() {
        let notes = [
            ("Встреча.md", 9),
            ("Альфа/Встреча.md", 2),
            ("Проекты/Бета/Встреча.md", 3),
            ("Архив/Бета/Встреча.md", 1),
            ("Здоровье/Звонок.md", 5),
            ("Команда/Здоровье/Звонок.md", 1),
        ];
        let l = links(&notes);
        for (path, _) in notes {
            assert_eq!(
                resolve(&l, &shortest(&l, path)).as_deref(),
                Some(path),
                "{path}"
            );
        }
    }

    #[test]
    fn shortest_path_of_unknown_note_is_not_found() {
        let l = links(&[("Встреча.md", 1)]);
        let err = l.shortest_unique_path(&p("Нет.md")).unwrap_err();
        assert_eq!(err.code, crate::commands::ErrorCode::NotFound);
    }

    #[test]
    fn suggest_inserts_name_or_shortest_path() {
        let l = links(&[
            ("Проекты/Альфа/Встреча.md", 1),
            ("Проекты/Бета/Встреча.md", 2),
            ("Итоги встречи.md", 3),
            ("План.md", 4),
        ]);
        let items = l.suggest(&p("План.md"), "встр").unwrap();
        let inserts: Vec<&str> = items.iter().map(|i| i.insert.as_str()).collect();
        assert_eq!(inserts, ["Альфа/Встреча", "Бета/Встреча", "Итоги встречи"]);
        assert_eq!(items[0].name, "Встреча");
        assert_eq!(items[0].path, p("Проекты/Альфа/Встреча.md"));
    }

    #[test]
    fn insert_for_stem_ending_in_md_keeps_working() {
        // Основа `Итоги.md` сама кончается на `.md`: без дописанного `.md`
        // разбор отбросил бы его и ссылка вела бы на «Итоги».
        let l = links(&[("Итоги.md.md", 1), ("Итоги.md", 2)]);
        let items = l.suggest(&p("Откуда.md"), "итоги.md").unwrap();
        let item = items.iter().find(|i| i.path == p("Итоги.md.md")).unwrap();
        assert_eq!(item.insert, "Итоги.md.md");
        assert_eq!(item.name, "Итоги.md");
        assert_eq!(resolve(&l, &item.insert).as_deref(), Some("Итоги.md.md"));
        let l = links(&[("А/Итоги.MD.md", 1), ("Б/Итоги.md.md", 2)]);
        assert_eq!(shortest(&l, "А/Итоги.MD.md"), "А/Итоги.MD.md");
        assert_eq!(
            resolve(&l, &shortest(&l, "Б/Итоги.md.md")).as_deref(),
            Some("Б/Итоги.md.md")
        );
    }

    #[test]
    fn suggest_by_path_when_query_has_slash() {
        let l = links(&[
            ("Проекты/Альфа/Встреча.md", 1),
            ("Проекты/Бета/Встреча.md", 2),
        ]);
        let items = l.suggest(&p("Откуда.md"), "бета/вст").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert, "Бета/Встреча");
    }

    #[test]
    fn suggest_ranks_exact_name_first_and_caps_results() {
        let mut notes: Vec<(String, i64)> = (0..80)
            .map(|i| (format!("Встречи/Встреча с отделом {i}.md"), i))
            .collect();
        notes.push(("План встречи.md".into(), 1));
        notes.push(("Встреча.md".into(), 1));
        let l = Links::new(notes.iter().map(|(path, at)| note(path, *at)).collect());
        let items = l.suggest(&p("Откуда.md"), "Встреча").unwrap();
        assert_eq!(items.len(), crate::switcher::MAX_RESULTS);
        assert_eq!(items[0].insert, "Встреча");
        // Пустой запрос — заметки в естественном порядке имён, тоже не больше 50.
        let all = l.suggest(&p("Откуда.md"), "").unwrap();
        assert_eq!(all.len(), crate::switcher::MAX_RESULTS);
        assert_eq!(all[0].name, "Встреча");
        assert_eq!(all[1].name, "Встреча с отделом 0");
        assert_eq!(all[3].name, "Встреча с отделом 2");
        assert_eq!(all[11].name, "Встреча с отделом 10");
    }

    mod props {
        use super::*;
        use proptest::collection::btree_set;
        use proptest::prelude::*;

        /// Пути заметок из малого алфавита — чтобы чаще совпадали имена и
        /// концы путей. Ключи путей различны (как на диске Windows).
        fn vault() -> impl Strategy<Value = Vec<String>> {
            let component = prop::sample::select(vec!["А", "а", "Б", "В", "Встреча", "ß", "ss"]);
            let path = prop::collection::vec(component, 1..5).prop_map(|parts| parts.join("/"));
            btree_set(path, 1..14).prop_map(|paths| {
                let mut seen = std::collections::HashSet::new();
                paths
                    .into_iter()
                    .filter(|path| {
                        let key: Vec<_> = path.split('/').map(names::key).collect();
                        seen.insert(key)
                    })
                    .map(|path| format!("{path}.md"))
                    .collect()
            })
        }

        proptest! {
            #[test]
            fn shortest_path_resolves_back(paths in vault(), dates in prop::collection::vec(0i64..3, 14)) {
                // Папка и заметка с одним ключом пути на одном уровне — не мешают:
                // папки в списке заметок не участвуют.
                let notes: Vec<NoteRef> = paths
                    .iter()
                    .zip(dates.iter().cycle())
                    .map(|(path, &at)| note(path, at))
                    .collect();
                let l = Links::new(notes.clone());
                for n in &notes {
                    let link = l.shortest_unique_path(&n.path).unwrap();
                    prop_assert_eq!(l.resolve(&p("Откуда.md"), &link).unwrap(), Some(n.path.clone()), "{}", link);
                }
            }

            #[test]
            fn resolve_never_panics(target in "\\PC{0,60}") {
                let l = links(&[("Встреча.md", 1), ("А/Встреча.md", 1)]);
                let _ = l.resolve(&p("Откуда.md"), &target);
                let _ = l.suggest(&p("Откуда.md"), &target);
            }
        }
    }
}

/// Сверка с манифестом генератора тестового хранилища (`tools/gen-vault`).
#[cfg(test)]
mod gen_vault_tests {
    use super::*;
    use gen_vault::{Config, Resolution, generate};
    use std::collections::HashSet;
    use std::path::Path;

    fn note_paths(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                note_paths(root, &path, out);
            } else if names::is_note(&name) {
                let rel = path.strip_prefix(root).unwrap();
                let parts: Vec<_> = rel.iter().map(|c| c.to_str().unwrap()).collect();
                out.push(parts.join("/"));
            }
        }
    }

    fn vp(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn matches_generator_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = generate(dir.path(), &Config::new(800, 7)).unwrap();
        let mut paths = Vec::new();
        note_paths(dir.path(), dir.path(), &mut paths);
        assert_eq!(paths.len(), manifest.notes);

        // Дата создания задаётся тестом: первая заметка группы дубликатов —
        // самая старая (на диске даты могут совпасть).
        let first: HashSet<&str> = manifest
            .duplicates
            .iter()
            .map(|g| g.created_first.as_str())
            .collect();
        let later: HashSet<&str> = manifest
            .duplicates
            .iter()
            .flat_map(|g| g.notes.iter().map(|n| n.path.as_str()))
            .filter(|p| !first.contains(p))
            .collect();
        let links = Links::new(
            paths
                .iter()
                .map(|p| NoteRef {
                    path: vp(p),
                    created_ms: if later.contains(p.as_str()) { 2 } else { 1 },
                })
                .collect(),
        );

        assert!(!manifest.duplicates.is_empty());
        for group in &manifest.duplicates {
            for n in &group.notes {
                assert_eq!(
                    links.shortest_unique_path(&vp(&n.path)).unwrap(),
                    n.shortest_link,
                    "{}",
                    n.path
                );
            }
        }

        assert!(manifest.expected_links.len() > 100);
        for link in &manifest.expected_links {
            let inner = link.link.trim_start_matches("[[").trim_end_matches("]]");
            let got = links.resolve(&vp(&link.source), inner).unwrap();
            let expected = match link.resolution {
                Resolution::Unresolved => None,
                Resolution::Unique | Resolution::Oldest => link.target.as_deref().map(vp),
            };
            assert_eq!(got, expected, "{} в {}", link.link, link.source);
        }
    }
}
