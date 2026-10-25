//! Команды группы «Ссылки и поиск».

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};

use super::error::{CoreError, CoreResult};
use super::limits;
use super::types::VaultPath;
use super::{Core, blocking, checked_path};

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
    /// Что вставить между `[[` и `]]`: имя или кратчайший различающий путь;
    /// `null`, если на заметку нельзя сослаться (`linkable: false`).
    pub insert: Option<String>,
    /// Имя заметки без `.md` — для показа в списке.
    pub name: String,
    /// Заметка.
    pub path: VaultPath,
    /// Можно ли сослаться: `false`, если во вставляемом тексте есть `#`, `[`,
    /// `]` или имя с пробелом в начале (С3.2). Интерфейс показывает такую
    /// заметку помеченной и ничего не вставляет.
    pub linkable: bool,
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

/// Цель, которую нет смысла отдавать модулю: пустая, с управляющими
/// символами или длиннее любого допустимого пути. Такая ссылка —
/// неразрешённая, а не ошибка всего ответа.
fn is_plausible_target(target: &str) -> bool {
    let max_utf16 = limits::MAX_PATH_COMPONENTS * (limits::MAX_COMPONENT_UTF16 + 1);
    !target.trim().is_empty()
        && !target.chars().any(char::is_control)
        && target.encode_utf16().count() <= max_utf16
}

/// Разрешение вики-ссылок заметки. Плохая или неразрешимая цель — `path:
/// null` для этой цели; ответ целиком отклоняется только при нарушении
/// пределов запроса или отказе модуля.
#[tauri::command]
pub async fn links_resolve<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksResolveRequest,
) -> CoreResult<LinksResolveResponse> {
    checked_path(&request.from, "from")?;
    limits::link_targets(request.targets.len())?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let index = open.read_index();
        let links = index.links();
        let results = request
            .targets
            .into_iter()
            .map(|target| {
                let path = if is_plausible_target(&target) {
                    links.resolve(&request.from, &target)?
                } else {
                    None
                };
                Ok(ResolvedLink { target, path })
            })
            .collect::<Result<_, CoreError>>()?;
        Ok(LinksResolveResponse { results })
    })
    .await
}

/// Подсказки автодополнения вики-ссылки.
#[tauri::command]
pub async fn links_suggest<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksSuggestRequest,
) -> CoreResult<LinksSuggestResponse> {
    checked_path(&request.from, "from")?;
    limits::query(&request.query)?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let items = open
            .read_index()
            .links()
            .suggest(&request.from, &request.query)?;
        Ok(LinksSuggestResponse { items })
    })
    .await
}

/// Нечёткий поиск заметок для Ctrl+O.
#[tauri::command]
pub async fn switcher_search<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: SwitcherSearchRequest,
) -> CoreResult<SwitcherSearchResponse> {
    limits::query(&request.query)?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        let items = open.read_index().switcher().search(&request.query)?;
        Ok(SwitcherSearchResponse { items })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::is_plausible_target;

    #[test]
    fn implausible_targets_are_unresolved_not_errors() {
        assert!(is_plausible_target("Альфа/Встреча"));
        assert!(is_plausible_target("/Встреча"));
        assert!(!is_plausible_target(""));
        assert!(!is_plausible_target("   "));
        assert!(!is_plausible_target("a\nb"));
        assert!(!is_plausible_target(&"я".repeat(64 * 256 + 1)));
    }
}
