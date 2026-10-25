//! Команды группы «Ссылки и поиск».

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::Core;
use super::error::{CoreError, CoreResult};
use super::types::VaultPath;

/// Запрос `links_resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksResolveRequest {
    /// Заметка, в которой стоят ссылки.
    pub from: VaultPath,
    /// Цели ссылок — текст между `[[` и `|` или `#`: `Встреча`,
    /// `Альфа/Встреча`, `/Встреча`.
    pub targets: Vec<String>,
}

/// Куда ведёт одна ссылка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLink {
    /// Цель из запроса.
    pub target: String,
    /// Заметка; `null` — неразрешённая ссылка.
    pub path: Option<VaultPath>,
}

/// Ответ `links_resolve`: по элементу на каждую цель, в том же порядке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksResolveResponse {
    /// Результаты.
    pub results: Vec<ResolvedLink>,
}

/// Запрос `links_suggest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksSuggestRequest {
    /// Заметка, в которой набирается ссылка.
    pub from: VaultPath,
    /// Набранная часть имени после `[[`.
    pub query: String,
}

/// Подсказка автодополнения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSuggestion {
    /// Что вставить между `[[` и `]]`: имя или кратчайший различающий путь.
    pub insert: String,
    /// Имя заметки без `.md` — для показа в списке.
    pub name: String,
    /// Заметка.
    pub path: VaultPath,
}

/// Ответ `links_suggest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksSuggestResponse {
    /// Подсказки, лучшие первыми.
    pub items: Vec<LinkSuggestion>,
}

/// Запрос `switcher_search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitcherSearchRequest {
    /// Строка поиска.
    pub query: String,
}

/// Совпадение быстрого перехода.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitcherItem {
    /// Имя заметки без `.md`.
    pub name: String,
    /// Заметка.
    pub path: VaultPath,
}

/// Ответ `switcher_search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitcherSearchResponse {
    /// До 50 совпадений, лучшие первыми.
    pub items: Vec<SwitcherItem>,
}

fn read<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Разрешение вики-ссылок заметки.
#[tauri::command]
pub async fn links_resolve<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksResolveRequest,
) -> CoreResult<LinksResolveResponse> {
    let open = core.require_vault(window.label())?;
    let links = read(&open.links);
    let results = request
        .targets
        .into_iter()
        .map(|target| {
            let path = links.resolve(&request.from, &target)?;
            Ok(ResolvedLink { target, path })
        })
        .collect::<Result<_, CoreError>>()?;
    Ok(LinksResolveResponse { results })
}

/// Подсказки автодополнения вики-ссылки.
#[tauri::command]
pub async fn links_suggest<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksSuggestRequest,
) -> CoreResult<LinksSuggestResponse> {
    let open = core.require_vault(window.label())?;
    let items = read(&open.links).suggest(&request.from, &request.query)?;
    Ok(LinksSuggestResponse { items })
}

/// Нечёткий поиск заметок для Ctrl+O.
#[tauri::command]
pub async fn switcher_search<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: SwitcherSearchRequest,
) -> CoreResult<SwitcherSearchResponse> {
    let open = core.require_vault(window.label())?;
    let items = read(&open.switcher).search(&request.query)?;
    Ok(SwitcherSearchResponse { items })
}
