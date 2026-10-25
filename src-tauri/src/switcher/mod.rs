//! Быстрый переход (Ctrl+O): нечёткий поиск по именам и путям заметок —
//! интерфейс модуля, реализация в задаче 1.1 (`nucleo-matcher`).
//!
//! Имена сравниваются по тому же ключу имени, что и ссылки (модуль `names`).
//! В интерфейс уходит не больше [`MAX_RESULTS`] лучших совпадений.

use crate::commands::{CoreError, CoreResult, SwitcherItem, VaultPath};

/// Сколько совпадений возвращается интерфейсу.
pub const MAX_RESULTS: usize = 50;

/// Поиск по заметкам одного хранилища. Список заменяется целиком при
/// изменении дерева.
#[derive(Debug, Default)]
pub struct Switcher {
    #[allow(dead_code)]
    notes: Vec<VaultPath>,
}

impl Switcher {
    /// Поиск по списку заметок хранилища.
    pub fn new(notes: Vec<VaultPath>) -> Self {
        Self { notes }
    }

    /// Заменяет список заметок (после изменения дерева).
    pub fn replace_notes(&mut self, notes: Vec<VaultPath>) {
        self.notes = notes;
    }

    /// До [`MAX_RESULTS`] заметок, лучшие первыми. Пустой запрос — недавние
    /// по порядку списка. Цель по скорости: 20 мс на 10 000 заметок (план 1.1).
    pub fn search(&self, _query: &str) -> CoreResult<Vec<SwitcherItem>> {
        Err(CoreError::not_implemented())
    }
}
