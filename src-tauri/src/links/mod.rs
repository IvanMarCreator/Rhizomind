//! Вики-ссылки: разрешение, кратчайший различающий путь, подсказки —
//! интерфейс модуля, реализация в задаче 1.1.
//!
//! Правила — спецификация 0.1, С3.4: разрешение по имени файла без `.md` по
//! ключу имени (модуль `names`); ссылка с путём ведёт ровно на этот файл;
//! ссылка без пути при дубликатах — на самую старую по дате создания, а при
//! равной дате — на первую по пути в естественном алфавитном порядке;
//! `[[/Встреча]]` — заметка в корне. Модуль чистый: работает со списком
//! заметок в памяти, к диску не обращается.

use crate::commands::{CoreError, CoreResult, LinkSuggestion, VaultPath};
use crate::names;
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
        Some((target, alias)) => (target.strip_suffix('\\').unwrap_or(target), non_empty(alias)),
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
    let name = names::note_stem(&file).map_or(file.clone(), str::to_owned);
    Ok(LinkTarget {
        folders: components,
        name,
        heading,
        alias,
        from_root,
    })
}

/// Ссылки одного хранилища. Список заметок заменяется целиком при изменении
/// дерева (`replace_notes`); запросы не меняют состояние.
#[derive(Debug, Default)]
pub struct Links {
    #[allow(dead_code)]
    notes: Vec<NoteRef>,
}

impl Links {
    /// Ссылки по списку заметок хранилища (`Vault::notes`).
    pub fn new(notes: Vec<NoteRef>) -> Self {
        Self { notes }
    }

    /// Заменяет список заметок (после изменения дерева).
    pub fn replace_notes(&mut self, notes: Vec<NoteRef>) {
        self.notes = notes;
    }

    /// Куда ведёт цель ссылки `target` (текст между `[[` и `|`/`#`) из
    /// заметки `from`. `Ok(None)` — неразрешённая ссылка, в том числе для
    /// цели, которая не может быть путём (`..`, зарезервированное имя и
    /// т.п.): плохая цель — не ошибка. `Err` — только отказ самого модуля.
    pub fn resolve(&self, _from: &VaultPath, _target: &str) -> CoreResult<Option<VaultPath>> {
        Err(CoreError::not_implemented())
    }

    /// Подсказки автодополнения по части имени: вставляемый текст — имя для
    /// уникальных заметок, кратчайший различающий путь для повторяющихся
    /// (с правилом вложенных путей из [`Links::shortest_unique_path`]).
    pub fn suggest(&self, _from: &VaultPath, _query: &str) -> CoreResult<Vec<LinkSuggestion>> {
        Err(CoreError::not_implemented())
    }

    /// Кратчайший различающий путь заметки без `.md`: `Встреча`,
    /// `Альфа/Встреча` или `/Встреча` для заметки в корне.
    ///
    /// Вложенные пути — путь одной заметки целиком совпадает с концом пути
    /// другой, включая корень (`Здоровье/Звонок.md` и
    /// `Команда/Здоровье/Звонок.md`): обе получают полный путь от корня,
    /// верхняя — с `/` в начале: `/Здоровье/Звонок` и
    /// `Команда/Здоровье/Звонок` (С3.4).
    pub fn shortest_unique_path(&self, _note: &VaultPath) -> CoreResult<String> {
        Err(CoreError::not_implemented())
    }
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
        for target in ["Встреча", "встреча", "ВСТРЕЧА", "Встреча.md", "Встреча#Итоги", "Встреча|встречи"] {
            assert_eq!(resolve(&l, target).as_deref(), Some("Проекты/Альфа/Встреча.md"), "{target}");
        }
        // Разложенная «й» в ссылке и составная в имени файла.
        let l = links(&[("Мой план.md", 1)]);
        assert_eq!(resolve(&l, "Мои\u{306} план").as_deref(), Some("Мой план.md"));
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
        let l = links(&[("Проекты/Альфа/Встреча.md", 1), ("Проекты/Бета/Встреча.md", 2)]);
        assert_eq!(resolve(&l, "Альфа/Встреча").as_deref(), Some("Проекты/Альфа/Встреча.md"));
        assert_eq!(resolve(&l, "Бета/Встреча").as_deref(), Some("Проекты/Бета/Встреча.md"));
        assert_eq!(resolve(&l, "проекты/бета/встреча").as_deref(), Some("Проекты/Бета/Встреча.md"));
        assert_eq!(resolve(&l, "Гамма/Встреча"), None);
        assert_eq!(resolve(&l, "Альфа/Проекты/Встреча"), None);
    }

    #[test]
    fn resolves_link_from_root() {
        let l = links(&[("Встреча.md", 5), ("Проекты/Альфа/Встреча.md", 1)]);
        assert_eq!(resolve(&l, "/Встреча").as_deref(), Some("Встреча.md"));
        assert_eq!(resolve(&l, "/Проекты/Альфа/Встреча").as_deref(), Some("Проекты/Альфа/Встреча.md"));
        assert_eq!(resolve(&l, "/Альфа/Встреча"), None);
        let l = links(&[("Проекты/Альфа/Встреча.md", 1)]);
        assert_eq!(resolve(&l, "/Встреча"), None);
    }

    #[test]
    fn bare_link_to_duplicates_goes_to_the_oldest() {
        let l = links(&[("Проекты/Бета/Встреча.md", 100), ("Проекты/Альфа/Встреча.md", 200)]);
        assert_eq!(resolve(&l, "Встреча").as_deref(), Some("Проекты/Бета/Встреча.md"));
        let l = links(&[("Работа/План.md", 50), ("План.md", 70)]);
        assert_eq!(resolve(&l, "План").as_deref(), Some("Работа/План.md"));
    }

    #[test]
    fn equal_dates_go_to_first_path_in_natural_order() {
        let l = links(&[("Б/Встреча.md", 100), ("А/Встреча.md", 100), ("В/Встреча.md", 100)]);
        assert_eq!(resolve(&l, "Встреча").as_deref(), Some("А/Встреча.md"));
        let l = links(&[("Папка 10/Итоги.md", 7), ("Папка 2/Итоги.md", 7)]);
        assert_eq!(resolve(&l, "Итоги").as_deref(), Some("Папка 2/Итоги.md"));
        // Короче путь — раньше: корень перед папкой при равной дате.
        let l = links(&[("Работа/План.md", 3), ("План.md", 3)]);
        assert_eq!(resolve(&l, "План").as_deref(), Some("План.md"));
    }

    #[test]
    fn path_link_prefers_exact_path_from_root_over_longer_suffix() {
        let l = links(&[("Команда/Здоровье/Звонок.md", 1), ("Здоровье/Звонок.md", 9)]);
        assert_eq!(resolve(&l, "Здоровье/Звонок").as_deref(), Some("Здоровье/Звонок.md"));
        // Без точного совпадения от корня — самая старая из подходящих по концу пути.
        let l = links(&[("X/Альфа/Встреча.md", 200), ("Y/Альфа/Встреча.md", 100)]);
        assert_eq!(resolve(&l, "Альфа/Встреча").as_deref(), Some("Y/Альфа/Встреча.md"));
    }

    #[test]
    fn bad_or_missing_targets_are_unresolved() {
        let l = links(&[("Встреча.md", 1), ("CON/Встреча.md", 2)]);
        for target in ["Нет такой", "", "../Встреча", "Альфа//Встреча", "Встреча: итоги", "CON", "Встреча.", "#Итоги"] {
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
        let l = links(&[("Проекты/Альфа/Уникальная.md", 1), ("Проекты/Альфа/Встреча.MD", 1)]);
        assert_eq!(shortest(&l, "Проекты/Альфа/Уникальная.md"), "Уникальная");
        assert_eq!(shortest(&l, "Проекты/Альфа/Встреча.MD"), "Встреча");
    }

    #[test]
    fn shortest_path_adds_folders_from_the_end() {
        let l = links(&[("Проекты/Альфа/Встреча.md", 1), ("Проекты/Бета/Встреча.md", 2)]);
        assert_eq!(shortest(&l, "Проекты/Альфа/Встреча.md"), "Альфа/Встреча");
        assert_eq!(shortest(&l, "Проекты/Бета/Встреча.md"), "Бета/Встреча");
        let l = links(&[("Архив/2024/Отчёт.md", 1), ("Черновики/2024/Отчёт.md", 2)]);
        assert_eq!(shortest(&l, "Архив/2024/Отчёт.md"), "Архив/2024/Отчёт");
        assert_eq!(shortest(&l, "Черновики/2024/Отчёт.md"), "Черновики/2024/Отчёт");
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
        assert_eq!(shortest(&l, "Проекты/Альфа/Встреча.md"), "Проекты/Альфа/Встреча");
        let l = links(&[("Здоровье/Звонок.md", 1), ("Команда/Здоровье/Звонок.md", 2)]);
        assert_eq!(shortest(&l, "Здоровье/Звонок.md"), "/Здоровье/Звонок");
        assert_eq!(shortest(&l, "Команда/Здоровье/Звонок.md"), "Команда/Здоровье/Звонок");
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
        assert_eq!(shortest(&l, "Проекты/Бета/Встреча.md"), "Проекты/Бета/Встреча");
        assert_eq!(shortest(&l, "Архив/Бета/Встреча.md"), "Архив/Бета/Встреча");
        // Цепочка вложенных: средняя — и конец пути верхней, и сама — конец нижней.
        let l = links(&[("Звонок.md", 1), ("Здоровье/Звонок.md", 2), ("Команда/Здоровье/Звонок.md", 3)]);
        assert_eq!(shortest(&l, "Звонок.md"), "/Звонок");
        assert_eq!(shortest(&l, "Здоровье/Звонок.md"), "/Здоровье/Звонок");
        assert_eq!(shortest(&l, "Команда/Здоровье/Звонок.md"), "Команда/Здоровье/Звонок");
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
            assert_eq!(resolve(&l, &shortest(&l, path)).as_deref(), Some(path), "{path}");
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
    fn suggest_by_path_when_query_has_slash() {
        let l = links(&[("Проекты/Альфа/Встреча.md", 1), ("Проекты/Бета/Встреча.md", 2)]);
        let items = l.suggest(&p("Откуда.md"), "бета/вст").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert, "Бета/Встреча");
    }

    #[test]
    fn suggest_ranks_exact_name_first_and_caps_results() {
        let mut notes: Vec<(String, i64)> = (0..80).map(|i| (format!("Встречи/Встреча с отделом {i}.md"), i)).collect();
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
