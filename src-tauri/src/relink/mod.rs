//! Обновление ссылок по правилу «ссылка ведёт туда же» (выпуск 0.2, С2-3.5,
//! С2-3.6, С2-5.4; Р2, Р3, Р8, Р11).
//!
//! Три шага, каждый проверяется отдельно (план 0.2, Т4):
//! - [`plan`] — чистая функция индекса, списка заметок «до» и переименований:
//!   какие ссылки каких заметок переписать и на какую цель (0.2-4.3);
//! - [`rewrite`] — чистая функция текста: новые цели только в местах целей,
//!   все остальные байты как были (0.2-4.4);
//! - выполнение плана с журналом `.rhizomind/link-update.json`, атомарной
//!   записью с проверкой версии и доделкой после сбоя (0.2-4.5); знаки
//!   планов для интерфейса (0.2-4.6).
//!
//! План считает только ядро; интерфейс получает число заметок и ссылок и
//! знак плана (`commands::RelinkToken`) и может лишь подтвердить или
//! отклонить его (С2-6 «Граница»).
//!
//! Задача 0.2-0.1 — типы и сигнатуры: план пока пустой; переписывание
//! текста — [`rewrite`] (0.2-4.4).

use crate::commands::{PathRename, VaultPath};
use crate::index::VaultIndex;
use crate::links::Links;

mod rewrite;

pub use rewrite::rewrite;

/// Одна ссылка плана: на какую цель переписать и куда она должна вести.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedLink {
    /// Цель ссылки до изменения, как в файле.
    pub old_target: String,
    /// Новая цель — как вставило бы автодополнение (Р3).
    pub new_target: String,
    /// Заметка, на которую ссылка вела и должна вести после изменения:
    /// при переписывании меняются только ссылки, которые на свежем тексте
    /// всё ещё ведут на неё (С2-5.4, шаг 4).
    pub points_to: VaultPath,
}

/// Заметка плана и её ссылки для переписывания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePlan {
    /// Заметка, в которой переписываются ссылки (путь после изменения).
    pub path: VaultPath,
    /// Ссылки этой заметки.
    pub links: Vec<PlannedLink>,
}

/// План переписывания ссылок.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelinkPlan {
    /// Заметки плана; заметка без ссылок для переписывания сюда не попадает.
    pub notes: Vec<NotePlan>,
}

impl RelinkPlan {
    /// Переписывать нечего.
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Сколько заметок будет переписано.
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// Сколько ссылок будет переписано.
    pub fn link_count(&self) -> usize {
        self.notes.iter().map(|note| note.links.len()).sum()
    }
}

/// План для переименований `renames` (заметок и папок): ссылки на
/// переименованное, ссылки на другие заметки, которые «перескочили» бы, и
/// ставшие неоднозначными (С2-3.5, С2-3.6). `before` — список заметок до
/// изменения; список «после» модуль строит сам. Диск не читается.
///
/// Заглушка 0.2-0.1: пустой план (0.2-4.3).
pub fn plan(index: &VaultIndex, before: &Links, renames: &[PathRename]) -> RelinkPlan {
    let _ = (index, before, renames);
    RelinkPlan::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plan_counts_nothing() {
        let plan = plan(&VaultIndex::new(), &Links::default(), &[]);
        assert!(plan.is_empty());
        assert_eq!((plan.note_count(), plan.link_count()), (0, 0));
    }
}
