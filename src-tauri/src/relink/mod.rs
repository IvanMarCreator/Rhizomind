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
//! План — [`plan()`] (0.2-4.3), переписывание текста — [`rewrite`] (0.2-4.4),
//! выполнение плана — [`run()`], доделка при открытии хранилища —
//! [`resume_on_open`] (0.2-4.5), знаки планов — [`tokens`] (0.2-4.6).

use crate::commands::{PathRename, VaultPath};

pub mod duplicates;
pub mod external;
mod journal;
#[cfg(test)]
mod perf_tests;
mod plan;
mod rewrite;
mod run;
pub mod tokens;

pub use plan::{plan, plan_moves};
pub use rewrite::rewrite;
pub use run::{RunReport, resume_on_open, run};

/// Тестовый шов падения посреди плана (сценарий доделки при открытии
/// хранилища, `commands::core_tests`).
#[cfg(test)]
pub(crate) use run::crash_after;

/// Есть ли на диске журнал прерванного обновления ссылок (дешёвая проверка
/// перед доделкой: без журнала список заметок для неё не строится).
pub fn journal_pending(vault: &crate::vault::Vault) -> bool {
    !matches!(
        journal::read(vault),
        journal::Read::Missing | journal::Read::Refused
    )
}

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
    /// Переименования, которые не удалось сопоставить со списком заметок
    /// «до» (нечего переименовывать, «куда» занято без пометки о замене,
    /// повтор уже выполненного): по ним ничего не переписано, вызывающий
    /// сообщает о сбое, а не молчит (план 0.2, «Дополнительно» к 0.2-4.3).
    pub unmatched: Vec<PathRename>,
}

impl RelinkPlan {
    /// Переписывать нечего и все переименования сопоставлены. План без
    /// заметок, но с несопоставленными переименованиями, — не пустой.
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.unmatched.is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VaultIndex;
    use crate::links::Links;

    #[test]
    fn empty_plan_counts_nothing() {
        let plan = plan(&VaultIndex::new(), &Links::default(), &[], &[]);
        assert!(plan.is_empty());
        assert_eq!((plan.note_count(), plan.link_count()), (0, 0));
    }
}
