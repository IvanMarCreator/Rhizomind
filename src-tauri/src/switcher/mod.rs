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
    /// Строки поиска по ключам пути заметки (последний компонент — имя файла).
    pub(crate) fn new(path: &KeyedPath) -> Self {
        let keys: Vec<&str> = path.keys().iter().map(names::NameKey::as_str).collect();
        let (file, folders) = keys.split_last().map_or(("", &[][..]), |(f, d)| (*f, d));
        let name = names::note_stem(file).unwrap_or(file);
        let mut full = folders.join("/");
        if !full.is_empty() {
            full.push('/');
        }
        full.push_str(name);
        Self {
            name: Utf32String::from(name),
            path: Utf32String::from(full),
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

/// Порядковые номера путей в естественном порядке (для равных оценок).
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

    /// Заменяет список заметок (после изменения дерева). Ключи и строки
    /// поиска считаются здесь, один раз, а не на каждый запрос.
    pub fn replace_notes(&mut self, notes: Vec<VaultPath>) {
        let keyed: Vec<KeyedPath> = notes
            .iter()
            .map(|p| KeyedPath::new(p.components()))
            .collect();
        let ranks = natural_ranks(&keyed);
        self.items = notes
            .into_iter()
            .zip(keyed.iter().zip(ranks))
            .map(|(path, (keyed, rank))| {
                let file = path.file_name().unwrap_or_default();
                let name = names::note_stem(file).unwrap_or(file).to_owned();
                Item {
                    haystack: Haystack::new(keyed),
                    path,
                    name,
                    rank,
                }
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
