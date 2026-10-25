//! Вики-ссылки: разрешение, кратчайший различающий путь, подсказки —
//! интерфейс модуля, реализация в задаче 1.1.
//!
//! Правила — спецификация 0.1, С3.4: разрешение по имени файла без `.md` по
//! ключу имени (модуль `names`); ссылка с путём ведёт ровно на этот файл;
//! ссылка без пути при дубликатах — на самую старую по дате создания;
//! `[[/Встреча]]` — заметка в корне. Модуль чистый: работает со списком
//! заметок в памяти, к диску не обращается.

use crate::commands::{CoreError, CoreResult, LinkSuggestion, VaultPath};

/// Заметка в списке, по которому разрешаются ссылки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    /// Путь заметки (с `.md`).
    pub path: VaultPath,
    /// Дата создания (или «дата появления», ADR 0006), миллисекунды UTC —
    /// для выбора самой старой заметки при дубликатах.
    pub created_ms: i64,
}

/// Ссылки одного хранилища. Список заметок заменяется целиком при изменении
/// дерева (`replace_notes`); запросы не меняют состояние.
#[derive(Debug, Default)]
pub struct Links {
    #[allow(dead_code)]
    notes: Vec<NoteRef>,
}

impl Links {
    /// Ссылки по списку заметок хранилища.
    pub fn new(notes: Vec<NoteRef>) -> Self {
        Self { notes }
    }

    /// Заменяет список заметок (после изменения дерева).
    pub fn replace_notes(&mut self, notes: Vec<NoteRef>) {
        self.notes = notes;
    }

    /// Куда ведёт цель ссылки `target` (текст между `[[` и `|`/`#`) из
    /// заметки `from`. `Ok(None)` — неразрешённая ссылка.
    pub fn resolve(&self, _from: &VaultPath, _target: &str) -> CoreResult<Option<VaultPath>> {
        Err(CoreError::not_implemented())
    }

    /// Подсказки автодополнения по части имени: вставляемый текст — имя для
    /// уникальных заметок, кратчайший различающий путь для повторяющихся.
    pub fn suggest(&self, _from: &VaultPath, _query: &str) -> CoreResult<Vec<LinkSuggestion>> {
        Err(CoreError::not_implemented())
    }

    /// Кратчайший различающий путь заметки без `.md`: `Встреча`,
    /// `Альфа/Встреча` или `/Встреча` для заметки в корне.
    pub fn shortest_unique_path(&self, _note: &VaultPath) -> CoreResult<String> {
        Err(CoreError::not_implemented())
    }
}
