//! Быстрый переход (Ctrl+O): нечёткий поиск по именам и путям заметок —
//! интерфейс модуля, реализация в задаче 1.1 (`nucleo-matcher`).
//!
//! Имена сравниваются по тому же ключу имени, что и ссылки (модуль `names`).
//! В интерфейс уходит не больше [`MAX_RESULTS`] лучших совпадений.

use std::cmp::{Ordering, Reverse};

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};

use crate::commands::{CoreResult, SwitcherItem, VaultPath};
use crate::names::{self, KeyedPath};

/// Сколько совпадений возвращается интерфейсу.
pub const MAX_RESULTS: usize = 50;

/// Строки одной заметки для нечёткого поиска: ключ имени без `.md` и ключ
/// пути через `/` (`проекты/альфа/встреча`). Общая часть быстрого перехода и
/// подсказок вики-ссылок (модуль `links`).
#[derive(Debug, Clone)]
pub(crate) struct Haystack {
    name: Utf32String,
    path: Utf32String,
}

impl Haystack {
    /// Строки поиска по ключам пути в ссылке (последний компонент — имя
    /// заметки без `.md`, см. [`link_path`]).
    pub(crate) fn new(path: &KeyedPath) -> Self {
        let keys: Vec<&str> = path.keys().iter().map(names::NameKey::as_str).collect();
        let name = keys.last().copied().unwrap_or_default();
        Self {
            name: Utf32String::from(name),
            path: Utf32String::from(keys.join("/")),
        }
    }

    /// Длина имени в символах — короче имя при равной оценке, раньше.
    pub(crate) fn name_len(&self) -> usize {
        self.name.len()
    }
}

/// Лучшие совпадения запроса, не больше [`MAX_RESULTS`], лучшие первыми.
/// `None` — в запросе нет слов (пустой или из пробелов): порядок по
/// умолчанию выбирает вызывающий.
///
/// Запрос приводится ключом имени и делится на слова по пробелам; каждое
/// слово ищется нечётко (`nucleo-matcher`), без особого синтаксиса (`!`,
/// `^`, `$` — обычные символы). Оценка заметки — лучшая из «имя ×2» и
/// «путь»: совпадение в имени важнее совпадения в папках. При равной оценке
/// порядок задаёт `tie`.
pub(crate) fn best_matches<T>(
    query: &str,
    items: &[T],
    haystack: impl Fn(&T) -> &Haystack,
    tie: impl Fn(usize, usize) -> Ordering,
) -> Option<Vec<usize>> {
    let query = names::key(query);
    let pattern = Pattern::new(
        query.as_str(),
        CaseMatching::Respect,
        Normalization::Never,
        AtomKind::Fuzzy,
    );
    if pattern.atoms.is_empty() {
        return None;
    }
    let mut config = Config::DEFAULT;
    config.normalize = false;
    config.ignore_case = false;
    let mut name_matcher = Matcher::new(config.clone());
    let mut path_matcher = Matcher::new(config.match_paths());
    let mut found: Vec<(u32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let item = haystack(item);
            let by_name = pattern
                .score(item.name.slice(..), &mut name_matcher)
                .map(|s| s.saturating_mul(2));
            let by_path = pattern.score(item.path.slice(..), &mut path_matcher);
            by_name.max(by_path).map(|score| (score, i))
        })
        .collect();
    found.sort_unstable_by(|a, b| Reverse(a.0).cmp(&Reverse(b.0)).then_with(|| tie(a.1, b.1)));
    Some(
        found
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, i)| i)
            .collect(),
    )
}

/// Путь заметки, как он пишется в ссылке: папки и имя без `.md`. `None` —
/// не заметка (`.md`): такие файлы в поиск и ссылки не попадают.
pub(crate) fn link_path(path: &VaultPath) -> Option<Vec<String>> {
    let (file, folders) = path.components().split_last()?;
    let stem = names::note_stem(file)?;
    let mut parts = folders.to_vec();
    parts.push(stem.to_owned());
    Some(parts)
}

/// Порядковые номера путей в порядке путей ядра (для равных оценок).
pub(crate) fn natural_ranks(paths: &[KeyedPath]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..paths.len()).collect();
    order.sort_by(|&a, &b| paths[a].cmp(&paths[b]));
    let mut ranks = vec![0; paths.len()];
    for (rank, i) in order.into_iter().enumerate() {
        ranks[i] = u32::try_from(rank).unwrap_or(u32::MAX);
    }
    ranks
}

#[derive(Debug)]
struct Item {
    path: VaultPath,
    name: String,
    haystack: Haystack,
    rank: u32,
}

/// Поиск по заметкам одного хранилища. Список заменяется целиком при
/// изменении дерева.
#[derive(Debug, Default)]
pub struct Switcher {
    items: Vec<Item>,
}

impl Switcher {
    /// Поиск по списку заметок хранилища.
    pub fn new(notes: Vec<VaultPath>) -> Self {
        let mut switcher = Self::default();
        switcher.replace_notes(notes);
        switcher
    }

    /// Заменяет список заметок (после изменения дерева). Файлы не `.md`
    /// пропускаются. Ключи и строки поиска считаются здесь, один раз, а не
    /// на каждый запрос.
    pub fn replace_notes(&mut self, notes: Vec<VaultPath>) {
        let notes: Vec<(VaultPath, Vec<String>)> = notes
            .into_iter()
            .filter_map(|path| {
                let link_path = link_path(&path)?;
                Some((path, link_path))
            })
            .collect();
        let keyed: Vec<KeyedPath> = notes.iter().map(|(_, lp)| KeyedPath::new(lp)).collect();
        let ranks = natural_ranks(&keyed);
        self.items = notes
            .into_iter()
            .zip(keyed.iter().zip(ranks))
            .map(|((path, mut link_path), (keyed, rank))| Item {
                haystack: Haystack::new(keyed),
                path,
                name: link_path.pop().unwrap_or_default(),
                rank,
            })
            .collect();
    }

    /// До [`MAX_RESULTS`] заметок, лучшие первыми; при равной оценке —
    /// короче имя, затем по пути в естественном порядке. Пустой запрос —
    /// первые заметки по порядку списка. Цель по скорости: 20 мс на 10 000
    /// заметок (план 1.1, тест `search_10k_names_within_20ms`).
    pub fn search(&self, query: &str) -> CoreResult<Vec<SwitcherItem>> {
        let chosen = self
            .best(query)
            .unwrap_or_else(|| (0..self.items.len().min(MAX_RESULTS)).collect());
        Ok(chosen
            .into_iter()
            .map(|i| SwitcherItem {
                name: self.items[i].name.clone(),
                path: self.items[i].path.clone(),
            })
            .collect())
    }

    fn best(&self, query: &str) -> Option<Vec<usize>> {
        best_matches(
            query,
            &self.items,
            |item| &item.haystack,
            |a, b| {
                let (a, b) = (&self.items[a], &self.items[b]);
                a.haystack
                    .name_len()
                    .cmp(&b.haystack.name_len())
                    .then(a.rank.cmp(&b.rank))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    fn switcher(paths: &[&str]) -> Switcher {
        Switcher::new(paths.iter().map(|path| p(path)).collect())
    }

    fn found(s: &Switcher, query: &str) -> Vec<String> {
        s.search(query)
            .unwrap()
            .into_iter()
            .map(|item| item.path.components().join("/"))
            .collect()
    }

    #[test]
    fn empty_query_lists_notes_in_list_order() {
        let s = switcher(&["Б.md", "А.md", "В/Г.md"]);
        assert_eq!(found(&s, ""), ["Б.md", "А.md", "В/Г.md"]);
        assert_eq!(found(&s, "   "), ["Б.md", "А.md", "В/Г.md"]);
        let many: Vec<String> = (0..120).map(|i| format!("Заметка {i}.md")).collect();
        let s = Switcher::new(many.iter().map(|path| p(path)).collect());
        assert_eq!(s.search("").unwrap().len(), MAX_RESULTS);
        assert_eq!(s.search("заметка").unwrap().len(), MAX_RESULTS);
    }

    #[test]
    fn matches_by_name_key() {
        let s = switcher(&[
            "Проекты/Альфа/Встреча.md",
            "Мой план.md",
            "Straße.md",
            "Итоги.md",
        ]);
        assert_eq!(found(&s, "встр"), ["Проекты/Альфа/Встреча.md"]);
        assert_eq!(found(&s, "ВСТРЕЧА"), ["Проекты/Альфа/Встреча.md"]);
        assert_eq!(found(&s, "Мои\u{306}"), ["Мой план.md"]);
        assert_eq!(found(&s, "STRASSE"), ["Straße.md"]);
        assert_eq!(found(&s, "втч"), ["Проекты/Альфа/Встреча.md"]);
        assert!(found(&s, "нет такого").is_empty());
    }

    #[test]
    fn item_shows_name_without_extension() {
        let s = switcher(&["Проекты/Встреча.md"]);
        let items = s.search("встреча").unwrap();
        assert_eq!(items[0].name, "Встреча");
        assert_eq!(items[0].path, p("Проекты/Встреча.md"));
    }

    #[test]
    fn yo_and_e_are_different_letters() {
        let s = switcher(&["Учёба/Ёлка.md"]);
        assert!(found(&s, "елка").is_empty());
        assert_eq!(found(&s, "ёлка"), ["Учёба/Ёлка.md"]);
    }

    #[test]
    fn searches_folders_too_and_name_match_ranks_first() {
        let s = switcher(&["Встреча/Отчёт.md", "Архив/Встреча.md"]);
        assert_eq!(
            found(&s, "встреча"),
            ["Архив/Встреча.md", "Встреча/Отчёт.md"]
        );
        let s = switcher(&["Проекты/Бета/Встреча.md", "Проекты/Альфа/Встреча.md"]);
        assert_eq!(found(&s, "альфа встр"), ["Проекты/Альфа/Встреча.md"]);
        assert_eq!(found(&s, "альфа/встр"), ["Проекты/Альфа/Встреча.md"]);
    }

    #[test]
    fn equal_scores_shorter_name_then_natural_path() {
        let s = switcher(&["Встреча с отделом.md", "Б/Встреча.md", "А/Встреча.md"]);
        assert_eq!(
            found(&s, "встреча"),
            ["А/Встреча.md", "Б/Встреча.md", "Встреча с отделом.md"]
        );
    }

    #[test]
    fn no_special_query_syntax() {
        let s = switcher(&["!Важно.md", "Итоги$.md", "^Начало.md"]);
        assert_eq!(found(&s, "!важ"), ["!Важно.md"]);
        assert_eq!(found(&s, "итоги$"), ["Итоги$.md"]);
        assert_eq!(found(&s, "^нач"), ["^Начало.md"]);
    }

    #[test]
    fn only_notes_are_searched() {
        let s = switcher(&["Схема.png", "Схема.md", "Папка", "схема.txt"]);
        assert_eq!(found(&s, "схема"), ["Схема.md"]);
        assert_eq!(found(&s, ""), ["Схема.md"]);
    }

    #[test]
    fn replace_notes_takes_effect() {
        let mut s = switcher(&["Встреча.md"]);
        s.replace_notes(vec![p("Итоги.md")]);
        assert!(found(&s, "встреча").is_empty());
        assert_eq!(found(&s, "итоги"), ["Итоги.md"]);
    }

    /// План 1.1: поиск по 10 000 именам — не дольше 20 мс. Только в выпускной
    /// сборке: `cargo test -p rhizomind --release --lib -- --ignored` (шаг CI).
    #[test]
    #[ignore = "тест производительности: запускается отдельным шагом CI в выпускной сборке"]
    fn search_10k_names_within_20ms() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        gen_vault::generate(dir.path(), &gen_vault::Config::new(10_000, 1)).unwrap();
        let mut notes = Vec::new();
        collect_notes(dir.path(), &mut Vec::new(), &mut notes);
        assert_eq!(notes.len(), 10_000);

        let started = Instant::now();
        let s = Switcher::new(notes);
        println!("replace_notes, 10 000 заметок: {:?}", started.elapsed());

        let limit = Duration::from_millis(20);
        for query in [
            "в",
            "встр",
            "заметка 1",
            "проекты альфа",
            "глубокая",
            "nonexistent xyz",
            "ёлка",
        ] {
            let mut times: Vec<Duration> = (0..7)
                .map(|_| {
                    let started = Instant::now();
                    let items = s.search(query).unwrap();
                    assert!(items.len() <= MAX_RESULTS);
                    started.elapsed()
                })
                .collect();
            times.sort();
            let (median, worst) = (times[times.len() / 2], times[times.len() - 1]);
            println!("{query:?}: медиана {median:?}, худшее {worst:?}");
            assert!(median <= limit, "{query:?}: {median:?} > {limit:?}");
        }
    }

    fn collect_notes(dir: &std::path::Path, prefix: &mut Vec<String>, out: &mut Vec<VaultPath>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name.starts_with('.') {
                continue;
            }
            prefix.push(name.clone());
            if entry.path().is_dir() {
                collect_notes(&entry.path(), prefix, out);
            } else if names::is_note(&name) {
                out.push(VaultPath::from_components(prefix.clone()));
            }
            prefix.pop();
        }
    }
}
