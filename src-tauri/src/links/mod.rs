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
